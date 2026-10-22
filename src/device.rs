//! Background engine (feature `device`): owns the MIDI connection (or a capture replay), finds
//! and connects to the pedal, performs the startup sync, sends edits and reports every message.
//!
//! [`Device::spawn`] starts the engine on its own thread and returns a [`Device`] handle. Send it
//! [`Command`]s (or use the helper methods) and read [`Update`]s from it. The engine only moves
//! bytes; apply [`Update::Message`]s to a [`DeviceState`](crate::DeviceState) to follow the
//! pedal's state, and reset that state on [`Update::Reset`].
//!
//! SysEx requests and writes go through a [`RequestQueue`] (one in flight, 1 s reply timeout,
//! docs/03). Live CCs/PCs ([`Device::send`]) bypass it. The pedal never echoes host messages, so
//! every message the engine sends is reported as an [`Update::Message`] with
//! [`Origin::Host`]; applying those keeps a `DeviceState` in step with your own edits.
//!
//! Dropping the [`Device`] disconnects and stops the engine thread.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvError, RecvTimeoutError, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

use crate::capture::{self, Entry};
use crate::catalog::Module;
use crate::midi::{Msg, hex, host, req};
use crate::queue::{RequestQueue, follow_up, handshake};
use crate::records::{Preset, Settings};
use crate::upload::{self, UploadError, UserAmpInfo};
use crate::{Assembler, Origin, is_stomp_port};

/// Engine settings.
#[derive(Clone, Debug)]
pub struct Config {
    /// MIDI client name shown by the OS (ALSA sequencer, CoreMIDI).
    pub client_name: String,
    /// Connect to the first pedal-looking port found, and again whenever it is plugged back in.
    /// [`Command::Disconnect`] turns this off, [`Command::Connect`] turns it back on.
    pub auto_connect: bool,
    /// Whether the engine may send anything (sync reads and edits). `false` = listen only.
    pub sync: bool,
    /// How long to wait for a SysEx reply or ack before moving on.
    pub reply_timeout: Duration,
    /// How often to rescan the MIDI ports (hotplug).
    pub scan_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            client_name: "Open Stomp".into(),
            auto_connect: true,
            sync: true,
            reply_timeout: Duration::from_secs(1),
            scan_interval: Duration::from_millis(1500),
        }
    }
}

pub enum Command {
    /// Connect to a MIDI input port by name (and the matching output).
    Connect(String),
    Disconnect,
    /// Read everything again (sends [`Update::Reset`] first).
    Resync,
    /// Whether the engine may send anything (sync reads and edits). Off = pure listening.
    SetSync(bool),
    /// Send a live message (CC/PC) right away.
    Send(Vec<u8>),
    /// Queue messages to send in order; SysEx writes wait for the pedal's ack.
    Write(Vec<Vec<u8>>),
    /// Play a capture log (docs/07) back instead of a pedal, at `speed`× its original timing.
    Replay { path: PathBuf, speed: f32 },
    StopReplay,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Disconnected,
    Connected {
        port: String,
        /// An output to the pedal was opened; without one the engine only listens.
        output: bool,
    },
    Replaying {
        file: String,
        progress: f32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    /// The MIDI input ports changed (sent on start and on every hotplug).
    Ports(Vec<String>),
    Status(Status),
    /// Startup sync progress; `(0, 0)` when no sync is running.
    Sync { done: usize, total: usize },
    /// One complete MIDI message, either direction.
    Message { at: SystemTime, origin: Origin, bytes: Vec<u8> },
    /// A `# MARK` annotation from a replayed capture.
    Mark { at: SystemTime, text: String },
    /// A note for the user: connection changes, timeouts, refused sends.
    Info { at: SystemTime, text: String, error: bool },
    /// A new session starts (connect, resync or replay): reset your device model.
    Reset,
}

enum Internal {
    Cmd(Command),
    Input(Vec<u8>),
    Shutdown,
}

type Waker = Arc<dyn Fn() + Send + Sync>;

/// Handle to the engine thread. Cheap methods; nothing blocks except the `recv*` calls.
pub struct Device {
    tx: Sender<Internal>,
    rx: Receiver<Update>,
    thread: Option<JoinHandle<()>>,
}

impl Device {
    /// Start the engine with no wake-up callback (poll or block on [`recv`](Device::recv)).
    pub fn spawn(config: Config) -> Device {
        Device::spawn_with_waker(config, || {})
    }

    /// Start the engine; `wake` runs after every batch of updates (e.g. to repaint a GUI).
    pub fn spawn_with_waker(config: Config, wake: impl Fn() + Send + Sync + 'static) -> Device {
        let (tx, rx_internal) = mpsc::channel();
        let (tx_updates, rx) = mpsc::channel();
        let engine_tx = tx.clone();
        let wake: Waker = Arc::new(wake);
        let thread = thread::Builder::new()
            .name("open-stomp-engine".into())
            .spawn(move || Engine::new(config, tx_updates, engine_tx, wake).run(rx_internal))
            .expect("spawn engine thread");
        Device { tx, rx, thread: Some(thread) }
    }

    pub fn command(&self, c: Command) {
        let _ = self.tx.send(Internal::Cmd(c));
    }

    pub fn try_recv(&self) -> Result<Update, TryRecvError> {
        self.rx.try_recv()
    }

    pub fn recv(&self) -> Result<Update, RecvError> {
        self.rx.recv()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Update, RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }

    /// Every update available right now, without blocking.
    pub fn drain(&self) -> impl Iterator<Item = Update> + '_ {
        self.rx.try_iter()
    }

    pub fn connect(&self, port: impl Into<String>) {
        self.command(Command::Connect(port.into()));
    }

    pub fn disconnect(&self) {
        self.command(Command::Disconnect);
    }

    pub fn resync(&self) {
        self.command(Command::Resync);
    }

    pub fn set_sync(&self, on: bool) {
        self.command(Command::SetSync(on));
    }

    pub fn replay(&self, path: impl Into<PathBuf>, speed: f32) {
        self.command(Command::Replay { path: path.into(), speed });
    }

    pub fn stop_replay(&self) {
        self.command(Command::StopReplay);
    }

    /// Send a live message (CC/PC) now.
    pub fn send(&self, bytes: Vec<u8>) {
        self.command(Command::Send(bytes));
    }

    /// Queue messages behind any pending SysEx (writes wait for their ack).
    pub fn write(&self, msgs: Vec<Vec<u8>>) {
        self.command(Command::Write(msgs));
    }

    /// Select preset `index` (0-based, P01 = 0).
    pub fn select_preset(&self, index: u8) {
        self.send(host::program(index));
    }

    /// Set a module's model and on/off. Ignored for LOOP/DRUM (no head CC).
    pub fn set_head(&self, m: Module, model: u8, on: bool) {
        if let Some(b) = host::head(m, model, on) {
            self.send(b);
        }
    }

    /// Set a module parameter by slot (app display order). Ignored for slots the module lacks.
    pub fn set_param(&self, m: Module, slot: usize, value: u8) {
        if let Some(b) = host::param(m, slot, value) {
            self.send(b);
        }
    }

    pub fn set_volume(&self, value: u8) {
        self.send(host::volume(value));
    }

    /// Make the pedal display jump to a module's page.
    pub fn show_page(&self, m: Module) {
        self.send(host::page(m));
    }

    /// Press a looper button (`host::LOOP_*`).
    pub fn looper_button(&self, button: u8) {
        self.send(host::looper_button(button));
    }

    /// Store `preset` in slot `preset.index` (with its name) and select it, using the official
    /// app's sequence. `source_slot` is the active preset the edits were made on.
    pub fn save_preset(&self, source_slot: u8, preset: &Preset) {
        self.write(host::save_preset(source_slot, preset));
    }

    /// Import converted NAM data into user AMP `slot` (0–9) the way the official app does: select
    /// the slot's amp model, write the slot-info record (name, factor), then the 2048 floats.
    /// Converting a `.nam` file into those floats is not implemented yet (docs/05).
    pub fn import_user_amp(&self, slot: u8, name: &str, data: &[f32], factor: f32) -> Result<(), UploadError> {
        upload::check(slot, data)?;
        if let Some(b) = host::head(Module::Amp, upload::USER_MODEL_BASE + slot, true) {
            self.send(b);
        }
        self.write(upload::user_amp_upload(&UserAmpInfo::new(slot, name, factor), data));
        Ok(())
    }

    /// Convert a `.nam` file with the best conversion, [`Measure::Optimised`](crate::nam::convert::Measure::Optimised)
    /// (closer to the model on hardware than the official app's), and import it into user AMP
    /// `slot` (0–9), named after the file like the official app does. Never reads the official app
    /// (see [`crate::nam::convert::official_excitation`] to opt in). The conversion runs on the
    /// calling thread and takes about a minute (optimised build); [`Device::import_nam_file_with`]
    /// with `Measure::App` gives the official algorithm in a few seconds. Returns the conversion.
    #[cfg(feature = "nam")]
    pub fn import_nam_file(
        &self,
        slot: u8,
        path: impl AsRef<std::path::Path>,
    ) -> Result<crate::nam::Converted, Box<dyn std::error::Error + Send + Sync>> {
        self.import_nam_file_with(slot, path, crate::nam::convert::Measure::Optimised)
    }

    /// [`Device::import_nam_file`] with a choice of conversion
    /// ([`Measure`](crate::nam::convert::Measure)): `Optimised` (the default, about a minute),
    /// `App` (the official algorithm, a few seconds) or `Clean`.
    #[cfg(feature = "nam")]
    pub fn import_nam_file_with(
        &self,
        slot: u8,
        path: impl AsRef<std::path::Path>,
        measure: crate::nam::convert::Measure,
    ) -> Result<crate::nam::Converted, Box<dyn std::error::Error + Send + Sync>> {
        let path = path.as_ref();
        if slot >= upload::USER_SLOTS {
            return Err(Box::new(UploadError::Slot(slot)));
        }
        let mut model = crate::nam::Model::load(path)?;
        let converted =
            crate::nam::convert::convert_measured(&mut model, &crate::nam::convert::excitation(), measure);
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.import_user_amp(slot, &name, &converted.data, converted.factor)?;
        Ok(converted)
    }

    /// Delete user AMP `slot` and re-read its info, like the official app.
    pub fn delete_user_amp(&self, slot: u8) -> Result<(), UploadError> {
        if slot >= upload::USER_SLOTS {
            return Err(UploadError::Slot(slot));
        }
        self.write(vec![upload::user_amp_delete(slot), req::user_amp(slot)]);
        Ok(())
    }

    /// Import IR data into user IR `slot` (0–9) the way the official app does: show the CAB page,
    /// select the slot's cab model, write the 2048 floats, then the name. Only slot 0 has been
    /// captured; other slots are assumed to work the same way.
    pub fn import_user_ir(&self, slot: u8, name: &str, data: &[f32]) -> Result<(), UploadError> {
        upload::check(slot, data)?;
        self.send(host::page(Module::Cab));
        if let Some(b) = host::head(Module::Cab, upload::USER_MODEL_BASE + slot, true) {
            self.send(b);
        }
        self.write(upload::user_ir_upload(slot, name, data));
        Ok(())
    }

    /// Write the global settings (read-modify-write: start from the pedal's current record).
    pub fn write_settings(&self, settings: &Settings) {
        self.write(vec![host::settings_write(settings)]);
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let _ = self.tx.send(Internal::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Connection {
    port: String,
    _input: MidiInputConnection<()>,
    output: Option<MidiOutputConnection>,
}

struct Engine {
    config: Config,
    out: Sender<Update>,
    self_tx: Sender<Internal>,
    wake: Waker,
    scanner: Option<MidiInput>,
    ports: Vec<String>,
    last_scan: Option<Instant>,
    conn: Option<Connection>,
    replay: Option<Arc<AtomicBool>>,
    asm: Assembler,
    queue: RequestQueue,
}

impl Engine {
    fn new(config: Config, out: Sender<Update>, self_tx: Sender<Internal>, wake: Waker) -> Self {
        let scanner = MidiInput::new(&format!("{} scan", config.client_name)).ok();
        let queue = RequestQueue::new(config.reply_timeout);
        Engine {
            config,
            out,
            self_tx,
            wake,
            scanner,
            ports: vec![],
            last_scan: None,
            conn: None,
            replay: None,
            asm: Assembler::default(),
            queue,
        }
    }

    fn emit(&self, u: Update) {
        let _ = self.out.send(u);
        (self.wake)();
    }

    fn info(&self, text: impl Into<String>, error: bool) {
        self.emit(Update::Info { at: SystemTime::now(), text: text.into(), error });
    }

    fn emit_progress(&self) {
        let (done, total) = self.queue.progress();
        self.emit(Update::Sync { done, total });
    }

    fn run(mut self, rx: Receiver<Internal>) {
        if self.scanner.is_none() {
            self.info("MIDI is unavailable on this system (could not create a MIDI client)", true);
        }
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Internal::Cmd(c)) => self.command(c),
                Ok(Internal::Input(data)) => self.input(&data),
                Ok(Internal::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.pump();
            #[cfg(target_os = "macos")]
            run_loop_tick();
            if self.last_scan.is_none_or(|t| t.elapsed() > self.config.scan_interval) {
                self.scan();
            }
        }
        self.stop_replay();
        self.conn = None;
    }

    fn command(&mut self, c: Command) {
        match c {
            Command::Connect(port) => {
                self.stop_replay();
                self.config.auto_connect = true;
                self.connect(&port);
            }
            Command::Disconnect => {
                self.config.auto_connect = false;
                self.disconnect("Disconnected");
            }
            Command::Resync => {
                if self.conn.is_some() {
                    self.emit(Update::Reset);
                    self.start_sync();
                }
            }
            Command::SetSync(on) => {
                self.config.sync = on;
                if !on {
                    self.queue.clear();
                    self.emit_progress();
                }
            }
            Command::Send(bytes) => {
                if self.writable() {
                    self.transmit(&bytes);
                }
            }
            Command::Write(msgs) => {
                if self.writable() {
                    for bytes in msgs {
                        self.queue.write(bytes);
                    }
                }
            }
            Command::Replay { path, speed } => {
                self.config.auto_connect = false;
                self.disconnect("Disconnected for replay");
                self.start_replay(path, speed);
            }
            Command::StopReplay => {
                if self.stop_replay() {
                    self.emit(Update::Status(Status::Disconnected));
                }
            }
        }
    }

    fn scan(&mut self) {
        self.last_scan = Some(Instant::now());
        let Some(scanner) = &self.scanner else { return };
        let ports: Vec<String> = scanner.ports().iter().filter_map(|p| scanner.port_name(p).ok()).collect();
        if ports != self.ports {
            self.ports = ports.clone();
            self.emit(Update::Ports(ports.clone()));
        }
        if let Some(c) = &self.conn
            && !ports.contains(&c.port)
        {
            self.disconnect("Pedal unplugged");
        }
        if self.conn.is_none()
            && self.replay.is_none()
            && self.config.auto_connect
            && let Some(p) = ports.iter().find(|p| is_stomp_port(p))
        {
            self.connect(&p.clone());
        }
    }

    fn connect(&mut self, port_name: &str) {
        self.disconnect_quiet();
        let result = (|| -> Result<Connection, String> {
            let mut input = MidiInput::new(&self.config.client_name).map_err(|e| e.to_string())?;
            input.ignore(Ignore::None);
            let port = input
                .ports()
                .into_iter()
                .find(|p| input.port_name(p).as_deref() == Ok(port_name))
                .ok_or_else(|| format!("port “{port_name}” not found"))?;
            let tx = self.self_tx.clone();
            let input = input
                .connect(
                    &port,
                    "stomp-in",
                    move |_ts, data, _| {
                        let _ = tx.send(Internal::Input(data.to_vec()));
                    },
                    (),
                )
                .map_err(|e| e.to_string())?;
            let output = open_output(&self.config.client_name, port_name);
            Ok(Connection { port: port_name.to_owned(), _input: input, output })
        })();
        match result {
            Ok(c) => {
                let output = c.output.is_some();
                let port = c.port.clone();
                self.conn = Some(c);
                self.emit(Update::Reset);
                self.emit(Update::Status(Status::Connected { port: port.clone(), output }));
                self.info(format!("Connected to {port}"), false);
                if !output {
                    self.info("No MIDI output to the pedal: listening only", true);
                }
                self.start_sync();
            }
            Err(e) => self.info(format!("Could not connect: {e}"), true),
        }
    }

    fn disconnect_quiet(&mut self) {
        self.conn = None;
        self.queue.clear();
        self.asm = Assembler::default();
    }

    fn disconnect(&mut self, why: &str) {
        if self.conn.is_some() {
            self.disconnect_quiet();
            self.emit(Update::Status(Status::Disconnected));
            self.emit_progress();
            self.info(why, false);
        }
    }

    fn has_output(&self) -> bool {
        self.conn.as_ref().is_some_and(|c| c.output.is_some())
    }

    fn start_sync(&mut self) {
        self.queue.clear();
        if !self.config.sync || !self.has_output() {
            return;
        }
        self.queue.start_sync(handshake());
        self.emit_progress();
    }

    /// Edits are allowed when connected with an output and not in listen-only mode.
    fn writable(&self) -> bool {
        let ok = self.config.sync && self.has_output();
        if !ok {
            self.info("Not sent: listen-only mode, or no MIDI output to the pedal", true);
        }
        ok
    }

    /// Send one message now and report it. The pedal never echoes host messages.
    fn transmit(&mut self, bytes: &[u8]) -> bool {
        let Some(out) = self.conn.as_mut().and_then(|c| c.output.as_mut()) else { return false };
        if let Err(e) = out.send(bytes) {
            self.info(format!("Send failed: {e}"), true);
            return false;
        }
        self.emit(Update::Message { at: SystemTime::now(), origin: Origin::Host, bytes: bytes.to_vec() });
        true
    }

    /// Send queued requests while nothing is in flight; expire a timed-out request.
    fn pump(&mut self) {
        if let Some(done) = self.queue.expire(Instant::now()) {
            let b = done.request.bytes();
            self.info(format!("No reply from pedal to {} …", hex(&b[..b.len().min(8)])), true);
            if done.request.is_sync() {
                self.emit_progress();
            }
        }
        while let Some(r) = self.queue.pop() {
            if !self.has_output() {
                self.queue.clear();
                return;
            }
            if !self.transmit(r.bytes()) {
                return;
            }
            self.queue.sent(r, Instant::now());
        }
    }

    fn input(&mut self, data: &[u8]) {
        if self.conn.is_none() {
            return;
        }
        let at = SystemTime::now();
        for bytes in self.asm.push(data) {
            let msg = Msg::parse(&bytes);
            if let Some(done) = self.queue.on_message(&msg)
                && done.request.is_sync()
            {
                self.emit_progress();
            }
            if self.config.sync {
                for r in follow_up(&msg) {
                    self.queue.read(r);
                }
            }
            self.emit(Update::Message { at, origin: Origin::Pedal, bytes });
        }
    }

    fn start_replay(&mut self, path: PathBuf, speed: f32) {
        self.stop_replay();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                self.info(format!("Could not read {}: {e}", path.display()), true);
                return;
            }
        };
        let entries = capture::parse(&text);
        let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let stop = Arc::new(AtomicBool::new(false));
        self.replay = Some(stop.clone());
        self.emit(Update::Reset);
        self.emit(Update::Status(Status::Replaying { file: file.clone(), progress: 0.0 }));
        self.info(format!("Replaying {file} ({} messages) at {speed}×", entries.len()), false);
        let (out, wake) = (self.out.clone(), self.wake.clone());
        thread::spawn(move || replay(entries, file, speed, stop, out, wake));
    }

    /// Returns whether a replay was running.
    fn stop_replay(&mut self) -> bool {
        match self.replay.take() {
            Some(stop) => {
                stop.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }
}

/// CoreMIDI updates a client's view of the devices (hotplug) through notifications delivered on
/// the run loop of the thread that created the client. The engine thread has no run loop of its
/// own, so give it one turn per tick without blocking.
#[cfg(target_os = "macos")]
fn run_loop_tick() {
    use core_foundation_sys::runloop::{CFRunLoopRunInMode, kCFRunLoopDefaultMode};
    // SAFETY: plain CoreFoundation call on the current thread; returns immediately (0 s timeout).
    unsafe {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.0, 1);
    }
}

fn open_output(client: &str, port_name: &str) -> Option<MidiOutputConnection> {
    let out = MidiOutput::new(client).ok()?;
    let ports = out.ports();
    let names: Vec<String> = ports.iter().map(|p| out.port_name(p).unwrap_or_default()).collect();
    // exact name first (ALSA, CoreMIDI), else any pedal-looking port (Windows names differ)
    let i = names.iter().position(|n| n == port_name).or_else(|| names.iter().position(|n| is_stomp_port(n)))?;
    out.connect(&ports[i], "stomp-out").ok()
}

/// Play a capture log back with its original timing (gaps capped at 1.5 s), scaled by `speed`.
fn replay(entries: Vec<Entry>, file: String, speed: f32, stop: Arc<AtomicBool>, out: Sender<Update>, wake: Waker) {
    let total = entries.len().max(1);
    let mut prev_ms: Option<u64> = None;
    for (i, e) in entries.into_iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let ms = match &e {
            Entry::Mark { ms, .. } | Entry::Message { ms, .. } => *ms,
        };
        if let Some(p) = prev_ms {
            let gap = ms.saturating_sub(p).min(1500) as f32 / speed.max(0.01);
            if gap >= 1.0 {
                thread::sleep(Duration::from_millis(gap as u64));
            }
        }
        prev_ms = Some(ms);
        let at = UNIX_EPOCH + Duration::from_millis(ms);
        let u = match e {
            Entry::Mark { text, .. } => Update::Mark { at, text },
            Entry::Message { origin, bytes, .. } => Update::Message { at, origin, bytes },
        };
        if out.send(u).is_err() {
            return;
        }
        if i % 25 == 0 {
            let progress = i as f32 / total as f32;
            let _ = out.send(Update::Status(Status::Replaying { file: file.clone(), progress }));
        }
        wake();
    }
    let _ = out.send(Update::Status(Status::Replaying { file, progress: 1.0 }));
    let _ = out.send(Update::Info { at: SystemTime::now(), text: "Replay finished".into(), error: false });
    wake();
}

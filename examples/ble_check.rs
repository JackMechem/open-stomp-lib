//! End-to-end check of the pedal over Bluetooth LE MIDI (Wireless on, USB unplugged).
//!
//! Connects to the first `(Bluetooth)` port, runs the full startup sync, then exercises the
//! controls, reading every change back from the pedal and putting it back afterwards:
//! effect on/off, a parameter, volume, a fast parameter sweep (slider drag), page change, preset
//! switching (only when the live preset has no unsaved edits), unchanged rewrites of the
//! current preset and the global settings, drum level and start/stop, loop REC level, and a
//! looper record/play/stop/clear cycle (only when the looper is empty). User AMP/IR slots are never
//! touched.
//!
//! ```sh
//! cargo run --features ble --example ble_check
//! ```

use std::time::{Duration, Instant};

use open_stomp::device::{Config, Device, Status, Update};
use open_stomp::midi::{Msg, host, req};
use open_stomp::{DeviceState, Module, Origin, Preset, is_ble_port};

struct Check {
    dev: Device,
    state: DeviceState,
    port: Option<String>,
    ports: Vec<String>,
    syncing: bool,
    sync_total: usize,
    errors: Vec<String>,
    pedal: Vec<Msg>,
    failed: usize,
}

impl Check {
    fn pump(&mut self, wait: Duration) {
        let end = Instant::now() + wait;
        while let Ok(u) = self.dev.recv_timeout(end.saturating_duration_since(Instant::now())) {
            match u {
                Update::Ports(p) => {
                    println!("ports: {p:?}");
                    self.ports = p;
                }
                Update::Status(Status::Connected { port, output }) => {
                    println!("connected: {port} (output {output})");
                    self.port = Some(port);
                }
                Update::Status(s) => println!("status: {s:?}"),
                Update::Sync { done, total } => {
                    if total > 0 {
                        self.syncing = done < total;
                        self.sync_total = total;
                    } else {
                        self.syncing = false;
                    }
                }
                Update::Message { origin, bytes, .. } => {
                    self.state.apply(&bytes, origin);
                    if origin == Origin::Pedal {
                        self.pedal.push(Msg::parse(&bytes));
                    }
                }
                Update::Info { text, error, .. } => {
                    println!("{} {text}", if error { "!!" } else { "--" });
                    if error {
                        self.errors.push(text);
                    }
                }
                Update::Reset => self.state = DeviceState::default(),
                Update::Mark { .. } => {}
            }
            if Instant::now() >= end {
                break;
            }
        }
    }

    /// Ask the pedal for something and wait for the reply payload.
    fn read(&mut self, request: Vec<u8>, cmd: u8) -> Option<Vec<u8>> {
        self.pedal.clear();
        self.dev.write(vec![request]);
        let end = Instant::now() + Duration::from_secs(3);
        while Instant::now() < end {
            self.pump(Duration::from_millis(30));
            if let Some(p) = self.pedal.iter().find_map(|m| match m {
                Msg::Nep { cmd: c, dir: 2, payload } if *c == cmd => Some(payload.clone()),
                _ => None,
            }) {
                return Some(p);
            }
        }
        None
    }

    /// The pedal's live edit buffer.
    fn live(&mut self) -> Preset {
        let p = self.read(req::edit_buffer(), 0x0C).expect("edit buffer reply");
        Preset::from_packed(&p).expect("edit buffer record")
    }

    fn check(&mut self, what: &str, ok: bool) {
        println!("{} {what}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            self.failed += 1;
        }
    }

    /// Wait until the queue has sent everything (acks included).
    fn settle(&mut self) {
        self.pump(Duration::from_millis(400));
    }
}

fn main() {
    let dev = Device::spawn(Config { auto_connect: false, ..Config::default() });
    let mut c = Check {
        dev,
        state: DeviceState::default(),
        port: None,
        ports: vec![],
        syncing: false,
        sync_total: 0,
        errors: vec![],
        pedal: vec![],
        failed: 0,
    };

    // find and connect
    let end = Instant::now() + Duration::from_secs(20);
    let port = loop {
        c.pump(Duration::from_millis(200));
        if let Some(p) = c.ports.iter().find(|&p| is_ble_port(p)).cloned() {
            break p;
        }
        assert!(Instant::now() < end, "no Bluetooth pedal found (is Wireless on?)");
    };
    let t = Instant::now();
    c.dev.connect(&port);
    while c.port.is_none() {
        c.pump(Duration::from_millis(100));
        assert!(t.elapsed() < Duration::from_secs(30), "connect failed");
    }
    println!("connect took {:.1} s", t.elapsed().as_secs_f32());

    // startup sync
    let t = Instant::now();
    c.syncing = true;
    while c.syncing && t.elapsed() < Duration::from_secs(120) {
        c.pump(Duration::from_millis(100));
    }
    let presets = c.state.presets.iter().filter(|p| p.is_some()).count();
    println!(
        "sync: {} requests in {:.1} s, {} presets, firmware {:?}, product {:?}",
        c.sync_total,
        t.elapsed().as_secs_f32(),
        presets,
        c.state.firmware,
        c.state.product
    );
    let sync_errors = c.errors.len();
    c.check("sync finished without timeouts", !c.syncing && sync_errors == 0);
    c.check("all presets read", presets == open_stomp::records::PRESET_COUNT);
    c.check("settings read", c.state.settings.is_some());

    let base = c.live();
    println!("live preset: {:?} (current {:?}, modified {})", base.name, c.state.current, c.state.modified());

    // effect on/off
    let rev = *base.module(Module::Reverb).unwrap();
    c.dev.set_head(Module::Reverb, rev.model, !rev.on);
    c.settle();
    let got = c.live().module(Module::Reverb).unwrap().on;
    c.check("reverb on/off", got != rev.on);
    c.dev.set_head(Module::Reverb, rev.model, rev.on);
    c.settle();
    let got = c.live().module(Module::Reverb).unwrap().on;
    c.check("reverb restored", got == rev.on);

    // parameter
    let amp = *base.module(Module::Amp).unwrap();
    let v = amp.params[0];
    let nv = if v < 100 { v + 1 } else { v - 1 };
    c.dev.set_param(Module::Amp, 0, nv);
    c.settle();
    let got = c.live().module(Module::Amp).unwrap().params[0];
    c.check(&format!("amp param 1: {v} -> {nv} (read {got})"), got == nv);

    // fast sweep, like dragging a slider: 60 CCs, no gaps, ending on the original value
    for i in 0..60u8 {
        c.dev.set_param(Module::Amp, 0, (v + i) % 101);
    }
    c.dev.set_param(Module::Amp, 0, v);
    c.settle();
    let got = c.live().module(Module::Amp).unwrap().params[0];
    c.check(&format!("amp param sweep ends on {v} (read {got})"), got == v);

    // a burst that can't be coalesced: two different CCs alternating, 20 messages
    for i in 0..10u8 {
        c.dev.set_param(Module::Amp, 0, (v + i) % 101);
        c.dev.set_volume(base.volume);
    }
    c.dev.set_param(Module::Amp, 0, v);
    c.pump(Duration::from_millis(800));
    let got = c.live().module(Module::Amp).unwrap().params[0];
    c.check(&format!("alternating burst ends on {v} (read {got})"), got == v);

    // volume
    let vol = base.volume;
    let nvol = if vol < 100 { vol + 1 } else { vol - 1 };
    c.dev.set_volume(nvol);
    c.settle();
    let got = c.live().volume;
    c.check(&format!("volume {vol} -> {nvol} (read {got})"), got == nvol);
    c.dev.set_volume(vol);
    c.settle();
    let got = c.live().volume;
    c.check("volume restored", got == vol);

    // page change (no read-back; the pedal shows the page)
    c.dev.show_page(Module::Amp);
    c.settle();
    c.check("page change sent", c.errors.len() == sync_errors);

    // preset switching, only when nothing unsaved would be lost
    let live_now = c.live();
    match c.state.current {
        Some(cur) if c.state.stored(cur).is_some_and(|p| p.same_sound(&live_now)) => {
            let other = if cur < 98 { cur + 1 } else { cur - 1 };
            let want = c.state.stored(other).map(|p| p.name.clone()).unwrap_or_default();
            c.dev.select_preset(other);
            c.pump(Duration::from_millis(800));
            let got = c.live().name;
            c.check(&format!("select preset {} ({want:?}, read {got:?})", other + 1), got == want);
            c.dev.select_preset(cur);
            c.pump(Duration::from_millis(800));
            let got = c.live().name;
            c.check(&format!("back to preset {} ({got:?})", cur + 1), got == base.name);
        }
        cur => println!("SKIP preset switching: current {cur:?} unknown or has unsaved edits"),
    }

    // unchanged rewrite of the stored preset and the settings (acked writes)
    if let Some(cur) = c.state.current
        && let Some(stored) = c.state.stored(cur).cloned()
    {
        let before = c.errors.len();
        c.dev.write(vec![host::preset_write(&stored)]);
        c.settle();
        let back = c.read(req::preset(cur), 0x0B).and_then(|p| Preset::from_packed(&p));
        c.check(
            "preset rewrite acked and unchanged",
            c.errors.len() == before && back.is_some_and(|b| b.raw == stored.raw),
        );
    }
    if let Some(s) = c.state.settings.clone() {
        let before = c.errors.len();
        c.dev.write_settings(&s);
        c.settle();
        let back = c.read(req::settings(), 0x14).and_then(|p| open_stomp::records::Settings::from_packed(&p));
        c.check(
            "settings rewrite acked and unchanged",
            c.errors.len() == before && back.is_some_and(|b| b.raw == s.raw),
        );
    }

    // drum: level, start/stop
    if let Some(d) = c.read(req::drum(), 0x19) {
        let lvl = d[2];
        let nl = if lvl < 100 { lvl + 1 } else { lvl - 1 };
        c.dev.send(host::cc(0x41, nl));
        c.settle();
        let got = c.read(req::drum(), 0x19).map(|d| d[2]);
        c.check(&format!("drum level {lvl} -> {nl} (read {got:?})"), got == Some(nl));
        c.dev.send(host::cc(0x41, lvl));
        c.settle();
        let got = c.read(req::drum(), 0x19).map(|d| d[2]);
        c.check("drum level restored", got == Some(lvl));
    }
    c.dev.send(host::cc(0x3F, 1));
    c.pump(Duration::from_millis(1500));
    c.dev.send(host::cc(0x3F, 0));
    c.settle();
    let alive = c.read(req::drum(), 0x19).is_some();
    c.check("drum start/stop (pedal still answering)", alive);

    // looper: REC level, then rec/play/stop/clear only when the looper is empty
    if let Some(l) = c.read(req::looper(), 0x18) {
        let lvl = l[1];
        let nl = if lvl < 100 { lvl + 1 } else { lvl - 1 };
        c.dev.send(host::cc(0x42, nl));
        c.settle();
        let got = c.read(req::looper(), 0x18).map(|l| l[1]);
        c.check(&format!("loop REC level {lvl} -> {nl} (read {got:?})"), got == Some(nl));
        c.dev.send(host::cc(0x42, lvl));
        c.settle();
        let got = c.read(req::looper(), 0x18).map(|l| l[1]);
        c.check("loop REC level restored", got == Some(lvl));
        if l[3] & 0x0F == 0 {
            // like the official app: show the loop page first; the pedal reports the state as CC 0x44
            c.dev.show_page(Module::Loop);
            c.settle();
            let mut states = vec![];
            for (button, wait) in [(1u8, 1200u64), (1, 800), (2, 500), (4, 500)] {
                c.pedal.clear();
                c.dev.looper_button(button);
                c.pump(Duration::from_millis(wait));
                let st = c.pedal.iter().rev().find_map(|m| match m {
                    Msg::Cc { cc: 0x44, val, .. } => Some(val & 0x0F),
                    _ => None,
                });
                states.push(st);
            }
            c.check(
                &format!("looper rec -> play -> stop -> clear (states {states:?})"),
                states == [Some(1), Some(2), Some(4), Some(0)],
            );
        } else {
            println!("SKIP looper rec/play/stop/clear: the looper holds a loop (state {:#04x})", l[3]);
        }
    }

    // looper / drum / tuner reads
    for (name, r, cmd) in [("looper", req::looper(), 0x18), ("drum", req::drum(), 0x19), ("tuner", req::tuner(), 0x6F)]
    {
        let got = c.read(r, cmd).is_some();
        c.check(&format!("{name} read"), got);
    }

    let end = c.live();
    c.check("live preset back to how it started", end.raw == base.raw);
    println!("\n{} failed, {} errors: {:?}", c.failed, c.errors.len(), c.errors);
    std::process::exit(if c.failed == 0 { 0 } else { 1 });
}

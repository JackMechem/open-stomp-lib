//! Bluetooth LE MIDI link for the engine (feature `ble`): the pedal's Wireless mode.
//!
//! A background thread runs btleplug on a small tokio runtime. It scans for pedals advertising
//! the BLE-MIDI service, connects on request, and turns notifications back into MIDI bytes for
//! the engine ([`blemidi`](crate::blemidi) does the framing). The engine sees a Bluetooth pedal as
//! one more port, named `"<name> (Bluetooth)"`.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self as std_mpsc, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use btleplug::api::{Central, CentralEvent, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral, PeripheralId};
use futures_util::stream::{Stream, StreamExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use uuid::Uuid;

use crate::blemidi::{self, Decoder, MIN_PACKET};
use crate::{BLE_PORT_SUFFIX, is_stomp_port};

/// How long a pedal stays listed after its last advertisement.
const STALE: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Minimum time between messages to the pedal. Its BLE module feeds the DSP over a serial link,
/// and an unpaced burst (61 CCs at once) froze the pedal. The official app sends CCs at ≥ 11 ms
/// (typically 16 ms, one per frame) over USB.
const GAP: Duration = Duration::from_millis(15);

/// What the link reports to the engine.
pub(crate) enum Event {
    /// Pedals currently in range (port names).
    Ports(Vec<String>),
    /// MIDI bytes from the pedal (whole messages).
    Input(Vec<u8>),
    /// The connection dropped.
    Lost,
    /// Bluetooth can't be used (no adapter, no permission); reported once.
    Unavailable(String),
}

enum Cmd {
    Connect(String, std_mpsc::Sender<Result<(), String>>),
    Disconnect,
    Send(Vec<Vec<u8>>),
}

/// Handle to the link thread. Dropping it stops the thread and disconnects.
pub(crate) struct Ble {
    tx: UnboundedSender<Cmd>,
    thread: Option<JoinHandle<()>>,
}

impl Ble {
    pub fn spawn(events: impl Fn(Event) + Send + 'static) -> Ble {
        let (tx, rx) = unbounded_channel();
        let thread = thread::Builder::new()
            .name("open-stomp-ble".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(rt) => rt,
                    Err(e) => return events(Event::Unavailable(e.to_string())),
                };
                rt.block_on(Link::run(rx, &events));
            })
            .expect("spawn ble thread");
        Ble { tx, thread: Some(thread) }
    }

    /// Connect to a pedal by port name; blocks until connected or failed.
    pub fn connect(&self, port: &str) -> Result<(), String> {
        let (tx, rx) = std_mpsc::channel();
        self.tx.send(Cmd::Connect(port.to_owned(), tx)).map_err(|_| "Bluetooth stopped".to_owned())?;
        rx.recv_timeout(CONNECT_TIMEOUT + Duration::from_secs(5))
            .unwrap_or_else(|_| Err("Bluetooth connect timed out".into()))
    }

    pub fn disconnect(&self) {
        let _ = self.tx.send(Cmd::Disconnect);
    }

    /// Send whole MIDI messages, in order.
    pub fn send(&self, msgs: Vec<Vec<u8>>) -> bool {
        self.tx.send(Cmd::Send(msgs)).is_ok()
    }
}

impl Drop for Ble {
    fn drop(&mut self) {
        // closing the channel ends the link loop, which disconnects
        let (tx, _) = unbounded_channel();
        drop(std::mem::replace(&mut self.tx, tx));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Connected {
    peripheral: Peripheral,
    characteristic: Characteristic,
    max_packet: usize,
}

struct Link {
    adapter: Adapter,
    service: Uuid,
    characteristic: Uuid,
    /// Pedals seen advertising: id -> (port name, last seen).
    seen: HashMap<PeripheralId, (String, Instant)>,
    listed: Vec<String>,
    conn: Option<Connected>,
    decoder: Decoder,
    start: Instant,
    /// Messages waiting for their turn (see [`GAP`]).
    pending: VecDeque<Vec<u8>>,
    next_send: tokio::time::Instant,
}

type Notifications = std::pin::Pin<Box<dyn Stream<Item = btleplug::api::ValueNotification> + Send>>;

impl Link {
    async fn run(mut rx: UnboundedReceiver<Cmd>, events: &(impl Fn(Event) + Send)) {
        let service = Uuid::parse_str(blemidi::SERVICE).expect("uuid");
        let characteristic = Uuid::parse_str(blemidi::CHARACTERISTIC).expect("uuid");
        let adapter = async {
            let manager = Manager::new().await.map_err(|e| e.to_string())?;
            let adapters = manager.adapters().await.map_err(|e| e.to_string())?;
            adapters.into_iter().next().ok_or_else(|| "no Bluetooth adapter".to_owned())
        };
        let adapter = match adapter.await {
            Ok(a) => a,
            Err(e) => {
                events(Event::Unavailable(e));
                // keep answering commands so the engine never blocks on a dead link
                while let Some(cmd) = rx.recv().await {
                    if let Cmd::Connect(_, reply) = cmd {
                        let _ = reply.send(Err("Bluetooth is unavailable".into()));
                    }
                }
                return;
            }
        };
        let mut central = match adapter.events().await {
            Ok(s) => s,
            Err(e) => return events(Event::Unavailable(e.to_string())),
        };
        let mut link = Link {
            adapter,
            service,
            characteristic,
            seen: HashMap::new(),
            listed: vec![],
            conn: None,
            decoder: Decoder::default(),
            start: Instant::now(),
            pending: VecDeque::new(),
            next_send: tokio::time::Instant::now(),
        };
        link.scan(true).await;
        let mut notes: Option<Notifications> = None;
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                cmd = rx.recv() => match cmd {
                    None => break,
                    Some(Cmd::Connect(port, reply)) => {
                        let r = link.connect(&port).await;
                        notes = match r {
                            Ok(n) => {
                                let _ = reply.send(Ok(()));
                                Some(n)
                            }
                            Err(e) => {
                                let _ = reply.send(Err(e));
                                None
                            }
                        };
                    }
                    Some(Cmd::Disconnect) => {
                        notes = None;
                        link.disconnect().await;
                    }
                    Some(Cmd::Send(msgs)) => {
                        if link.conn.is_some() {
                            for m in msgs {
                                link.enqueue(m);
                            }
                        }
                    }
                },
                _ = tokio::time::sleep_until(link.next_send), if !link.pending.is_empty() => {
                    let m = link.pending.pop_front().expect("pending");
                    link.next_send = tokio::time::Instant::now() + GAP;
                    if !link.send(&[m]).await {
                        notes = None;
                        link.disconnect().await;
                        events(Event::Lost);
                    }
                }
                n = next_note(&mut notes) => match n {
                    Some(n) if n.uuid == link.characteristic => {
                        let bytes: Vec<u8> = link.decoder.push(&n.value).concat();
                        if !bytes.is_empty() {
                            events(Event::Input(bytes));
                        }
                    }
                    Some(_) => {}
                    None => {
                        notes = None;
                        link.disconnect().await;
                        events(Event::Lost);
                    }
                },
                e = central.next() => match e {
                    Some(CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id)) => link.saw(id).await,
                    Some(CentralEvent::DeviceDisconnected(id)) => {
                        if link.conn.as_ref().is_some_and(|c| c.peripheral.id() == id) {
                            notes = None;
                            link.disconnect().await;
                            events(Event::Lost);
                        }
                    }
                    Some(_) => {}
                    None => break,
                },
                _ = tick.tick() => {}
            }
            link.expire();
            let ports = link.ports();
            if ports != link.listed {
                link.listed = ports.clone();
                events(Event::Ports(ports));
            }
        }
        link.disconnect().await;
        let _ = link.adapter.stop_scan().await;
    }

    async fn scan(&self, on: bool) {
        let _ = if on {
            self.adapter.start_scan(ScanFilter { services: vec![self.service] }).await
        } else {
            self.adapter.stop_scan().await
        };
    }

    async fn saw(&mut self, id: PeripheralId) {
        let Ok(p) = self.adapter.peripheral(&id).await else {
            return;
        };
        let Ok(Some(props)) = p.properties().await else {
            return;
        };
        let name = props.local_name.unwrap_or_default();
        if props.services.contains(&self.service) && is_stomp_port(&name) {
            self.seen.insert(id, (format!("{name}{BLE_PORT_SUFFIX}"), Instant::now()));
        }
    }

    fn expire(&mut self) {
        let connected = self.conn.as_ref().map(|c| c.peripheral.id());
        self.seen.retain(|id, (_, at)| Some(id) == connected.as_ref() || at.elapsed() < STALE);
    }

    fn ports(&self) -> Vec<String> {
        let mut v: Vec<String> = self.seen.values().map(|(n, _)| n.clone()).collect();
        v.sort();
        v.dedup();
        v
    }

    async fn connect(&mut self, port: &str) -> Result<Notifications, String> {
        self.disconnect().await;
        let id = self.seen.iter().find(|(_, (n, _))| n == port).map(|(id, _)| id.clone());
        let id = id.ok_or_else(|| format!("“{port}” is not in range"))?;
        let p = self.adapter.peripheral(&id).await.map_err(|e| e.to_string())?;
        self.scan(false).await;
        let r = async {
            tokio::time::timeout(CONNECT_TIMEOUT, async {
                if !p.is_connected().await.unwrap_or(false) {
                    p.connect().await?;
                }
                p.discover_services().await
            })
            .await
            .map_err(|_| "timed out".to_owned())?
            .map_err(|e| e.to_string())?;
            let c = p
                .characteristics()
                .into_iter()
                .find(|c| c.uuid == self.characteristic)
                .ok_or_else(|| "the pedal has no BLE-MIDI characteristic".to_owned())?;
            p.subscribe(&c).await.map_err(|e| e.to_string())?;
            let notes = p.notifications().await.map_err(|e| e.to_string())?;
            // ATT MTU minus the 3-byte write header
            let max_packet = (p.mtu() as usize).saturating_sub(3).max(MIN_PACKET);
            Ok((c, notes, max_packet))
        }
        .await;
        match r {
            Ok((characteristic, notes, max_packet)) => {
                self.conn = Some(Connected { peripheral: p, characteristic, max_packet });
                self.decoder = Decoder::default();
                Ok(notes)
            }
            Err(e) => {
                let _ = p.disconnect().await;
                self.scan(true).await;
                Err(e)
            }
        }
    }

    /// Queue a message. A CC that only replaces the value of the CC queued just before it
    /// (a slider drag) overwrites it, so a fast drag can't build a backlog.
    fn enqueue(&mut self, m: Vec<u8>) {
        let is_cc = |x: &[u8]| x.len() == 3 && x[0] & 0xF0 == 0xB0;
        if is_cc(&m)
            && let Some(last) = self.pending.back_mut()
            && is_cc(last)
            && last[..2] == m[..2]
        {
            *last = m;
            return;
        }
        self.pending.push_back(m);
    }

    async fn disconnect(&mut self) {
        self.pending.clear();
        if let Some(c) = self.conn.take() {
            let _ = c.peripheral.disconnect().await;
            self.scan(true).await;
        }
    }

    /// Write messages as BLE-MIDI packets; false if the link failed.
    async fn send(&mut self, msgs: &[Vec<u8>]) -> bool {
        let Some(c) = &self.conn else { return false };
        let ts = (self.start.elapsed().as_millis() & 0x1FFF) as u16;
        for packet in blemidi::encode(msgs, ts, c.max_packet) {
            if c.peripheral.write(&c.characteristic, &packet, WriteType::WithoutResponse).await.is_err() {
                return false;
            }
        }
        true
    }
}

async fn next_note(notes: &mut Option<Notifications>) -> Option<btleplug::api::ValueNotification> {
    match notes {
        Some(n) => n.next().await,
        None => std::future::pending().await,
    }
}

/// Engine-side sender for link events.
pub(crate) fn forward<T: Send + 'static>(tx: Sender<T>, wrap: fn(Event) -> T) -> impl Fn(Event) + Send + 'static {
    move |e| {
        let _ = tx.send(wrap(e));
    }
}

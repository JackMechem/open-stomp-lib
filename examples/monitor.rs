//! Connect to the pedal (or replay a capture) and print everything that happens on it.
//!
//! ```sh
//! cargo run --example monitor                       # auto-connect, sync, then follow the pedal
//! cargo run --example monitor -- --listen           # send nothing at all
//! cargo run --example monitor -- --port "JME STOMPBOX MIDI 1"
//! cargo run --example monitor -- --replay captures/logs/01-main-session.log --speed 8
//! cargo run --example monitor -- --all              # include protocol plumbing and repeats
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use open_stomp::device::{Config, Device, Status, Update};
use open_stomp::state::preset_label;
use open_stomp::{Category, DeviceState, Origin};

fn clock(at: SystemTime) -> String {
    let ms = at.duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}.{:03} UTC", s / 3600 % 24, s / 60 % 60, s % 60, ms % 1000)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let all = args.iter().any(|a| a == "--all");
    let listen = args.iter().any(|a| a == "--listen");
    let port = value("--port");
    let replay = value("--replay");
    let speed = value("--speed").and_then(|s| s.parse().ok()).unwrap_or(4.0);

    let config = Config { sync: !listen, auto_connect: port.is_none() && replay.is_none(), ..Config::default() };
    let dev = Device::spawn(config);
    if let Some(p) = port {
        dev.connect(p);
    }
    if let Some(r) = &replay {
        dev.replay(r, speed);
    }

    let mut st = DeviceState::default();
    let mut synced = false;
    while let Ok(u) = dev.recv() {
        match u {
            Update::Ports(ports) => {
                if !ports.iter().any(|p| open_stomp::is_stomp_port(p)) {
                    eprintln!("waiting for the pedal… (MIDI inputs: {ports:?})");
                }
            }
            Update::Status(Status::Connected { port, output }) => {
                eprintln!("connected to {port}{}", if output { "" } else { " (input only)" });
            }
            Update::Status(_) => {}
            Update::Reset => {
                st = DeviceState::default();
                synced = false;
            }
            Update::Sync { done, total } => {
                if total > 0 && done % 20 == 0 {
                    eprintln!("syncing {done}/{total}");
                } else if total == 0 && !synced && st.firmware.is_some() {
                    synced = true;
                    let name = st.current.and_then(|i| st.preset_name(i)).unwrap_or("?");
                    let cur = st.current.map(preset_label).unwrap_or_default();
                    eprintln!(
                        "synced: {} firmware {} · active {cur} {name}",
                        st.product.as_deref().unwrap_or("pedal"),
                        st.firmware.as_deref().unwrap_or("?"),
                    );
                }
            }
            Update::Mark { at, text } => println!("{} ===== {text} =====", clock(at)),
            Update::Info { at, text, error } => {
                println!("{} {}{text}", clock(at), if error { "! " } else { "" });
                if text == "Replay finished" {
                    break;
                }
            }
            Update::Message { at, origin, bytes } => {
                let ev = st.apply(&bytes, origin);
                if (ev.category == Category::Protocol || ev.redundant) && !all {
                    continue;
                }
                let who = if origin == Origin::Pedal { "PEDAL" } else { "HOST " };
                println!("{} {who} {:8} {:40} {}", clock(at), ev.category.label(), ev.subject, ev.value);
            }
        }
    }
}

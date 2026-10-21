//! The engine thread end to end, using capture replay instead of a pedal.
#![cfg(feature = "device")]

use std::time::{Duration, Instant};

use open_stomp::device::{Config, Device, Status, Update};
use open_stomp::{DeviceState, Origin};

#[test]
fn replay_main_session() {
    let dev = Device::spawn(Config { auto_connect: false, ..Config::default() });
    dev.replay(concat!(env!("CARGO_MANIFEST_DIR"), "/captures/logs/01-main-session.log"), 1000.0);
    let mut st = DeviceState::default();
    let (mut reset, mut replaying, mut messages) = (false, false, 0);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match dev.recv_timeout(left).expect("replay finishes") {
            Update::Reset => reset = true,
            Update::Status(Status::Replaying { .. }) => replaying = true,
            Update::Message { origin, bytes, .. } => {
                st.apply(&bytes, origin);
                messages += 1;
                assert!(matches!(origin, Origin::Host | Origin::Pedal));
            }
            Update::Info { text, error: false, .. } if text == "Replay finished" => break,
            _ => {}
        }
    }
    assert!(reset && replaying);
    assert!(messages > 0);
    assert_eq!(st.firmware.as_deref(), Some("V1.1.1"));
    drop(dev); // joins the engine thread
}

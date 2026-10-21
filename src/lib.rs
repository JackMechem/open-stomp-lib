//! A library for the SORAN Dual Stomp (Lite) guitar multi-effects pedal over USB-MIDI.
//!
//! The protocol was reverse engineered from the vendor's *Stomp Manager* app; `docs/` in the
//! repository describes it in full, and `docs/10-library-guide.md` is the guide to this crate.
//! The crate has two layers:
//!
//! * **Protocol** (always available, no I/O, no dependencies): [`pack`] (8-in-7 bitstream),
//!   [`midi`] (message parsing, SysEx framing, stream reassembly, request and command builders),
//!   [`catalog`] (modules, models, parameters), [`records`] (preset and settings records),
//!   [`state`] (a model of the pedal that every message is applied to), [`queue`] (the
//!   one-request-at-a-time pacing the pedal needs) and [`capture`] (capture log reader).
//! * **Device** (feature `device`, on by default): [`device`] runs a background engine that finds
//!   the pedal, connects, performs the startup sync, sends edits and reports everything that
//!   happens, or replays a capture log instead of a real pedal.
//!
//! ```no_run
//! # #[cfg(feature = "device")] {
//! use open_stomp::device::{Config, Device, Update};
//! use open_stomp::DeviceState;
//!
//! let dev = Device::spawn(Config::default()); // finds and connects to the pedal by itself
//! let mut state = DeviceState::default();
//! while let Ok(update) = dev.recv() {
//!     match update {
//!         Update::Reset => state = DeviceState::default(),
//!         Update::Message { origin, bytes, .. } => {
//!             let ev = state.apply(&bytes, origin);
//!             println!("{:8} {} = {}", ev.category.label(), ev.subject, ev.value);
//!         }
//!         _ => {}
//!     }
//! }
//! // and to control it: dev.select_preset(4); dev.set_param(Module::Amp, 0, 100); ...
//! # }
//! ```
//!
//! Tested against the Dual Stomp Lite on firmware V1.1.1. The capture logs in `captures/logs/`
//! are the test vectors.

pub mod capture;
pub mod catalog;
#[cfg(feature = "device")]
pub mod device;
pub mod midi;
pub mod pack;
pub mod queue;
pub mod records;
pub mod state;

pub use catalog::Module;
pub use midi::{Assembler, Msg};
pub use records::{Preset, Settings};
pub use state::{Category, DeviceState, Event, Origin};

/// USB vendor and product id of the pedal (`3962:2000`, "JME STOMPBOX").
pub const USB_ID: (u16, u16) = (0x3962, 0x2000);

/// Whether a MIDI port name belongs to the pedal. Matches what the official app matches
/// (`.*JME.*`, `.*SORAN AMPEY.*`) plus the product name, ignoring case.
pub fn is_stomp_port(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n.contains("JME") || n.contains("SORAN") || n.contains("STOMPBOX")
}

#[cfg(test)]
mod tests {
    #[test]
    fn port_names() {
        for n in ["JME STOMPBOX MIDI 1", "JME STOMPBOX:JME STOMPBOX MIDI 1 24:0", "jme stompbox"] {
            assert!(super::is_stomp_port(n), "{n}");
        }
        assert!(!super::is_stomp_port("IAC Driver Bus 1"));
    }
}

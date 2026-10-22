//! Reader for the capture logs in `captures/logs/` (docs/07):
//! `<unix ms> <TX|RX> <len> [<endpoint>] <hex…>` and `# MARK <ms> <text>`.

use crate::midi::Assembler;
use crate::state::Origin;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Mark { ms: u64, text: String },
    Message { ms: u64, origin: Origin, bytes: Vec<u8> },
}

/// Parse a whole capture log into complete messages, reassembling split SysEx.
pub fn parse(text: &str) -> Vec<Entry> {
    let (mut tx, mut rx) = (Assembler::default(), Assembler::default());
    let mut out = vec![];
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# MARK ") {
            let (ms, text) = rest.split_once(' ').unwrap_or((rest, ""));
            out.push(Entry::Mark { ms: ms.parse().unwrap_or(0), text: text.to_owned() });
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let (Some(ms), Some(dir), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let (origin, asm) = match dir {
            "TX" => (Origin::Host, &mut tx),
            "RX" | "MON" => (Origin::Pedal, &mut rx),
            _ => continue,
        };
        let Some(hex) = rest.split_once(']').map(|(_, h)| h) else { continue };
        let data: Vec<u8> = hex.split_whitespace().filter_map(|h| u8::from_str_radix(h, 16).ok()).collect();
        let ms = ms.parse().unwrap_or(0);
        for bytes in asm.push(&data) {
            out.push(Entry::Message { ms, origin, bytes });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Module;
    use crate::state::{Category, DeviceState};

    fn main_session() -> Vec<Entry> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/captures/logs/01-main-session.log");
        parse(&std::fs::read_to_string(path).expect("capture log"))
    }

    #[test]
    fn replay_main_session() {
        let mut st = DeviceState::default();
        let mut unknown = 0;
        let mut handshake_done = false;
        for e in main_session() {
            let Entry::Message { origin, bytes, .. } = e else { continue };
            let ev = st.apply(&bytes, origin);
            if ev.category == Category::Unknown {
                unknown += 1;
            }
            if !handshake_done && st.presets.iter().all(|p| p.is_some()) {
                handshake_done = true;
                assert_eq!(st.firmware.as_deref(), Some("V1.1.1"));
                assert_eq!(st.model_id.as_deref(), Some("JME-01"));
                assert_eq!(st.product.as_deref(), Some("JME STOMPBOX"));
                for (i, p) in st.presets.iter().enumerate() {
                    let p = p.as_ref().unwrap();
                    assert_eq!(p.index as usize, i);
                    assert!(!p.name.is_empty(), "preset {i} has a name");
                    assert!(p.name.chars().all(|c| c.is_ascii_graphic() || c == ' '), "{:?}", p.name);
                    assert!(p.modules[Module::Amp.index()].model < 40);
                }
                // the edit buffer after the handshake is P01
                assert!(st.live_known);
            }
        }
        assert!(handshake_done);
        assert!(unknown < 10, "{unknown} unknown messages");
        let s = st.settings.expect("settings");
        assert!(s.input_trim <= 20);
    }

    /// Every preset / settings record on the wire re-encodes to the same bytes, and the
    /// param-count bytes agree with the catalogue.
    #[test]
    fn records_round_trip() {
        use crate::midi::{Msg, host};
        use crate::records::{Preset, Settings, param_count};
        let (mut presets, mut settings) = (0, 0);
        for e in main_session() {
            let Entry::Message { bytes, .. } = e else { continue };
            let Msg::Nep { cmd, dir, payload } = Msg::parse(&bytes) else { continue };
            if dir != 1 && dir != 2 {
                continue;
            }
            match cmd {
                0x0B | 0x0C => {
                    let p = Preset::from_packed(&payload).unwrap();
                    assert_eq!(p.to_packed(), payload, "preset {}", p.name);
                    for m in Module::EFFECTS {
                        let count = p.raw[[25, 32, 41, 61, 66, 73, 20, 48][m.index()]];
                        let model = p.modules[m.index()].model;
                        assert_eq!(count, param_count(m, model), "{} {} model {model}", p.name, m.name());
                    }
                    if dir == 1 {
                        assert_eq!(host::preset_write(&p), bytes);
                    }
                    presets += 1;
                }
                0x14 => {
                    let s = Settings::from_packed(&payload).unwrap();
                    assert_eq!(s.to_packed(), payload);
                    if dir == 1 {
                        assert_eq!(host::settings_write(&s), bytes);
                    }
                    settings += 1;
                }
                _ => {}
            }
        }
        assert!(presets >= 100 && settings > 100, "{presets} presets, {settings} settings");
    }

    /// Rebuild the captured "save as" (P69 renamed "4Ooz!" and stored to P22) from state alone.
    #[test]
    fn save_as_matches_capture() {
        use crate::midi::host;
        let entries = main_session();
        let save_at = entries
            .iter()
            .position(|e| matches!(e, Entry::Message { origin: Origin::Host, bytes, .. } if bytes.get(5) == Some(&0x12)))
            .unwrap();
        let mut st = DeviceState::default();
        for e in &entries[..save_at] {
            if let Entry::Message { origin, bytes, .. } = e {
                st.apply(bytes, *origin);
            }
        }
        assert_eq!(st.current, Some(0x44));
        let mut p = st.live.clone();
        p.index = 0x15;
        p.name = "4Ooz!".into();
        let ours = host::save_preset(st.current.unwrap(), &p);
        let theirs: Vec<Vec<u8>> = entries[save_at..]
            .iter()
            .filter_map(|e| match e {
                Entry::Message { origin: Origin::Host, bytes, .. } => Some(bytes.clone()),
                _ => None,
            })
            .take(3)
            .collect();
        assert_eq!(ours, theirs);
    }

    /// Every captured NAM and IR import is rebuilt byte for byte from its decoded parts (slot,
    /// name, factor, floats), and the imported names reach the state.
    #[test]
    fn uploads_match_capture() {
        use crate::midi::{Cmd, Msg};
        use crate::upload::{self, Chunk, Reassembler, UserAmpInfo, amp_sub};

        let host: Vec<Vec<u8>> = main_session()
            .into_iter()
            .filter_map(|e| match e {
                Entry::Message { origin: Origin::Host, bytes, .. } => Some(bytes),
                _ => None,
            })
            .collect();
        let nep = |b: &[u8]| match Msg::parse(b) {
            Msg::Nep { cmd, dir: 1, payload } => Some((cmd, payload)),
            _ => None,
        };
        // the data chunks from `from` on, until one upload is complete: (index of its last chunk, slot, floats)
        let collect = |from: usize| -> (usize, u8, Vec<f32>) {
            let mut r = Reassembler::default();
            for (k, b) in host.iter().enumerate().skip(from) {
                let (_, p) = nep(b).expect("data chunk");
                if let Some((slot, floats)) = r.push(&Chunk::parse(&p).unwrap()) {
                    return (k, slot, floats);
                }
            }
            panic!("incomplete upload");
        };
        let (mut amps, mut irs) = (0, 0);
        let mut i = 0;
        while i < host.len() {
            let Some((cmd, payload)) = nep(&host[i]) else {
                i += 1;
                continue;
            };
            let chunk = Chunk::parse(&payload);
            if cmd == Cmd::UserAmp as u8 && let Some(info) = UserAmpInfo::parse(&payload) {
                // NAM import: slot-info record, then the data
                let (end, slot, floats) = collect(i + 1);
                assert_eq!((slot, floats.len(), info.factor), (info.slot, upload::UPLOAD_FLOATS, 1.0));
                let ours = upload::user_amp_upload(&UserAmpInfo::new(info.slot, &info.name, info.factor), &floats);
                assert_eq!(ours, host[i..=end], "NAM import {:?}", info.name);
                amps += 1;
                i = end;
            } else if cmd == Cmd::IrData as u8 && chunk.as_ref().is_some_and(|c| c.offset == 0) {
                // IR import: the data, then the 0x09 name write
                let (end, slot, floats) = collect(i);
                let name = crate::records::parse_ir_slot(&nep(&host[end + 1]).unwrap().1).unwrap().1;
                assert_eq!(upload::user_ir_upload(slot, &name, &floats), host[i..=end + 1], "IR import {name:?}");
                irs += 1;
                i = end + 1;
            } else if cmd == Cmd::UserAmp as u8 && let Some(c) = chunk.filter(|c| c.sub == amp_sub::DELETE) {
                assert_eq!(upload::user_amp_delete(c.slot), host[i]);
            }
            i += 1;
        }
        assert_eq!((amps, irs), (3, 1), "FNDR twice + Jcm800, one IR");

        let mut st = DeviceState::default();
        for e in main_session() {
            if let Entry::Message { origin, bytes, .. } = e {
                st.apply(&bytes, origin);
            }
        }
        assert_eq!(st.user_amps[0].as_deref(), Some("FNDR BFSR VB Edge WRM2 CAB FREE"));
        assert_eq!(st.user_amps[2].as_deref(), Some("Jcm800"));
        assert_eq!(st.user_irs[0].as_deref(), Some("Delay And Reverb"));
    }

    #[test]
    fn preset_switch_applies_cached_preset() {
        let mut st = DeviceState::default();
        for e in main_session() {
            if let Entry::Message { origin, bytes, .. } = e {
                st.apply(&bytes, origin);
                if bytes[0] & 0xF0 == 0xC0 && st.presets.iter().all(|p| p.is_some()) {
                    let i = bytes[1];
                    assert_eq!(st.current, Some(i));
                    assert_eq!(st.live.name, st.presets[i as usize].as_ref().unwrap().name);
                }
            }
        }
    }
}

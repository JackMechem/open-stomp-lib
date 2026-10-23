//! BLE-MIDI packet framing (Apple / MMA "MIDI over Bluetooth LE" spec), used for the pedal's
//! Wireless mode. Pure code: the transport itself is in the engine (feature `ble`).
//!
//! A packet is a header byte (`1 0 t12…t7`) followed by MIDI messages, each status byte preceded
//! by a timestamp byte (`1 t6…t0`). SysEx may span packets: a continuation packet carries its
//! data bytes straight after the header, and the closing `F7` gets its own timestamp byte.
//! Running status is allowed, with or without a fresh timestamp.
//!
//! The pedal speaks exactly the USB-MIDI protocol over BLE (SysEx, CCs, PCs), only framed like
//! this.

/// The pedal's GATT service and characteristic (the standard BLE-MIDI ones).
pub const SERVICE: &str = "03b80e5a-ede8-4b33-a751-6ce34ec4c700";
pub const CHARACTERISTIC: &str = "7772e5db-3868-4112-a1a9-f2669d106bf3";

/// Packet payload size that every BLE link supports (ATT MTU 23 minus the 3-byte header).
pub const MIN_PACKET: usize = 20;

fn header(ts: u16) -> u8 {
    0x80 | ((ts >> 7) & 0x3F) as u8
}

fn stamp(ts: u16) -> u8 {
    0x80 | (ts & 0x7F) as u8
}

/// Frame whole MIDI messages into BLE-MIDI packets of at most `max` bytes (`max` ≥ 5), packing
/// several messages per packet. `ts_ms` is the 13-bit millisecond timestamp (wraps).
pub fn encode(msgs: &[Vec<u8>], ts_ms: u16, max: usize) -> Vec<Vec<u8>> {
    let max = max.max(5);
    let (h, t) = (header(ts_ms), stamp(ts_ms));
    let mut out = vec![];
    let mut cur = vec![h];
    let flush = |cur: &mut Vec<u8>, out: &mut Vec<Vec<u8>>| {
        if cur.len() > 1 {
            out.push(std::mem::replace(cur, vec![h]));
        }
    };
    for m in msgs {
        if m.is_empty() {
            continue;
        }
        if m[0] == 0xF0 {
            // timestamp + F0 + data, split anywhere in the data; timestamp + F7 stay together
            if cur.len() + 3 > max {
                flush(&mut cur, &mut out);
            }
            cur.extend([t, 0xF0]);
            let end = m.last() == Some(&0xF7);
            let data = &m[1..m.len() - end as usize];
            for &b in data {
                if cur.len() + 1 > max {
                    out.push(std::mem::replace(&mut cur, vec![h]));
                }
                cur.push(b);
            }
            if end {
                if cur.len() + 2 > max {
                    out.push(std::mem::replace(&mut cur, vec![h]));
                }
                cur.extend([t, 0xF7]);
            }
        } else {
            if cur.len() + 1 + m.len() > max {
                flush(&mut cur, &mut out);
            }
            cur.push(t);
            cur.extend_from_slice(m);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Number of data bytes after a (non-SysEx) status byte.
fn data_len(status: u8) -> usize {
    match status {
        0xF1 | 0xF3 => 1,
        0xF2 => 2,
        0xF0..=0xFF => 0,
        s if s & 0xF0 == 0xC0 || s & 0xF0 == 0xD0 => 1,
        _ => 2,
    }
}

/// Turns BLE-MIDI packets back into whole MIDI messages, keeping SysEx and running status across
/// packets.
#[derive(Default)]
pub struct Decoder {
    sysex: Option<Vec<u8>>,
    running: Option<u8>,
    msg: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, packet: &[u8]) -> Vec<Vec<u8>> {
        let mut out = vec![];
        if packet.first().is_none_or(|h| h & 0x80 == 0) {
            return out;
        }
        let mut i = 1;
        while i < packet.len() {
            let b = packet[i];
            if b & 0x80 == 0 {
                self.data(b, &mut out);
                i += 1;
                continue;
            }
            // timestamp byte; the next byte is a status or (running status) data
            i += 1;
            let Some(&s) = packet.get(i) else { break };
            if s & 0x80 == 0 {
                continue;
            }
            i += 1;
            match s {
                0xF7 => {
                    if let Some(mut x) = self.sysex.take() {
                        x.push(0xF7);
                        out.push(x);
                    }
                }
                0xF0 => {
                    self.sysex = Some(vec![0xF0]);
                    self.running = None;
                }
                0xF8..=0xFF => out.push(vec![s]),
                _ => {
                    self.sysex = None;
                    self.running = (s < 0xF0).then_some(s);
                    self.msg = vec![s];
                    if data_len(s) == 0 {
                        out.push(std::mem::take(&mut self.msg));
                    }
                }
            }
        }
        out
    }

    fn data(&mut self, b: u8, out: &mut Vec<Vec<u8>>) {
        if let Some(x) = &mut self.sysex {
            x.push(b);
            return;
        }
        if self.msg.is_empty() {
            match self.running {
                Some(r) => self.msg.push(r),
                None => return,
            }
        }
        self.msg.push(b);
        if self.msg.len() == 1 + data_len(self.msg[0]) {
            out.push(std::mem::take(&mut self.msg));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(packets: &[Vec<u8>]) -> Vec<Vec<u8>> {
        let mut d = Decoder::default();
        packets.iter().flat_map(|p| d.push(p)).collect()
    }

    #[test]
    fn pedal_replies() {
        // captured from the pedal over BLE (version reply, a CC)
        let p = [
            0x99, 0xE4, 0xF0, 0x00, 0x21, 0x7A, 0x10, 0x56, 0x31, 0x2E, 0x31, 0x2E, 0x31, 0x00, 0x4A, 0x4D, 0x45, 0xE4,
            0xF7,
        ];
        assert_eq!(
            decode_all(&[p.to_vec(), vec![0xA0, 0x81, 0xB0, 0x3B, 0x02]]),
            vec![
                vec![0xF0, 0x00, 0x21, 0x7A, 0x10, 0x56, 0x31, 0x2E, 0x31, 0x2E, 0x31, 0x00, 0x4A, 0x4D, 0x45, 0xF7],
                vec![0xB0, 0x3B, 0x02],
            ]
        );
    }

    #[test]
    fn host_request_matches_probe() {
        // what the CoreBluetooth probe sent and the pedal answered
        let ts = (0x19 << 7) | 0x50;
        assert_eq!(
            encode(&[vec![0xF0, 0x00, 0x21, 0x7A, 0x00, 0xF7]], ts, 182),
            vec![vec![0x99, 0xD0, 0xF0, 0x00, 0x21, 0x7A, 0x00, 0xD0, 0xF7]]
        );
    }

    #[test]
    fn running_status() {
        // timestamp + data (new time, same status), and bare data (same time)
        let p = vec![0x80, 0x81, 0xB0, 0x12, 0x40, 0x82, 0x13, 0x41, 0x14, 0x42];
        assert_eq!(decode_all(&[p]), vec![vec![0xB0, 0x12, 0x40], vec![0xB0, 0x13, 0x41], vec![0xB0, 0x14, 0x42]]);
    }

    #[test]
    fn realtime_inside_sysex() {
        let p = vec![0x80, 0x81, 0xF0, 0x01, 0x02, 0x82, 0xF8, 0x03, 0x83, 0xF7];
        assert_eq!(decode_all(&[p]), vec![vec![0xF8], vec![0xF0, 0x01, 0x02, 0x03, 0xF7]]);
    }

    #[test]
    fn round_trip_split() {
        let big: Vec<u8> = [0xF0].into_iter().chain((0..300).map(|i| (i % 128) as u8)).chain([0xF7]).collect();
        let msgs = vec![vec![0xB1, 0x05, 0x03], big, vec![0xC1, 0x04], vec![0xF0, 0x7E, 0xF7]];
        for max in [5, 6, 7, 20, 179] {
            let packets = encode(&msgs, 0x1ABC, max);
            assert!(packets.iter().all(|p| p.len() <= max && p.len() > 1), "max {max}");
            assert_eq!(decode_all(&packets), msgs, "max {max}");
        }
        assert_eq!(encode(&msgs[..1], 0, 20).len(), 1);
        // small messages share a packet
        assert_eq!(
            encode(&[vec![0xB1, 1, 2], vec![0xB1, 3, 4]], 0, 20),
            vec![vec![0x80, 0x80, 0xB1, 1, 2, 0x80, 0xB1, 3, 4]]
        );
    }
}

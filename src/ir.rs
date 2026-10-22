//! Impulse responses for the pedal's user CAB (IR) slots (docs/05): reading audio files.
//!
//! The upload itself is [`crate::upload::user_ir_upload`] / `Device::import_user_ir`; how the official
//! app turns an IR file into the 2048 uploaded floats is still being worked out (docs/09).

use std::fmt;

/// A decoded WAV file: one sample vector per channel, scaled to ±1.0.
#[derive(Clone, Debug, PartialEq)]
pub struct Wav {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WavError {
    /// Not a RIFF/WAVE file, or a chunk is cut short.
    Format(String),
    /// A valid WAV in an encoding this reader doesn't handle (e.g. compressed).
    Unsupported(String),
}

impl fmt::Display for WavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WavError::Format(s) => write!(f, "invalid WAV file: {s}"),
            WavError::Unsupported(s) => write!(f, "unsupported WAV encoding: {s}"),
        }
    }
}

impl std::error::Error for WavError {}

const PCM: u16 = 1;
const FLOAT: u16 = 3;
const EXTENSIBLE: u16 = 0xFFFE;

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

impl Wav {
    /// Decode a WAV file's bytes: PCM 8/16/24/32-bit or IEEE float 32/64-bit, any channel count,
    /// plain or `WAVE_FORMAT_EXTENSIBLE`.
    pub fn parse(bytes: &[u8]) -> Result<Wav, WavError> {
        let bad = |s: &str| WavError::Format(s.to_owned());
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err(bad("no RIFF/WAVE header"));
        }
        let (mut fmt, mut data) = (None, None);
        let mut i = 12;
        while i + 8 <= bytes.len() {
            let (id, len) = (&bytes[i..i + 4], u32_at(bytes, i + 4) as usize);
            let body = &bytes[i + 8..(i + 8 + len).min(bytes.len())];
            match id {
                b"fmt " => fmt = Some(body),
                // a truncated data chunk (e.g. a writer that never patched the length) is read as is
                b"data" => data = Some(body),
                _ => {}
            }
            i += 8 + len + (len & 1);
        }
        let fmt = fmt.ok_or_else(|| bad("no fmt chunk"))?;
        let data = data.ok_or_else(|| bad("no data chunk"))?;
        if fmt.len() < 16 {
            return Err(bad("short fmt chunk"));
        }
        let mut tag = u16_at(fmt, 0);
        let channels = u16_at(fmt, 2) as usize;
        let sample_rate = u32_at(fmt, 4);
        let bits = u16_at(fmt, 14) as usize;
        if tag == EXTENSIBLE {
            if fmt.len() < 26 {
                return Err(bad("short extensible fmt chunk"));
            }
            tag = u16_at(fmt, 24); // first two bytes of the sub-format GUID
        }
        if channels == 0 || sample_rate == 0 {
            return Err(bad("no channels or no sample rate"));
        }
        let width = bits.div_ceil(8);
        let decode: fn(&[u8]) -> f32 = match (tag, width) {
            (PCM, 1) => |s| (s[0] as f32 - 128.0) / 128.0,
            (PCM, 2) => |s| i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
            (PCM, 3) => |s| (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0,
            (PCM, 4) => |s| (i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64 / 2_147_483_648.0) as f32,
            (FLOAT, 4) => |s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
            (FLOAT, 8) => |s| f64::from_le_bytes(s.try_into().expect("8 bytes")) as f32,
            _ => return Err(WavError::Unsupported(format!("format {tag:#06x}, {bits} bits"))),
        };
        let frame = width * channels;
        let mut out = vec![Vec::with_capacity(data.len() / frame); channels];
        for f in data.chunks_exact(frame) {
            for (c, s) in f.chunks_exact(width).enumerate() {
                out[c].push(decode(s));
            }
        }
        Ok(Wav { sample_rate, channels: out })
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Wav, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Wav::parse(&std::fs::read(path)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal WAV file around `data`.
    fn wav(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut f = vec![];
        f.extend_from_slice(&tag.to_le_bytes());
        f.extend_from_slice(&channels.to_le_bytes());
        f.extend_from_slice(&rate.to_le_bytes());
        f.extend_from_slice(&(rate * channels as u32 * bits as u32 / 8).to_le_bytes());
        f.extend_from_slice(&(channels * bits / 8).to_le_bytes());
        f.extend_from_slice(&bits.to_le_bytes());
        let mut b = b"RIFF\0\0\0\0WAVE".to_vec();
        for (id, body) in [(b"fmt ", &f[..]), (b"LIST", &b"junk"[..]), (b"data", data)] {
            b.extend_from_slice(id);
            b.extend_from_slice(&(body.len() as u32).to_le_bytes());
            b.extend_from_slice(body);
        }
        b
    }

    #[test]
    fn pcm_and_float() {
        let w = Wav::parse(&wav(PCM, 1, 48000, 24, &[0, 0, 0x40, 0, 0, 0xC0, 0xFF, 0xFF, 0x7F])).unwrap();
        assert_eq!(w.sample_rate, 48000);
        assert_eq!(w.channels, vec![vec![0.5, -0.5, 8_388_607.0 / 8_388_608.0]]);
        let w = Wav::parse(&wav(PCM, 2, 44100, 16, &[0, 0x40, 0, 0xC0])).unwrap();
        assert_eq!(w.channels, vec![vec![0.5], vec![-0.5]]);
        let d: Vec<u8> = [0.25f32, -1.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(Wav::parse(&wav(FLOAT, 1, 96000, 32, &d)).unwrap().channels, vec![vec![0.25, -1.0]]);
    }

    #[test]
    fn errors() {
        assert!(matches!(Wav::parse(b"nope"), Err(WavError::Format(_))));
        assert!(matches!(Wav::parse(&wav(2, 1, 48000, 4, &[0; 8])), Err(WavError::Unsupported(_))));
    }
}

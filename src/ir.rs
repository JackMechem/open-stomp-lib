//! Impulse responses for the pedal's user CAB (IR) slots (docs/05): reading and writing WAV files,
//! resampling, and the conversion to the pedal's upload format.
//!
//! The official app uploads the first [`IR_TAPS`] samples of the IR at 48 kHz, time-reversed, times a
//! gain. [`convert`] does the same; the app's gain rule is not known yet, so the gain here
//! (unit energy over the uploaded samples) is provisional and may differ by a dB or so (docs/09).
//! The upload itself is [`crate::upload::user_ir_upload`] / `Device::import_user_ir`.

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

impl Wav {
    /// Encode as a WAV file: 16/24/32-bit PCM (`float = false`) or 32-bit float (`bits = 32`, `float = true`).
    pub fn to_bytes(&self, bits: u16, float: bool) -> Vec<u8> {
        assert!(matches!((bits, float), (16 | 24 | 32, false) | (32, true)), "unsupported WAV encoding");
        let ch = self.channels.len() as u16;
        let frames = self.channels.first().map_or(0, Vec::len);
        let width = bits as usize / 8;
        let mut data = Vec::with_capacity(frames * ch as usize * width);
        for i in 0..frames {
            for c in &self.channels {
                let v = c.get(i).copied().unwrap_or(0.0);
                if float {
                    data.extend_from_slice(&v.to_le_bytes());
                } else {
                    let full = (1i64 << (bits - 1)) as f64;
                    let q = (v as f64 * full).round().clamp(-full, full - 1.0) as i64;
                    data.extend_from_slice(&q.to_le_bytes()[..width]);
                }
            }
        }
        let mut b = Vec::with_capacity(44 + data.len());
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&(if float { FLOAT } else { PCM }).to_le_bytes());
        b.extend_from_slice(&ch.to_le_bytes());
        b.extend_from_slice(&self.sample_rate.to_le_bytes());
        b.extend_from_slice(&(self.sample_rate * ch as u32 * width as u32).to_le_bytes());
        b.extend_from_slice(&(ch * width as u16).to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(&data);
        b
    }
}

// ---------------------------------------------------------------------------------------------
// resampling

/// Modified Bessel function of the first kind, order 0 (for the Kaiser window).
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
    while term > 1e-12 * sum {
        term *= (x / (2.0 * k)).powi(2);
        sum += term;
        k += 1.0;
    }
    sum
}

/// Resample `x` from `from` Hz to `to` Hz: windowed-sinc interpolation (Kaiser, β = 9, 32 zero
/// crossings each side), low-pass at 95 % of the lower Nyquist frequency. The output covers the same
/// duration (`ceil(len · to / from)` samples), with no delay.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    const ZEROS: f64 = 32.0;
    const BETA: f64 = 9.0;
    let ratio = to as f64 / from as f64;
    // cutoff in cycles per input sample
    let fc = 0.5 * ratio.min(1.0) * 0.95;
    let half = ZEROS / (2.0 * fc); // window half-width in input samples
    let i0b = bessel_i0(BETA);
    let len = (x.len() as u64 * to as u64).div_ceil(from as u64) as usize;
    (0..len)
        .map(|n| {
            let t = n as f64 / ratio;
            let (lo, hi) = (((t - half).ceil().max(0.0)) as usize, ((t + half).floor() as usize).min(x.len() - 1));
            let mut acc = 0.0;
            for (k, &v) in x.iter().enumerate().take(hi + 1).skip(lo) {
                let d = k as f64 - t;
                let u = d / half;
                let w = bessel_i0(BETA * (1.0 - u * u).max(0.0).sqrt()) / i0b;
                let a = std::f64::consts::PI * 2.0 * fc * d;
                let sinc = if a.abs() < 1e-12 { 1.0 } else { a.sin() / a };
                acc += v as f64 * 2.0 * fc * sinc * w;
            }
            acc as f32
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// conversion

/// Samples per uploaded IR (42.7 ms at 48 kHz).
pub const IR_TAPS: usize = 2048;
/// The pedal's sample rate.
pub const IR_SAMPLE_RATE: u32 = 48000;

/// An IR converted for upload.
#[derive(Clone, Debug)]
pub struct ConvertedIr {
    /// The [`IR_TAPS`] floats to upload: `data[IR_TAPS − 1 − k] = gain · ir[k]`.
    pub data: Vec<f32>,
    /// The gain applied.
    pub gain: f32,
    /// The IR at 48 kHz (the file's first channel, resampled if needed), before truncation.
    pub ir: Vec<f32>,
}

/// Convert an IR the way the official app does: first channel, resampled to 48 kHz, the first
/// [`IR_TAPS`] samples (zero-padded), time-reversed, times a gain. The gain is provisional (unit
/// energy over the uploaded samples; the app's rule is not known yet, docs/09).
pub fn convert(wav: &Wav) -> ConvertedIr {
    let ch = wav.channels.first().map(Vec::as_slice).unwrap_or(&[]);
    let ir = resample(ch, wav.sample_rate, IR_SAMPLE_RATE);
    let head = &ir[..ir.len().min(IR_TAPS)];
    let energy: f64 = head.iter().map(|&v| v as f64 * v as f64).sum();
    let gain = if energy > 0.0 { (1.0 / energy.sqrt()) as f32 } else { 1.0 };
    let mut data = vec![0.0f32; IR_TAPS];
    for (k, &v) in head.iter().enumerate() {
        data[IR_TAPS - 1 - k] = v * gain;
    }
    ConvertedIr { data, gain, ir }
}

/// The slot name the official app gives an imported IR: the file name without its extension, cut
/// to 16 characters (seen in the one captured import; the record has room for 31).
pub fn slot_name(path: &std::path::Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().chars().take(16).collect()).unwrap_or_default()
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
    fn write_and_read_back() {
        let w = Wav { sample_rate: 44100, channels: vec![vec![0.5, -0.25, 0.0], vec![0.125, 1.0 - 1.0 / 8_388_608.0, -1.0]] };
        for (bits, float) in [(24, false), (32, true)] {
            assert_eq!(Wav::parse(&w.to_bytes(bits, float)).unwrap(), w);
        }
        let r = Wav::parse(&w.to_bytes(16, false)).unwrap();
        assert!(r.channels[0].iter().zip(&w.channels[0]).all(|(a, b)| (a - b).abs() < 1e-4));
    }

    #[test]
    fn resampling_keeps_a_sine() {
        let f = 1000.0;
        let x: Vec<f32> = (0..4410).map(|n| (std::f64::consts::TAU * f * n as f64 / 44100.0).sin() as f32).collect();
        let y = resample(&x, 44100, 48000);
        assert_eq!(y.len(), 4800);
        // away from the ends (the window needs ~32 cycles of context)
        for (n, &v) in y.iter().enumerate().take(4400).skip(400) {
            let want = (std::f64::consts::TAU * f * n as f64 / 48000.0).sin() as f32;
            assert!((v - want).abs() < 1e-3, "sample {n}: {v} vs {want}");
        }
        assert_eq!(resample(&x, 48000, 48000), x);
        assert_eq!((resample(&x[..441], 44100, 48000).len(), resample(&x[..442], 44100, 48000).len()), (480, 482));
    }

    #[test]
    fn conversion_layout() {
        let mut ch = vec![0.0f32; 3000];
        (ch[0], ch[10], ch[2500]) = (0.3, -0.4, 0.9);
        let c = convert(&Wav { sample_rate: 48000, channels: vec![ch, vec![1.0; 3000]] });
        assert_eq!(c.data.len(), IR_TAPS);
        // reversed, first channel only, nothing past IR_TAPS, unit energy
        assert!((c.data[IR_TAPS - 1] - 0.6).abs() < 1e-6 && (c.data[IR_TAPS - 11] + 0.8).abs() < 1e-6);
        assert!((c.gain - 2.0).abs() < 1e-6);
        assert_eq!(c.data.iter().filter(|v| **v != 0.0).count(), 2);
        // short IRs are zero-padded
        let s = convert(&Wav { sample_rate: 48000, channels: vec![vec![0.5]] });
        assert_eq!((s.data[IR_TAPS - 1], s.data[0]), (1.0, 0.0));
        assert_eq!(slot_name(std::path::Path::new("/x/Delay And Reverb Ambient.wav")), "Delay And Reverb");
    }

    #[test]
    fn errors() {
        assert!(matches!(Wav::parse(b"nope"), Err(WavError::Format(_))));
        assert!(matches!(Wav::parse(&wav(2, 1, 48000, 4, &[0; 8])), Err(WavError::Unsupported(_))));
    }
}

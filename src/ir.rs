//! Impulse responses for the pedal's user CAB (IR) slots (docs/05): reading and writing WAV files,
//! resampling, and the conversion to the pedal's upload format.
//!
//! [`convert`] is the official app's IR conversion, recovered from its native code (docs/05) and
//! matching its uploads exactly: a file that isn't a 48 kHz mono 24-bit WAV is first converted to
//! one (first channel, JUCE's resampler, 24-bit); the first [`IR_TAPS`] samples are time-reversed and
//! scaled so that the response between 398 Hz and 3 kHz is centred on 0 dB.
//! The upload itself is [`crate::upload::user_ir_upload`] / `Device::import_user_ir`.

use std::fmt;

/// A decoded WAV file: one sample vector per channel, scaled to ±1.0.
#[derive(Clone, Debug, PartialEq)]
pub struct Wav {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
    /// How the file stored its samples (the official app converts anything but 24-bit PCM).
    pub encoding: Encoding,
}

/// Sample encoding of a WAV file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// Integer PCM with this many bits.
    Pcm(u16),
    /// IEEE float with this many bits.
    Float(u16),
}

impl Wav {
    /// In-memory audio (32-bit float, like a float WAV file).
    pub fn new(sample_rate: u32, channels: Vec<Vec<f32>>) -> Wav {
        Wav { sample_rate, channels, encoding: Encoding::Float(32) }
    }
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
        let encoding = if tag == FLOAT { Encoding::Float(bits as u16) } else { Encoding::Pcm(bits as u16) };
        Ok(Wav { sample_rate, channels: out, encoding })
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
/// Samples of an uploaded IR that the pedal actually plays (8 ms): `data[1664..2048]`, i.e. taps
/// 0..383. Measured by re-amping probe IRs (docs/05).
pub const PEDAL_IR_TAPS: usize = 384;
/// The pedal's sample rate.
pub const IR_SAMPLE_RATE: u32 = 48000;

/// An IR converted for upload.
#[derive(Clone, Debug)]
pub struct ConvertedIr {
    /// The [`IR_TAPS`] floats to upload: `data[IR_TAPS − 1 − k] = gain · ir[k]`.
    pub data: Vec<f32>,
    /// The gain applied.
    pub gain: f32,
    /// The IR at 48 kHz as the app sees it (after its conversion), before truncation.
    pub ir: Vec<f32>,
}

/// JUCE's `ResamplingAudioSource` low-pass (`createLowPass`): a 2nd-order Butterworth-style
/// section at the lower Nyquist frequency, as `(b0, b1, b2, a1, a2)`.
fn juce_low_pass(ratio: f64) -> [f64; 5] {
    let rate = if ratio > 1.0 { 0.5 / ratio } else { 0.5 * ratio };
    let n = 1.0 / (std::f64::consts::PI * rate.max(0.001)).tan();
    let n2 = n * n;
    let c1 = 1.0 / (1.0 + std::f64::consts::SQRT_2 * n + n2);
    [c1, c1 * 2.0, c1, c1 * 2.0 * (1.0 - n2), c1 * (1.0 - std::f64::consts::SQRT_2 * n + n2)]
}

/// JUCE's `applyFilter`: in place, double-precision state, float samples.
fn juce_filter(x: &mut [f32], c: &[f64; 5]) {
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for v in x {
        let i = *v as f64;
        let o = c[0] * i + c[1] * x1 + c[2] * x2 - c[3] * y1 - c[4] * y2;
        (x2, x1, y2, y1) = (x1, i, y1, o);
        *v = o as f32;
    }
}

/// Resample like the official app: JUCE's `ResamplingAudioSource` (linear interpolation, with its
/// low-pass applied to the input when downsampling and to the output when upsampling, none
/// within ±0.01 % of 1:1), writing `⌊len · to / from⌋` samples.
pub fn resample_like_app(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    let ratio = from as f64 / to as f64; // input samples per output sample
    let out_len = (x.len() as f64 / ratio) as usize;
    let filter = juce_low_pass(ratio);
    // the input as the interpolator sees it (zeros past the end, filtered too when downsampling)
    let mut src: Vec<f32> = x.to_vec();
    src.resize(x.len().max((out_len as f64 * ratio) as usize + 4), 0.0);
    if ratio > 1.0001 {
        juce_filter(&mut src, &filter);
    }
    let (mut pos, mut sub) = (0usize, 0.0f64);
    let mut out = Vec::with_capacity(out_len);
    for _ in 0..out_len {
        let alpha = sub as f32;
        let (a, b) = (src.get(pos).copied().unwrap_or(0.0), src.get(pos + 1).copied().unwrap_or(0.0));
        out.push(a + alpha * (b - a));
        sub += ratio;
        while sub >= 1.0 {
            pos += 1;
            sub -= 1.0;
        }
    }
    if ratio < 0.9999 {
        juce_filter(&mut out, &filter);
    }
    out
}

/// Write to and read back from a 24-bit WAV the way JUCE does (`× 8388607`, rounded, `÷ 8388608`).
fn via_24_bit(x: &mut [f32]) {
    for v in x {
        let q = ((*v as f64).clamp(-1.0, 1.0) * 8_388_607.0).round_ties_even().clamp(-8_388_608.0, 8_388_607.0);
        *v = (q / 8_388_608.0) as f32;
    }
}

/// FFT length and bins of the app's gain measurement: 16384 points, bins 136–1023 (398 Hz – 3 kHz).
const GAIN_FFT: usize = 16384;
const GAIN_BINS: std::ops::Range<usize> = 136..1024;

/// The app's gain: `1 / √(max |X| · min |X|)` over bins 136–1023 of the 16384-point spectrum of
/// the first [`IR_TAPS`] samples, i.e. the band's peak and trough end up symmetric about 0 dB.
pub fn app_gain(head: &[f32]) -> f64 {
    let n = GAIN_FFT as f64;
    // a direct DFT over the 888 bins (no FFT dependency; a few ms)
    let (mut lo, mut hi) = (f64::MAX, 0f64);
    for k in GAIN_BINS {
        let w = std::f64::consts::TAU * k as f64 / n;
        let (c, s) = (w.cos(), w.sin());
        let (mut re, mut im, mut cr, mut ci) = (0.0, 0.0, 1.0f64, 0.0f64);
        for &v in head.iter().take(IR_TAPS) {
            re += v as f64 * cr;
            im -= v as f64 * ci;
            (cr, ci) = (cr * c - ci * s, cr * s + ci * c);
        }
        let p = re * re + im * im;
        (lo, hi) = (lo.min(p), hi.max(p));
    }
    if lo > 0.0 && hi.is_finite() { 1.0 / (lo.sqrt() * hi.sqrt()).sqrt() } else { 1.0 }
}

/// Convert an IR exactly as the official app does (docs/05):
/// 1. unless the file is a 48 kHz mono 24-bit WAV, convert it to one: the first channel, resampled
///    with JUCE's `ResamplingAudioSource` ([`resample_like_app`]), stored as 24-bit;
/// 2. take the first [`IR_TAPS`] samples (zero-padded), time-reversed;
/// 3. scale by [`app_gain`].
pub fn convert(wav: &Wav) -> ConvertedIr {
    let ch = wav.channels.first().map(Vec::as_slice).unwrap_or(&[]);
    let direct = wav.sample_rate == IR_SAMPLE_RATE && wav.channels.len() == 1 && wav.encoding == Encoding::Pcm(24);
    let ir = if direct {
        ch.to_vec()
    } else {
        let mut v = resample_like_app(ch, wav.sample_rate, IR_SAMPLE_RATE);
        via_24_bit(&mut v);
        v
    };
    let head = &ir[..ir.len().min(IR_TAPS)];
    let gain = app_gain(head);
    let mut data = vec![0.0f32; IR_TAPS];
    for (k, &v) in head.iter().enumerate() {
        data[IR_TAPS - 1 - k] = (gain * v as f64) as f32;
    }
    ConvertedIr { data, gain: gain as f32, ir }
}

// ---------------------------------------------------------------------------------------------
// optimised conversion (feature `nam`, for the FFT)

/// FFT length of the minimum-phase design (1.46 Hz per bin at 48 kHz).
#[cfg(feature = "nam")]
const DESIGN_FFT: usize = 1 << 15;
/// Smoothing of the target response: ±1/12 octave (1/6-octave bands).
#[cfg(feature = "nam")]
const SMOOTH: f64 = 1.059_463_094_359_295_3; // 2^(1/12)

/// Fractional-octave power smoothing on an rfft grid (`p`: power per bin).
#[cfg(feature = "nam")]
fn smooth_power(p: &[f64]) -> Vec<f64> {
    let mut pre = Vec::with_capacity(p.len() + 1);
    pre.push(0.0);
    for v in p {
        pre.push(pre.last().unwrap() + v);
    }
    (0..p.len())
        .map(|k| {
            let lo = ((k as f64 / SMOOTH).floor() as usize).min(k);
            let hi = ((k as f64 * SMOOTH).ceil() as usize).max(k).min(p.len() - 1);
            (pre[hi + 1] - pre[lo]) / (hi + 1 - lo) as f64
        })
        .collect()
}

/// `|FFT(x, n)|²` for bins 0..=n/2.
#[cfg(feature = "nam")]
fn power_spectrum(x: &[f64], n: usize) -> Vec<f64> {
    use rustfft::num_complex::Complex64;
    let mut c: Vec<Complex64> = x.iter().take(n).map(|&v| Complex64::new(v, 0.0)).collect();
    c.resize(n, Complex64::default());
    rustfft::FftPlanner::new().plan_fft_forward(n).process(&mut c);
    c[..n / 2 + 1].iter().map(|v| v.norm_sqr()).collect()
}

/// A minimum-phase impulse response with magnitude `mag` (bins 0..=n/2 of an `n`-point grid),
/// by the folded real cepstrum.
#[cfg(feature = "nam")]
fn min_phase(mag: &[f64], taps: usize) -> Vec<f64> {
    use rustfft::num_complex::Complex64;
    let n = (mag.len() - 1) * 2;
    let mut p = rustfft::FftPlanner::new();
    let (fwd, inv) = (p.plan_fft_forward(n), p.plan_fft_inverse(n));
    let floor = mag.iter().fold(0f64, |m, &v| m.max(v)) * 1e-6;
    let mut c: Vec<Complex64> = (0..n).map(|k| Complex64::new(mag[k.min(n - k)].max(floor).ln(), 0.0)).collect();
    inv.process(&mut c);
    for (i, v) in c.iter_mut().enumerate() {
        let w = match i {
            0 => 1.0,
            i if i < n / 2 => 2.0,
            i if i == n / 2 => 1.0,
            _ => 0.0,
        };
        *v = Complex64::new(v.re / n as f64 * w, 0.0);
    }
    fwd.process(&mut c);
    for v in c.iter_mut() {
        *v = v.exp();
    }
    inv.process(&mut c);
    c[..taps].iter().map(|v| v.re / n as f64).collect()
}

/// Range of the fit (Hz). A cab IR has next to nothing above 16 kHz (−80 dB and below), and
/// matching that costs taps the rest of the range needs.
#[cfg(feature = "nam")]
const FIT_LO_HZ: f64 = 30.0;
#[cfg(feature = "nam")]
const FIT_HI_HZ: f64 = 16000.0;

/// Bands of the fit: 1/6 octave wide, every 1/12 octave from [`FIT_LO_HZ`] to [`FIT_HI_HZ`], as
/// bin ranges of an `n`-point FFT at 48 kHz.
#[cfg(feature = "nam")]
fn fit_bands(n: usize) -> Vec<std::ops::Range<usize>> {
    let hz = |f: f64| f * n as f64 / IR_SAMPLE_RATE as f64;
    std::iter::successors(Some(FIT_LO_HZ), |f| Some(f * 2f64.powf(1.0 / 12.0)))
        .take_while(|&f| f <= FIT_HI_HZ)
        .map(|f| {
            let (lo, hi) = (hz(f / SMOOTH).floor() as usize, hz(f * SMOOTH).ceil() as usize);
            lo..hi.max(lo + 1)
        })
        .collect()
}

/// Fit `h0`'s taps directly to the full IR's band levels: Adam on the variance of the dB error
/// over [`fit_bands`] (the level is left free; [`app_gain`] sets it afterwards), with the gradient
/// through the FFT written out.
#[cfg(feature = "nam")]
fn refine(h0: &[f64], nf: usize, full_smoothed: &[f64]) -> Vec<f64> {
    use rustfft::num_complex::Complex64;
    let n = DESIGN_FFT;
    let bands = fit_bands(n);
    // target band levels (dB) from the full IR's spectrum, on the same frequencies
    let step = nf / n;
    let target: Vec<f64> = bands
        .iter()
        .map(|b| {
            let p = b.clone().map(|k| full_smoothed[k * step]).sum::<f64>() / b.len() as f64;
            10.0 * p.max(1e-30).log10()
        })
        .collect();
    let mut planner = rustfft::FftPlanner::new();
    let (fwd, inv) = (planner.plan_fft_forward(n), planner.plan_fft_inverse(n));
    let scale = h0.iter().fold(0f64, |m, v| m.max(v.abs())).max(1e-12);
    let mut h: Vec<f64> = h0.iter().map(|v| v / scale).collect();
    let eval = |h: &[f64], grad: Option<&mut [f64]>| -> f64 {
        let mut c: Vec<Complex64> = h.iter().map(|&v| Complex64::new(v, 0.0)).collect();
        c.resize(n, Complex64::default());
        fwd.process(&mut c);
        let p: Vec<f64> = bands.iter().map(|b| b.clone().map(|k| c[k].norm_sqr()).sum::<f64>() / b.len() as f64).collect();
        let d: Vec<f64> = p.iter().zip(&target).map(|(p, t)| 10.0 * p.max(1e-30).log10() - t).collect();
        let mean = d.iter().sum::<f64>() / d.len() as f64;
        let e = d.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / d.len() as f64;
        if let Some(g) = grad {
            // dE/dH_k for every bin of every band, then back to the taps through the FFT
            let mut gk = vec![Complex64::default(); n];
            for ((b, dv), pv) in bands.iter().zip(&d).zip(&p) {
                let de_dp = 2.0 * (dv - mean) / d.len() as f64 * 10.0 / (std::f64::consts::LN_10 * pv.max(1e-30));
                for k in b.clone() {
                    gk[k] += c[k] * (2.0 * de_dp / b.len() as f64);
                }
            }
            inv.process(&mut gk);
            for (gi, v) in g.iter_mut().zip(&gk) {
                *gi = v.re;
            }
        }
        e
    };
    let (b1, b2, eps, lr) = (0.9, 0.999, 1e-12, 2e-3);
    let (mut m, mut v) = (vec![0.0; h.len()], vec![0.0; h.len()]);
    let mut g = vec![0.0; h.len()];
    let mut best = (eval(&h, None), h.clone());
    let steps: usize = 400;
    for it in 0..steps {
        let e = eval(&h, Some(&mut g));
        if e < best.0 {
            best = (e, h.clone());
        }
        let t = (it + 1) as i32;
        let rate = lr * 0.5 * (1.0 + (std::f64::consts::PI * it as f64 / steps as f64).cos());
        for i in 0..h.len() {
            m[i] = b1 * m[i] + (1.0 - b1) * g[i];
            v[i] = b2 * v[i] + (1.0 - b2) * g[i] * g[i];
            h[i] -= rate * (m[i] / (1.0 - f64::powi(b1, t))) / ((v[i] / (1.0 - f64::powi(b2, t))).sqrt() + eps);
        }
    }
    let e = eval(&h, None);
    if e < best.0 {
        best = (e, h);
    }
    best.1.iter().map(|v| v * scale).collect()
}

/// The optimised conversion: a [`PEDAL_IR_TAPS`]-tap filter (the part of an upload the pedal
/// plays) with the whole IR's 1/6-octave magnitude response, instead of the IR's first 2048 samples
/// cut off by the pedal after 384. The file is resampled with [`resample`] (windowed sinc); the
/// filter is minimum-phase (no leading delay, energy packed into the start), refined over a few
/// iterations, then fitted tap by tap to the full IR's band levels. Its level is the one the app
/// means to give the IR ([`app_gain`] on the IR's first 2048 samples from its onset), matched
/// over 100 Hz – 8 kHz. The unused part of the upload is zero.
#[cfg(feature = "nam")]
pub fn convert_optimised(wav: &Wav) -> ConvertedIr {
    let ch = wav.channels.first().map(Vec::as_slice).unwrap_or(&[]);
    let ir = resample(ch, wav.sample_rate, IR_SAMPLE_RATE);
    let x: Vec<f64> = ir.iter().map(|&v| v as f64).collect();
    let nf = x.len().next_power_of_two().max(DESIGN_FFT);
    let full = smooth_power(&power_spectrum(&x, nf));
    let step = nf / DESIGN_FFT;
    // the target on the design grid (magnitude), never below -120 dB of its peak
    let target: Vec<f64> = (0..=DESIGN_FFT / 2).map(|k| full[k * step].sqrt()).collect();
    let peak = target.iter().fold(0f64, |m, &v| m.max(v));
    let target: Vec<f64> = target.iter().map(|&v| v.max(peak * 1e-6)).collect();

    // the last 48 taps fade out (half cosine), so the cut at 384 doesn't ring
    let fade = 48;
    let window = |h: &mut [f64]| {
        for (i, v) in h.iter_mut().enumerate().skip(PEDAL_IR_TAPS - fade) {
            let t = (i - (PEDAL_IR_TAPS - fade)) as f64 / fade as f64;
            *v *= 0.5 + 0.5 * (std::f64::consts::PI * t).cos();
        }
    };
    // error of a design: rms of the log-magnitude difference over 30 Hz – 16 kHz (1/6-octave bins),
    // with the level difference removed
    let lo_bin = (30.0 * DESIGN_FFT as f64 / 48000.0) as usize;
    let hi_bin = (16000.0 * DESIGN_FFT as f64 / 48000.0) as usize;
    let probe: Vec<usize> = std::iter::successors(Some(lo_bin as f64), |b| Some(b * 2f64.powf(1.0 / 24.0)))
        .map(|b| b.round() as usize)
        .take_while(|&b| b <= hi_bin)
        .collect();
    let response = |h: &[f64]| -> Vec<f64> { smooth_power(&power_spectrum(h, DESIGN_FFT)).iter().map(|p| p.sqrt()).collect() };
    let error = |r: &[f64]| {
        let d: Vec<f64> = probe.iter().map(|&k| 20.0 * (r[k].max(1e-30) / target[k]).log10()).collect();
        let mean = d.iter().sum::<f64>() / d.len() as f64;
        (d.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / d.len() as f64).sqrt()
    };
    let mut design = target.clone();
    let mut best: Option<(f64, Vec<f64>)> = None;
    for _ in 0..12 {
        let mut h = min_phase(&design, PEDAL_IR_TAPS);
        window(&mut h);
        let r = response(&h);
        let e = error(&r);
        if best.as_ref().is_none_or(|b| e < b.0) {
            best = Some((e, h));
        }
        // push the design towards the target where the 384 taps fall short, within ±12 dB
        for k in 0..design.len() {
            let corr = (target[k] / r[k].max(1e-30)).powf(0.8);
            design[k] = (design[k] * corr).clamp(target[k] / 4.0, target[k] * 4.0);
        }
    }
    let h0 = best.map(|b| b.1).unwrap_or_default();
    let h = refine(&h0, nf, &full);
    // level: what the app means to give this IR (its rule on the first 2048 samples, counted from
    // the IR's onset so leading silence doesn't skew it), matched over 100 Hz – 8 kHz
    let peak_x = x.iter().fold(0f64, |m, v| m.max(v.abs()));
    let onset = x.iter().position(|v| v.abs() > 0.01 * peak_x).unwrap_or(0).saturating_sub(8);
    let head: Vec<f32> = x[onset..].iter().take(IR_TAPS).map(|&v| v as f32).collect();
    let g_ref = app_gain(&head);
    let bands = fit_bands(DESIGN_FFT);
    let step = nf / DESIGN_FFT;
    let ours = smooth_power(&power_spectrum(&h, DESIGN_FFT));
    let (mut sum, mut count) = (0.0, 0);
    for b in &bands {
        let f = b.start as f64 * IR_SAMPLE_RATE as f64 / DESIGN_FFT as f64;
        if (100.0..=8000.0).contains(&f) {
            let o = b.clone().map(|k| ours[k]).sum::<f64>() / b.len() as f64;
            let t = b.clone().map(|k| full[k * step]).sum::<f64>() / b.len() as f64 * g_ref * g_ref;
            sum += 10.0 * (t.max(1e-30) / o.max(1e-30)).log10();
            count += 1;
        }
    }
    let gain = 10f64.powf(sum / count.max(1) as f64 / 20.0);
    let h: Vec<f32> = h.iter().map(|&v| v as f32).collect();
    let mut data = vec![0.0f32; IR_TAPS];
    for (k, &v) in h.iter().enumerate() {
        data[IR_TAPS - 1 - k] = (gain * v as f64) as f32;
    }
    ConvertedIr { data, gain: gain as f32, ir }
}

/// How an IR is converted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Method {
    /// Exactly the official app's conversion ([`convert`]).
    #[default]
    App,
    /// A 384-tap filter fitted to the whole IR ([`convert_optimised`]; feature `nam`).
    #[cfg(feature = "nam")]
    Optimised,
}

/// Convert with the given [`Method`].
pub fn convert_with(wav: &Wav, method: Method) -> ConvertedIr {
    match method {
        Method::App => convert(wav),
        #[cfg(feature = "nam")]
        Method::Optimised => convert_optimised(wav),
    }
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
        let w = Wav::new(44100, vec![vec![0.5, -0.25, 0.0], vec![0.125, 1.0 - 1.0 / 8_388_608.0, -1.0]]);
        for (bits, float, enc) in [(24, false, Encoding::Pcm(24)), (32, true, Encoding::Float(32))] {
            assert_eq!(Wav::parse(&w.to_bytes(bits, float)).unwrap(), Wav { encoding: enc, ..w.clone() });
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
        (ch[0], ch[2500]) = (0.25, 0.9);
        let mut w = Wav::new(48000, vec![ch, vec![1.0; 3000]]);
        let c = convert(&w);
        assert_eq!(c.data.len(), IR_TAPS);
        // first channel, reversed, nothing past IR_TAPS; a lone impulse is scaled to 1
        assert!((c.data[IR_TAPS - 1] - 1.0).abs() < 1e-6 && (c.gain - 4.0).abs() < 1e-5);
        assert_eq!(c.data.iter().filter(|v| **v != 0.0).count(), 1);
        // short IRs are zero-padded; a 48 kHz mono 24-bit file is used as is
        w = Wav { sample_rate: 48000, channels: vec![vec![-0.5]], encoding: Encoding::Pcm(24) };
        let s = convert(&w);
        assert_eq!((s.data[IR_TAPS - 1], s.data[0], s.ir.len()), (-1.0, 0.0, 1));
        assert_eq!(slot_name(std::path::Path::new("/x/Delay And Reverb Ambient.wav")), "Delay And Reverb");
    }

    #[test]
    fn app_gain_centres_the_mid_band() {
        // two impulses whose response swings between about 0.75 and 0.25 across the band
        // (≈ 1/√(0.75·0.25)); 2.309391 is what the official app applied to this IR
        let mut x = vec![0.0f32; 2048];
        (x[0], x[1000]) = (0.5, -0.25);
        assert!((app_gain(&x) / 2.309391 - 1.0).abs() < 1e-6, "{}", app_gain(&x));
    }

    #[test]
    fn app_resampler_basics() {
        // 1:1 is a copy; the output length is ⌊len · to / from⌋
        let x: Vec<f32> = (0..100).map(|i| (i as f32 * 0.1).sin()).collect();
        assert_eq!(resample_like_app(&x, 48000, 48000), x);
        assert_eq!((resample_like_app(&x, 44100, 48000).len(), resample_like_app(&x, 96000, 48000).len()), (108, 50));
        // DC passes the low-pass at unity gain
        let dc = resample_like_app(&[0.5; 4000], 44100, 48000);
        assert!((dc[3000] - 0.5).abs() < 1e-4);
    }

    #[cfg(feature = "nam")]
    #[test]
    fn optimised_conversion_fits_a_delayed_ir_into_384_taps() {
        // a decaying resonance that starts 1000 samples in: the official upload puts it out of the
        // pedal's reach; the optimised one keeps its response within the 384 played taps
        let x: Vec<f32> = (0..6000)
            .map(|n| if n < 1000 { 0.0 } else { let t = (n - 1000) as f32; (-t / 120.0).exp() * (t * 0.05).sin() + if n == 1000 { 0.5 } else { 0.0 } })
            .collect();
        let w = Wav::new(48000, vec![x]);
        let c = convert_optimised(&w);
        assert_eq!(c.data.len(), IR_TAPS);
        assert!(c.data[..IR_TAPS - PEDAL_IR_TAPS].iter().all(|&v| v == 0.0));
        let played: f32 = c.data[IR_TAPS - PEDAL_IR_TAPS..].iter().map(|v| v * v).sum();
        let official = convert(&w);
        let official_played: f32 = official.data[IR_TAPS - PEDAL_IR_TAPS..].iter().map(|v| v * v).sum();
        assert!(c.data.iter().all(|v| v.is_finite()) && played > 1e3 * official_played, "{played} vs {official_played}");
        assert_eq!(convert_with(&w, Method::App).data, official.data);
    }

    #[test]
    fn errors() {
        assert!(matches!(Wav::parse(b"nope"), Err(WavError::Format(_))));
        assert!(matches!(Wav::parse(&wav(2, 1, 48000, 4, &[0; 8])), Err(WavError::Unsupported(_))));
    }
}

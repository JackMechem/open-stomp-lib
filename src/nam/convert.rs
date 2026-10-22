//! NAM model → pedal user AMP data: the official app's converter, reimplemented (docs/05).
//!
//! The pedal doesn't run the network. It runs *G1 → its own static nonlinearity → G2*, two
//! 1024-tap FIR filters fitted to the model:
//!
//! 1. **Best linear approximation**: a pink multisine at about −90 dBFS through the model;
//!    `H = |Y| / |X|` per FFT bin, plus a 1/24-octave smoothed copy.
//! 2. **Tone test**: a full-scale sine at each of 94 frequencies (58.6 Hz – 14.6 kHz); the level of
//!    the fundamental and of the 3rd harmonic at the output.
//! 3. `G1 = smoothed H / max(fundamental, 3rd harmonic)` at the tone frequencies, interpolated in
//!    log-frequency; `G2 = H / G1`.
//! 4. Both become minimum-phase impulse responses; the upload is the first 1024 taps of each,
//!    reversed, and a factor of 1.0.
//!
//! The pedal only runs the first [`PEDAL_TAPS`] (512) taps of each uploaded filter (found by
//! re-amping, docs/05): the app's extra 512 taps per filter are ignored. [`Measure::App`] keeps the
//! app's layout byte for byte; the other modes upload exactly what the pedal runs ([`upload_data`]).
//!
//! Every stage was checked against the vendor code (open-stomp-manager `tools/namconv`). The one
//! difference is the excitation: the vendor's 8192-sample multisine is a stored table, so
//! [`excitation`] generates our own with the same spectrum (see [`Converted`] for how close the
//! results are).

use rustfft::FftPlanner;
use rustfft::num_complex::Complex64;

use super::Model;

/// FFT length of every measurement (48 kHz → 5.859375 Hz per bin).
pub const N: usize = 8192;
const HALF: usize = N / 2;
/// Samples per tone, of which the first [`TONE_SETTLE`] are not measured.
const TONE_LEN: usize = 10240;
const TONE_SETTLE: usize = 2048;

/// Tone-test frequencies as FFT bins (the app's `ToneTest_FftBins`): roughly 1/12 octave apart.
pub const TONE_BINS: [usize; 94] = [
    10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 23, 24, 25, 27, 28, 30, 32, 34, 36, 38, 41, 43, 46, 48, 51, 54,
    58, 61, 65, 69, 73, 77, 82, 86, 92, 97, 103, 109, 116, 123, 130, 138, 146, 155, 164, 174, 184, 195, 207, 219, 232,
    246, 260, 276, 292, 310, 328, 348, 369, 391, 414, 439, 465, 492, 522, 553, 586, 621, 658, 697, 738, 782, 829, 878,
    931, 986, 1045, 1107, 1173, 1243, 1317, 1395, 1478, 1566, 1660, 1758, 1863, 1974, 2092, 2216, 2348, 2488,
];
/// Impulse-response taps of each uploaded filter that the pedal actually runs. An upload holds
/// 1024 floats per filter, the response reversed (`data[1023 − k]` is tap `k` of G1,
/// `data[2047 − k]` of G2), but the pedal uses only taps 0..511 (`data[512..1024]`,
/// `data[1536..2048]`) and ignores the rest.
pub const PEDAL_TAPS: usize = 512;
/// Tones whose 3rd harmonic is measured (3 × bin stays below Nyquist).
const THIRD_TONES: usize = 83;

/// The result of a conversion, with every intermediate stage.
#[derive(Clone, Debug)]
pub struct Converted {
    /// The 2048 floats to upload (`g1 ‖ g2`).
    pub data: Vec<f32>,
    /// The slot-info factor (always 1.0, as in the official app).
    pub factor: f32,
    /// Fundamental level per tone.
    pub levels: Vec<f64>,
    /// 3rd-harmonic level per tone (first 83 tones).
    pub levels3: Vec<f64>,
    /// The measured linear response `20·log10 H`, bins 0..4095.
    pub bla_db: Vec<f64>,
    /// G1 target gain per tone, in dB.
    pub g1_gains_db: Vec<f64>,
    /// The 8192-tap minimum-phase responses before truncation.
    pub g1_ir: Vec<f64>,
    pub g2_ir: Vec<f64>,
}

/// Upload data for two impulse responses as the pedal runs them: the first [`PEDAL_TAPS`] taps of
/// each, reversed, in the second half of each 1024-float block; the halves the pedal ignores are 0.
pub fn upload_data(g1_ir: &[f64], g2_ir: &[f64]) -> Vec<f32> {
    let block = |ir: &[f64]| {
        let mut b = vec![0.0f32; 1024];
        for (k, &v) in ir.iter().take(PEDAL_TAPS).enumerate() {
            b[1023 - k] = v as f32;
        }
        b
    };
    [block(g1_ir), block(g2_ir)].concat()
}

/// The two impulse responses the pedal actually runs for 2048 floats of upload data
/// ([`PEDAL_TAPS`] taps each; the inverse of [`upload_data`]).
pub fn pedal_irs(data: &[f32]) -> (Vec<f64>, Vec<f64>) {
    assert_eq!(data.len(), 2048, "upload data is 2048 floats");
    let ir = |b: &[f32]| (0..PEDAL_TAPS).map(|k| b[1023 - k] as f64).collect();
    (ir(&data[..1024]), ir(&data[1024..]))
}

/// The DC blocker the app applies to every model output; its state carries over between calls.
#[derive(Default)]
struct DcBlocker {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl DcBlocker {
    fn run(&mut self, x: &mut [f32]) {
        const B0: f32 = 0.995_382_7;
        const B1: f32 = -1.990_765;
        const B2: f32 = 0.995_382_7;
        const A1: f32 = -1.990_744;
        const A2: f32 = 0.990_786_7;
        for v in x {
            let x0 = *v;
            let y0 = (self.x1 * B1 + x0 * B0 + self.x2 * B2) - (self.y2 * A2 + self.y1 * A1);
            *v = y0;
            (self.x2, self.x1, self.y2, self.y1) = (self.x1, x0, self.y1, y0);
        }
    }
}

struct Fft {
    fwd: std::sync::Arc<dyn rustfft::Fft<f64>>,
    inv: std::sync::Arc<dyn rustfft::Fft<f64>>,
}

impl Fft {
    fn new() -> Self {
        let mut p = FftPlanner::new();
        Fft { fwd: p.plan_fft_forward(N), inv: p.plan_fft_inverse(N) }
    }

    /// `|FFT(x)|²` for bins 0..4095 (no window, no scaling), like the app's `getSpectrum`.
    fn power(&self, x: &[f32]) -> Vec<f64> {
        let mut buf: Vec<Complex64> = x.iter().map(|&v| Complex64::new(v as f64, 0.0)).collect();
        buf.resize(N, Complex64::default());
        self.fwd.process(&mut buf);
        buf[..HALF].iter().map(|c| c.norm_sqr()).collect()
    }

    /// dB curve over bins 0..4095 → 8192-tap minimum-phase impulse response (`Magnitude2MinPhase`).
    fn min_phase(&self, db: &[f64]) -> Vec<f64> {
        // the app mirrors as m[8191 - k] = m[k]
        let m: Vec<f64> = (0..N).map(|i| 10f64.powf(db[if i < HALF { i } else { N - 1 - i }] / 20.0)).collect();
        let mut c: Vec<Complex64> =
            m.iter().map(|&v| Complex64::new(if v == 0.0 { -30.0 } else { v.ln() }, 0.0)).collect();
        // analytic signal of ln|m|: keep DC and Nyquist, double positive, zero negative frequencies
        self.fwd.process(&mut c);
        for (i, v) in c.iter_mut().enumerate() {
            *v *= match i {
                0 => 1.0,
                i if i == HALF => 1.0,
                i if i < HALF => 2.0,
                _ => 0.0,
            } / N as f64;
        }
        self.inv.process(&mut c);
        let mut spec: Vec<Complex64> = m.iter().zip(&c).map(|(&mag, a)| Complex64::from_polar(mag, -a.im)).collect();
        self.inv.process(&mut spec);
        spec.iter().map(|v| v.re / N as f64).collect()
    }
}

/// 1/24-octave power smoothing with log-frequency triangular weights (bins 2..4095).
fn smooth(h: &[f64]) -> Vec<f64> {
    let r = 1.014_545_3_f32; // 2^(1/48)
    let mut out = vec![0.0; HALF];
    for (k, o) in out.iter_mut().enumerate().skip(2) {
        let lo = ((k as f32 / r).floor() as usize).max(2);
        let hi = ((k as f32 * r).ceil() as usize).clamp(2, HALF - 1).max(lo);
        let m = k.max(lo);
        let (lm, llo, lhi) = ((m as f64).log10(), ((lo - 1) as f64).log10(), ((hi + 1) as f64).log10());
        let (mut sw, mut sp) = (0.0, 0.0);
        for (j, hj) in h.iter().enumerate().take(hi + 1).skip(lo) {
            let lj = (j as f64).log10();
            let w = if j > m {
                if lhi != lm { (lhi - lj) / (lhi - lm) } else { 1.0 }
            } else {
                (lj - llo) / (lm - llo)
            };
            sw += w;
            sp += w * hj * hj;
        }
        *o = sp / sw;
    }
    out[0] = out[2];
    out[1] = out[2];
    out.iter().map(|p| p.sqrt()).collect()
}

/// Values at `bins`, linearly interpolated over all bins 0..4095 (held outside).
fn interp_linear(values: &[f64], bins: &[usize], below: f64) -> Vec<f64> {
    let mut out = vec![below; HALF];
    for i in 0..bins.len() - 1 {
        for (k, o) in out.iter_mut().enumerate().take(bins[i + 1]).skip(bins[i]) {
            let t = (k - bins[i]) as f64 / (bins[i + 1] - bins[i]) as f64;
            *o = t * values[i + 1] + (1.0 - t) * values[i];
        }
    }
    let last = *bins.last().expect("bins");
    out[last..].fill(*values.last().expect("values"));
    out
}

/// Values at `bins`, interpolated in log-frequency over all bins 0..4095 (held outside).
fn interp_log(values: &[f64], bins: &[usize]) -> Vec<f64> {
    let mut out = vec![values[0]; HALF];
    for i in 0..bins.len() - 1 {
        let (l0, l1) = ((bins[i] as f64).log10(), (bins[i + 1] as f64).log10());
        for (k, o) in out.iter_mut().enumerate().take(bins[i + 1]).skip(bins[i]) {
            let t = ((k as f64).log10() - l0) / (l1 - l0);
            *o = t * values[i + 1] + (1.0 - t) * values[i];
        }
    }
    let last = *bins.last().expect("bins");
    out[last..].fill(*values.last().expect("values"));
    out
}

/// First 8 bytes and FNV-1a 64-bit hash of the official app's 8192-sample BLA excitation (its
/// `BLA_x` table, float32 little-endian). Only this fingerprint is in the source: the table itself
/// is vendor data, read from the user's own copy of the app by [`official_excitation`].
const OFFICIAL_PREFIX: [u8; 8] = [0xcf, 0xc0, 0x91, 0x37, 0x11, 0x79, 0x17, 0x37];
const OFFICIAL_FNV1A64: u64 = 0x4aa0_4ced_5cff_86d2;

fn fnv1a64(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325, |h, &c| (h ^ c as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Find the official app's BLA excitation in one file's bytes.
fn find_official(bytes: &[u8]) -> Option<Vec<f32>> {
    let len = N * 4;
    let mut i = 0;
    while i + len <= bytes.len() {
        match bytes[i..bytes.len() - len + OFFICIAL_PREFIX.len()].windows(8).position(|w| w == OFFICIAL_PREFIX) {
            None => return None,
            Some(p) => {
                let at = i + p;
                let table = &bytes[at..at + len];
                if fnv1a64(table) == OFFICIAL_FNV1A64 {
                    return Some(table.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().expect("4"))).collect());
                }
                i = at + 1;
            }
        }
    }
    None
}

/// Read the official app's BLA excitation from an installed copy of Stomp Manager: a file (macOS
/// `imagelib.framework/imagelib`) or a directory searched recursively (the `.app` bundle on macOS,
/// the install folder on Windows). Nothing is executed; the table is located by a fingerprint.
///
/// With it, [`convert_with`] reproduces the official app's conversion (≤ 0.03 dB). Without it, the
/// result differs by the measurement noise of the app's method (see [`convert`]): no other
/// excitation, including one with the same spectrum, gets closer.
pub fn official_excitation(path: impl AsRef<std::path::Path>) -> Result<Vec<f32>, super::NamError> {
    fn search(p: &std::path::Path, depth: usize) -> Option<Vec<f32>> {
        let meta = std::fs::symlink_metadata(p).ok()?;
        if meta.is_file() {
            // the table lives in a native library of a few MB; skip big media files
            return (meta.len() < 200 << 20).then(|| std::fs::read(p).ok()).flatten().and_then(|b| find_official(&b));
        }
        if meta.is_dir() && depth < 12 {
            let mut entries: Vec<_> = std::fs::read_dir(p).ok()?.filter_map(|e| e.ok().map(|e| e.path())).collect();
            // look at likely places first
            entries.sort_by_key(|e| {
                let n = e.to_string_lossy().to_lowercase();
                !(n.contains("imagelib") || n.contains("frameworks") || n.ends_with(".dll"))
            });
            return entries.iter().find_map(|e| search(e, depth + 1));
        }
        None
    }
    let p = path.as_ref();
    search(p, 0).ok_or_else(|| {
        super::NamError::Format(format!("no Stomp Manager BLA excitation found in {}", p.display()))
    })
}

/// Standard install locations of the official app, for [`official_excitation`].
pub fn official_app_locations() -> Vec<std::path::PathBuf> {
    let mut v = vec![std::path::PathBuf::from("/Applications/Stomp Manager.app")];
    if let Some(home) = std::env::var_os("HOME") {
        v.push(std::path::Path::new(&home).join("Applications/Stomp Manager.app"));
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(dir) = std::env::var_os(var) {
            v.push(std::path::Path::new(&dir).join("Stomp Manager"));
        }
    }
    v
}

/// [`official_excitation`] from the first standard install location that has it.
pub fn find_official_excitation() -> Option<Vec<f32>> {
    official_app_locations().iter().filter(|p| p.exists()).find_map(|p| official_excitation(p).ok())
}

/// Our BLA excitation: the same spectrum as the app's (amplitude `√(1/max(f, 40 Hz))` per bin,
/// overall scale 1e-5, about −90 dBFS peak), with phases from a fixed-seed generator.
pub fn excitation() -> Vec<f32> {
    let mut seed: u64 = 0x4f50_454e_5354_4f4d; // "OPENSTOM"
    let mut next = || {
        // xorshift64*
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        (seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut spec = vec![Complex64::default(); N];
    for (k, s) in spec.iter_mut().enumerate().take(HALF).skip(1) {
        let f = k as f64 * 48000.0 / N as f64;
        let a = (1.0 / f.max(40.0)).sqrt();
        // a·sin(φ + ωn) has DFT coefficient (N/2)·a·e^{j(φ−π/2)} at bin k
        *s = Complex64::from_polar(N as f64 / 2.0 * a, next() * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2);
    }
    // a DC term of the same magnitude as the flat low bins, like the app's (H[0] must be finite)
    spec[0] = Complex64::new(N as f64 / 2.0 * (1.0f64 / 40.0).sqrt(), 0.0);
    for k in 1..HALF {
        spec[N - k] = spec[k].conj();
    }
    FftPlanner::new().plan_fft_inverse(N).process(&mut spec);
    spec.iter().map(|c| (c.re / N as f64 * 1e-5) as f32).collect()
}

/// How the small-signal response `H` is measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Measure {
    /// Exactly as the official app: one 8192-sample run from the model's reset state, DC blocker
    /// starting from zero. Below ~100 Hz the blocker's start-up transient dominates (docs/05), so
    /// the low bass of the result follows that transient rather than the model.
    #[default]
    App,
    /// The excitation played twice and measured on the second, steady-state period: no transient,
    /// the model's actual small-signal response at every frequency. Departs from the app on purpose,
    /// and uploads only the taps the pedal runs ([`upload_data`]).
    Clean,
}

/// Convert a model with our excitation (see [`excitation`]), the app's way. Differs from the
/// official app's result by the app's own measurement noise (1/3-octave response typically within
/// 0.1–1.5 dB). For an exact match, use [`convert_with`] and [`official_excitation`].
pub fn convert(model: &mut Model) -> Converted {
    convert_with(model, &excitation())
}

/// Convert a model with a given 8192-sample BLA excitation, the app's way. Resets the model first.
///
/// With the official app's own excitation (its `BLA_x` table, which can be dumped from an installed
/// copy with the open-stomp-manager `tools/namconv/harness`), the result matches the app's upload
/// to within float rounding (≤ 0.03 dB in 1/3-octave response).
pub fn convert_with(model: &mut Model, bla_x: &[f32]) -> Converted {
    convert_measured(model, bla_x, Measure::App)
}

/// Convert with a given excitation and measurement mode. Resets the model first.
pub fn convert_measured(model: &mut Model, bla_x: &[f32], measure: Measure) -> Converted {
    assert_eq!(bla_x.len(), N, "the excitation is {N} samples");
    let fft = Fft::new();
    let clean_h = (measure == Measure::Clean).then(|| {
        model.reset();
        let x2 = [bla_x, bla_x].concat();
        let mut y2 = vec![0.0f32; 2 * N];
        model.process(&x2, &mut y2);
        DcBlocker::default().run(&mut y2);
        let (py, px) = (fft.power(&y2[N..]), fft.power(bla_x));
        py.iter().zip(&px).map(|(a, b)| a.sqrt() / b.sqrt()).collect::<Vec<f64>>()
    });
    model.reset();
    let mut dc = DcBlocker::default();

    // 1. best linear approximation (the run also sets the model and DC blocker state the tone
    //    test starts from, as in the app)
    let mut y = vec![0.0f32; N];
    model.process(bla_x, &mut y);
    dc.run(&mut y);
    let h: Vec<f64> = match clean_h {
        Some(h) => h,
        None => {
            let (py, px) = (fft.power(&y), fft.power(bla_x));
            py.iter().zip(&px).map(|(a, b)| a.sqrt() / b.sqrt()).collect()
        }
    };
    let h_db: Vec<f64> = h.iter().map(|v| 20.0 * v.log10()).collect();
    let hs = smooth(&h);

    // 2. tone test
    let (mut levels, mut levels3) = (Vec::with_capacity(94), Vec::with_capacity(THIRD_TONES));
    let mut x = vec![0.0f32; TONE_LEN];
    let mut out = vec![0.0f32; TONE_LEN];
    for (i, &b) in TONE_BINS.iter().enumerate() {
        let w = b as f32 * 6.283_185_5 * (1.0 / N as f32);
        for (n, v) in x.iter_mut().enumerate() {
            *v = (n as f32 * w).sin();
        }
        model.process(&x, &mut out);
        dc.run(&mut out);
        let p = fft.power(&out[TONE_SETTLE..]);
        levels.push(p[b].sqrt() * std::f64::consts::FRAC_PI_4 / 4096.0);
        if i < THIRD_TONES {
            levels3.push(p[3 * b].sqrt() * 3.0 * std::f64::consts::FRAC_PI_4 / 4096.0);
        }
    }

    // 3. gains
    let bins3: Vec<usize> = TONE_BINS[..THIRD_TONES].iter().map(|b| 3 * b).collect();
    let f = interp_linear(&levels, &TONE_BINS, levels[0]);
    let t = interp_linear(&levels3, &bins3, 0.0);
    let g1_gains_db: Vec<f64> = TONE_BINS.iter().map(|&b| 20.0 * (hs[b] / f[b].max(t[b])).log10()).collect();

    // 4. filters
    let g1_db = interp_log(&g1_gains_db, &TONE_BINS);
    let g2_db: Vec<f64> = h_db.iter().zip(&g1_db).map(|(a, b)| a - b).collect();
    let (g1_ir, g2_ir) = (fft.min_phase(&g1_db), fft.min_phase(&g2_db));
    let data = match measure {
        // the app's layout, byte for byte (the pedal ignores the first 512 floats of each block)
        Measure::App => g1_ir[..1024].iter().rev().chain(g2_ir[..1024].iter().rev()).map(|&v| v as f32).collect(),
        Measure::Clean => upload_data(&g1_ir, &g2_ir),
    };
    Converted { data, factor: 1.0, levels, levels3, bla_db: h_db, g1_gains_db, g1_ir, g2_ir }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_phase_reproduces_magnitude() {
        let fft = Fft::new();
        // a smooth tilt: +6 dB at DC falling to -6 dB at Nyquist
        let db: Vec<f64> = (0..HALF).map(|k| 6.0 - 12.0 * k as f64 / HALF as f64).collect();
        let ir = fft.min_phase(&db);
        let mut c: Vec<Complex64> = ir.iter().map(|&v| Complex64::new(v, 0.0)).collect();
        fft.fwd.process(&mut c);
        for k in [1, 10, 100, 1000, 4000] {
            assert!((20.0 * c[k].norm().log10() - db[k]).abs() < 0.01, "bin {k}");
        }
        // minimum phase: energy concentrated at the start
        let e: f64 = ir.iter().map(|v| v * v).sum();
        assert!(ir[..64].iter().map(|v| v * v).sum::<f64>() > 0.99 * e);
    }

    #[test]
    fn flat_curve_is_an_impulse() {
        let ir = Fft::new().min_phase(&vec![0.0; HALF]);
        assert!((ir[0] - 1.0).abs() < 1e-9 && ir[1..].iter().all(|v| v.abs() < 1e-9));
    }

    #[test]
    fn smoothing_keeps_a_constant() {
        let s = smooth(&vec![3.0; HALF]);
        assert!(s.iter().all(|v| (v - 3.0).abs() < 1e-9));
    }

    #[test]
    fn interpolation() {
        let bins = [10, 20, 40];
        let l = interp_linear(&[1.0, 2.0, 4.0], &bins, 0.5);
        assert_eq!((l[0], l[10], l[15], l[20], l[30], l[4000]), (0.5, 1.0, 1.5, 2.0, 3.0, 4.0));
        let g = interp_log(&[0.0, 6.0, 12.0], &bins);
        assert!((g[14] - 6.0 * (14f64 / 10.0).log10() / 2f64.log10()).abs() < 1e-12);
        assert_eq!((g[0], g[4095]), (0.0, 12.0));
    }

    #[test]
    fn upload_layout() {
        let g1: Vec<f64> = (0..2000).map(|k| k as f64 + 1.0).collect();
        let g2: Vec<f64> = (0..2000).map(|k| -(k as f64) - 1.0).collect();
        let d = upload_data(&g1, &g2);
        assert_eq!(d.len(), 2048);
        assert!(d[..512].iter().chain(&d[1024..1536]).all(|&v| v == 0.0));
        assert_eq!((d[1023], d[512], d[2047], d[1536]), (1.0, 512.0, -1.0, -512.0));
        let (h1, h2) = pedal_irs(&d);
        assert_eq!((h1.len(), h2.len()), (PEDAL_TAPS, PEDAL_TAPS));
        assert!(h1.iter().zip(&g1).chain(h2.iter().zip(&g2)).all(|(a, b)| a == b));
    }

    #[test]
    fn official_fingerprint_search() {
        // a synthetic "table" with the right prefix but wrong contents is rejected
        let mut fake = vec![0u8; 100];
        fake.extend_from_slice(&OFFICIAL_PREFIX);
        fake.resize(100 + N * 4 + 50, 7);
        assert!(find_official(&fake).is_none());
        assert!(find_official(&[1, 2, 3]).is_none());
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn excitation_spectrum() {
        let x = excitation();
        let fft = Fft::new();
        let p = fft.power(&x);
        for k in [10usize, 100, 1000, 4000] {
            let want = N as f64 / 2.0 * (1.0 / (k as f64 * 48000.0 / N as f64)).sqrt() * 1e-5;
            assert!((p[k].sqrt() / want - 1.0).abs() < 1e-3, "bin {k}");
        }
        let peak = x.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak > 1e-5 && peak < 1e-4, "about -90 dBFS: {peak}");
    }
}

//! Optimised conversion: fit the two filters so the pedal itself sounds like the model (docs/05).
//!
//! The official converter designs G1 and G2 from two measurements and never looks at what the
//! pedal does with them. This module simulates the pedal instead and adjusts G1/G2 until the
//! simulated output matches the model's on a test signal:
//!
//! * **The pedal** (fitted from re-amp recordings, accurate to the rig's own repeatability):
//!   `y = G2 ⊛ f(G1 ⊛ x)`, with both filters cut to [`PEDAL_TAPS`] and `f` the fixed saturation
//!   curve [`saturation`].
//! * **The target**: the model's output for the same signal (input −12 dB, DC-blocked, scaled to
//!   the pedal's output level).
//! * **The filters**: smooth log-magnitude curves at 41 control points (20 Hz × 2^(i/4)), each made
//!   minimum-phase, as the official converter's filters are.
//! * **The fit**: Adam on a multi-resolution STFT log-magnitude distance over sweeps, tones and pink
//!   noise at several levels, starting from the [`Measure::Clean`](super::convert::Measure::Clean)
//!   conversion. Gradients are written out by hand (no ML framework).
//!
//! A port of open-stomp-manager `tools/pedalsim` (PyTorch); `tests/data/optim` holds its reference
//! values.

// the gradients that use the rest come in the next step of the port
#![allow(dead_code)]

use std::sync::Arc;

use rustfft::num_complex::Complex64;
use rustfft::{Fft, FftPlanner};

use super::Model;
use super::convert::PEDAL_TAPS;

/// Sample rate of the test signal and the simulation.
pub const SR: usize = 48000;

// ---------------------------------------------------------------------------------------------
// the pedal

/// Knots of the saturation curve (input level, in test-signal units after G1).
const F_KNOTS: [f64; 17] =
    [-32.0, -16.0, -8.0, -4.0, -2.0, -1.0, -0.5, -0.25, 0.0, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0];
/// The curve at [`F_KNOTS`], fitted jointly to re-amp recordings of three different uploads. The
/// recordings barely reach beyond ±32, so the curve is held flat (saturated) there.
const F_VALUES: [f64; 17] = [
    -0.159341381678132,
    -0.1515523482954498,
    -0.12383923234960825,
    -0.07909273657985298,
    -0.04498259976139617,
    -0.02245628731142803,
    -0.011406034112795504,
    -0.005583136803729503,
    0.00018537827202383975,
    0.005986027774350994,
    0.01180106412522064,
    0.02312483036632483,
    0.045952939777459255,
    0.08342816833097066,
    0.13236822043215563,
    0.15836762403716023,
    0.16086942240926744,
];

/// Segment of the curve used for `u`: the knot interval `(k[i], k[i+1]]` holding it, or `None`
/// outside the knots (flat).
fn knot_interval(u: f64) -> Option<usize> {
    if u <= F_KNOTS[0] || u > F_KNOTS[F_KNOTS.len() - 1] {
        return None;
    }
    Some(F_KNOTS.partition_point(|&k| k < u) - 1)
}

/// The pedal's fixed saturation curve `f`, applied between G1 and G2 (piecewise linear).
pub fn saturation(u: f64) -> f64 {
    match knot_interval(u) {
        None if u <= F_KNOTS[0] => F_VALUES[0],
        None => F_VALUES[F_VALUES.len() - 1],
        Some(i) => {
            let t = (u - F_KNOTS[i]) / (F_KNOTS[i + 1] - F_KNOTS[i]);
            F_VALUES[i] * (1.0 - t) + F_VALUES[i + 1] * t
        }
    }
}

/// `f'(u)` (the slope of the segment holding `u`; 0 outside the knots).
fn saturation_slope(u: f64) -> f64 {
    knot_interval(u).map_or(0.0, |i| (F_VALUES[i + 1] - F_VALUES[i]) / (F_KNOTS[i + 1] - F_KNOTS[i]))
}

// ---------------------------------------------------------------------------------------------
// FFT helpers

fn plan(n: usize) -> (Arc<dyn Fft<f64>>, Arc<dyn Fft<f64>>) {
    let mut p = FftPlanner::new();
    (p.plan_fft_forward(n), p.plan_fft_inverse(n))
}

fn complex(x: &[f64], n: usize) -> Vec<Complex64> {
    let mut v: Vec<Complex64> = x.iter().take(n).map(|&r| Complex64::new(r, 0.0)).collect();
    v.resize(n, Complex64::default());
    v
}

/// Overlap-save FFT convolution of long signals with filters of up to [`PEDAL_TAPS`] taps, and
/// its two adjoints (for the gradients).
struct Conv {
    fwd: Arc<dyn Fft<f64>>,
    inv: Arc<dyn Fft<f64>>,
}

/// FFT block length of [`Conv`].
const BLOCK: usize = 8192;
/// Filter history each block carries.
const HIST: usize = PEDAL_TAPS - 1;
/// New output samples per block.
const HOP: usize = BLOCK - HIST;

impl Conv {
    fn new() -> Self {
        let (fwd, inv) = plan(BLOCK);
        Conv { fwd, inv }
    }

    fn blocks(len: usize) -> usize {
        len.div_ceil(HOP)
    }

    /// The spectrum of a filter (zero-padded to [`BLOCK`]).
    fn filter(&self, h: &[f64]) -> Vec<Complex64> {
        assert!(h.len() <= PEDAL_TAPS);
        let mut v = complex(h, BLOCK);
        self.fwd.process(&mut v);
        v
    }

    /// The block spectra of a signal: block `b` covers `x[b·HOP − HIST .. b·HOP − HIST + BLOCK]`
    /// (zero outside the signal).
    fn signal(&self, x: &[f64]) -> Vec<Vec<Complex64>> {
        (0..Self::blocks(x.len()))
            .map(|b| {
                let s = (b * HOP) as isize - HIST as isize;
                let mut v: Vec<Complex64> = (0..BLOCK as isize)
                    .map(|j| {
                        let i = s + j;
                        Complex64::new(if i >= 0 && (i as usize) < x.len() { x[i as usize] } else { 0.0 }, 0.0)
                    })
                    .collect();
                self.fwd.process(&mut v);
                v
            })
            .collect()
    }

    /// `y = h ⊛ x` (first `len` samples), from the block spectra of `x`.
    fn apply(&self, blocks: &[Vec<Complex64>], h: &[Complex64], len: usize) -> Vec<f64> {
        let mut y = vec![0.0; len];
        let mut buf = vec![Complex64::default(); BLOCK];
        for (b, xb) in blocks.iter().enumerate() {
            for ((o, a), c) in buf.iter_mut().zip(xb).zip(h) {
                *o = a * c;
            }
            self.inv.process(&mut buf);
            let s = b * HOP;
            for (j, o) in y[s..(s + HOP).min(len)].iter_mut().enumerate() {
                *o = buf[HIST + j].re / BLOCK as f64;
            }
        }
        y
    }
}

// ---------------------------------------------------------------------------------------------
// the test signal

/// A named part of the [`TestSignal`].
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub name: String,
    pub start: usize,
    pub len: usize,
    /// Part of the fit (the rest is only simulated).
    pub train: bool,
}

/// The signal the optimiser matches the pedal to the model on (all at [`SR`]).
#[derive(Clone, Debug)]
pub struct TestSignal {
    pub x: Vec<f64>,
    pub segments: Vec<Segment>,
}

/// xorshift64* with Box–Muller normals: a fixed, portable noise source.
struct Rng(u64);

impl Rng {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        ((self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    fn normal(&mut self) -> f64 {
        let (a, b) = (self.uniform(), self.uniform());
        (-2.0 * a.ln()).sqrt() * (std::f64::consts::TAU * b).cos()
    }
}

/// `n` samples of pink noise at unit rms: white noise with its spectrum divided by `√weight(k)`
/// (`weight(0)` is the DC bin).
fn pink(n: usize, seed: u64, weight: impl Fn(usize) -> Option<f64>) -> Vec<f64> {
    let mut rng = Rng(seed);
    let mut v: Vec<Complex64> = (0..n).map(|_| Complex64::new(rng.normal(), 0.0)).collect();
    let (fwd, inv) = plan(n);
    fwd.process(&mut v);
    for k in 0..=n / 2 {
        let g = weight(k).map_or(0.0, |w| 1.0 / w.sqrt());
        v[k] *= g;
        if k != 0 && k != n - k {
            v[n - k] *= g;
        }
    }
    inv.process(&mut v);
    let x: Vec<f64> = v.iter().map(|c| c.re).collect();
    let rms = (x.iter().map(|a| a * a).sum::<f64>() / n as f64).sqrt();
    x.iter().map(|a| a / rms).collect()
}

/// The test signal (about 53 s): the re-amp signal the pedal model was fitted on (log sweeps at
/// −36/−24/−12/−6 dBFS, bass tones at three levels, pink noise at −18 dBFS) followed by pink noise
/// at −36, −24 and −12 dBFS. The fit uses everything but the −18 dBFS pink noise.
pub fn test_signal() -> TestSignal {
    let mut x: Vec<f64> = Vec::new();
    let mut segments = Vec::new();
    let mut add = |name: String, mut s: Vec<f64>, gap: usize, fade: bool, train: bool, x: &mut Vec<f64>| {
        if fade {
            let n = 480;
            for i in 0..n {
                let g = i as f64 / (n - 1) as f64;
                s[i] *= g;
                let j = s.len() - n + i;
                s[j] *= 1.0 - g;
            }
            // played as float32
            s.iter_mut().for_each(|v| *v = *v as f32 as f64);
        }
        segments.push(Segment { name, start: x.len(), len: s.len(), train });
        x.extend_from_slice(&s);
        x.resize(x.len() + gap, 0.0);
    };
    let db = |d: f64| 10f64.powf(d / 20.0);

    // log sweeps, 20 Hz – 20 kHz in 4 s
    let (t_len, f1, f2): (f64, f64, f64) = (4.0, 20.0, 20000.0);
    let l = t_len / (f2 / f1).ln();
    let sweep: Vec<f64> = (0..(t_len * SR as f64) as usize)
        .map(|i| (std::f64::consts::TAU * f1 * l * ((i as f64 / SR as f64 / l).exp() - 1.0)).sin())
        .collect();
    for d in [-36.0, -24.0, -12.0, -6.0] {
        add(format!("sweep {d}"), sweep.iter().map(|v| v * db(d)).collect(), SR * 6 / 10, true, true, &mut x);
    }
    // 1 s tones
    for f in [41.2, 55.0, 73.4, 98.0, 196.0] {
        for d in [-24.0, -12.0, -6.0] {
            let tone = (0..SR).map(|i| (std::f64::consts::TAU * f * (i as f64 / SR as f64)).sin() * db(d)).collect();
            add(format!("tone {f:.1} {d}"), tone, SR * 3 / 10, true, true, &mut x);
        }
    }
    // pink noise (spectrum / √bin), not used in the fit
    let p = pink(3 * SR, 1, |k| Some(k.max(1) as f64));
    add("pink -18".into(), p.iter().map(|v| v * db(-18.0)).collect(), SR * 6 / 10, true, false, &mut x);
    // pink noise for the fit (spectrum / √f, nothing below 20 Hz), at three levels
    for (i, d) in [-30.0, -18.0, -6.0].into_iter().enumerate() {
        x.resize(x.len() + SR / 2, 0.0);
        let n = 3 * SR;
        let p = pink(n, 1000 + i as u64, |k| {
            let f = k as f64 * SR as f64 / n as f64;
            (f >= 20.0).then_some(f)
        });
        // × 0.5 (≈ −6 dB)
        add(format!("synth pink {}", d - 6.0), p.iter().map(|v| v * db(d) * 0.5).collect(), 0, false, true, &mut x);
    }
    x.resize(x.len() + SR / 2, 0.0);
    TestSignal { x, segments }
}

// ---------------------------------------------------------------------------------------------
// the target

/// Model input level relative to the test signal (dB) and the pedal's output level relative to
/// the model's, measured on the re-amp rig (an Ampeg SVT bass model; `nam_calibration`).
const TARGET_GAIN_IN_DB: f64 = -12.0;
const TARGET_GAIN_OUT: f64 = 0.09316022181625924;

/// What the pedal should output for `x`: the model's output (input −12 dB, from its reset state),
/// through the app's DC blocker, at the pedal's output level.
pub fn render_target(model: &mut Model, x: &[f64]) -> Vec<f64> {
    let gin = 10f64.powf(TARGET_GAIN_IN_DB / 20.0);
    let input: Vec<f32> = x.iter().map(|v| (v * gin) as f32).collect();
    let mut out = vec![0.0f32; x.len()];
    model.reset();
    model.process(&input, &mut out);
    // the DC blocker in f64
    const B: [f64; 3] = [0.9953827, -1.990765, 0.9953827];
    const A: [f64; 2] = [-1.990744, 0.9907867];
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    out.iter()
        .map(|&v| {
            let x0 = v as f64;
            let y0 = B[0] * x0 + B[1] * x1 + B[2] * x2 - A[0] * y1 - A[1] * y2;
            (x2, x1, y2, y1) = (x1, x0, y1, y0);
            y0 * TARGET_GAIN_OUT
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// the simulator

/// The pedal simulator for one input signal (its block spectra are computed once).
pub struct Simulator {
    conv: Conv,
    len: usize,
    x: Vec<Vec<Complex64>>,
}

/// One simulation, with what the gradients need.
struct Run {
    /// `G1 ⊛ x`, the input of the saturation.
    u: Vec<f64>,
    /// Block spectra of `f(u)`.
    v: Vec<Vec<Complex64>>,
    /// The output.
    y: Vec<f64>,
}

impl Simulator {
    pub fn new(x: &[f64]) -> Self {
        let conv = Conv::new();
        let blocks = conv.signal(x);
        Simulator { conv, len: x.len(), x: blocks }
    }

    fn run(&self, h1: &[f64], h2: &[f64]) -> Run {
        let u = self.conv.apply(&self.x, &self.conv.filter(h1), self.len);
        let fu: Vec<f64> = u.iter().map(|&a| saturation(a)).collect();
        let v = self.conv.signal(&fu);
        let y = self.conv.apply(&v, &self.conv.filter(h2), self.len);
        Run { u, v, y }
    }

    /// The pedal's output for impulse responses `h1` (G1) and `h2` (G2), up to [`PEDAL_TAPS`] each.
    pub fn output(&self, h1: &[f64], h2: &[f64]) -> Vec<f64> {
        self.run(h1, h2).y
    }
}

/// The pedal's output for `x` with user AMP `data` (2048 floats as uploaded).
pub fn simulate(data: &[f32], x: &[f64]) -> Vec<f64> {
    let (h1, h2) = super::convert::pedal_irs(data);
    Simulator::new(x).output(&h1, &h2)
}

// ---------------------------------------------------------------------------------------------
// the loss

/// STFT resolutions: (window length, hop).
const RESOLUTIONS: [(usize, usize); 3] = [(4096, 1024), (1024, 256), (256, 64)];
/// Samples left out at each end of a segment (fades, filter onsets).
const TRIM: usize = 2400;

struct Resolution {
    n: usize,
    hop: usize,
    window: Vec<f64>,
    fwd: Arc<dyn Fft<f64>>,
    inv: Arc<dyn Fft<f64>>,
}

impl Resolution {
    fn new(n: usize, hop: usize) -> Self {
        let window = (0..n).map(|j| 0.5 - 0.5 * (std::f64::consts::TAU * j as f64 / n as f64).cos()).collect();
        let (fwd, inv) = plan(n);
        Resolution { n, hop, window, fwd, inv }
    }

    fn frames(&self, len: usize) -> usize {
        1 + len / self.hop
    }

    /// Index into `x` of sample `i` of the centred, reflect-padded signal.
    fn source(&self, i: usize, len: usize) -> usize {
        let pad = self.n / 2;
        if i < pad {
            pad - i
        } else if i < pad + len {
            i - pad
        } else {
            2 * (len - 1) + pad - i
        }
    }

    /// `|STFT(x)|`, frame-major (`frames × (n/2 + 1)`), as `torch.stft(center=True,
    /// pad_mode="reflect")` with a periodic Hann window.
    fn magnitude(&self, x: &[f64]) -> Vec<f64> {
        let (len, bins) = (x.len(), self.n / 2 + 1);
        let mut out = Vec::with_capacity(self.frames(len) * bins);
        let mut buf = vec![Complex64::default(); self.n];
        for f in 0..self.frames(len) {
            for (j, b) in buf.iter_mut().enumerate() {
                *b = Complex64::new(self.window[j] * x[self.source(f * self.hop + j, len)], 0.0);
            }
            self.fwd.process(&mut buf);
            out.extend(buf[..bins].iter().map(|c| c.norm()));
        }
        out
    }
}

/// Multi-resolution STFT log-magnitude L1 distance to a target, over segments.
struct SpecLoss {
    res: Vec<Resolution>,
    /// `(start, end, floor, log |STFT(target)| per resolution)`; floor = 1e-3 · rms(target) · √n is
    /// added to every magnitude.
    parts: Vec<(usize, usize, f64, Vec<Vec<f64>>)>,
}

impl SpecLoss {
    fn new(target: &[f64], slices: &[(usize, usize)]) -> Self {
        let res: Vec<Resolution> = RESOLUTIONS.iter().map(|&(n, h)| Resolution::new(n, h)).collect();
        let parts = slices
            .iter()
            .map(|&(a, b)| {
                let t = &target[a..b];
                let eps = 1e-3 * (t.iter().map(|v| v * v).sum::<f64>() / t.len() as f64).sqrt();
                let logs = res
                    .iter()
                    .map(|r| {
                        let e = eps * (r.n as f64).sqrt();
                        r.magnitude(t).iter().map(|m| (m + e).ln()).collect()
                    })
                    .collect();
                (a, b, eps, logs)
            })
            .collect();
        SpecLoss { res, parts }
    }

    /// The loss of `y` (mean over segments and resolutions of the mean absolute log difference).
    fn value(&self, y: &[f64]) -> f64 {
        let mut tot = 0.0;
        for (a, b, eps, logs) in &self.parts {
            for (r, t) in self.res.iter().zip(logs) {
                let e = eps * (r.n as f64).sqrt();
                let m = r.magnitude(&y[*a..*b]);
                tot += m.iter().zip(t).map(|(m, t)| ((m + e).ln() - t).abs()).sum::<f64>() / m.len() as f64;
            }
        }
        tot / (self.parts.len() * self.res.len()) as f64
    }
}

/// The part of each training segment the loss looks at.
fn train_slices(segments: &[Segment]) -> Vec<(usize, usize)> {
    segments.iter().filter(|s| s.train).map(|s| (s.start + TRIM, s.start + s.len - TRIM)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden(name: &str) -> Vec<f64> {
        let p = format!("{}/tests/data/optim/{name}.bin", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&p)
            .unwrap_or_else(|e| panic!("{p}: {e}"))
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    fn manifest() -> serde_json::Value {
        let p = format!("{}/tests/data/optim/golden.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    /// Max |a − b| relative to max |b|.
    fn rel_err(a: &[f64], b: &[f64]) -> f64 {
        assert_eq!(a.len(), b.len());
        let scale = b.iter().fold(0f64, |m, v| m.max(v.abs()));
        a.iter().zip(b).fold(0f64, |m, (x, y)| m.max((x - y).abs())) / scale
    }

    #[test]
    fn saturation_matches_reference() {
        let (u, v) = (golden("f_u"), golden("f_v"));
        let ours: Vec<f64> = u.iter().map(|&a| saturation(a)).collect();
        assert!(rel_err(&ours, &v) < 1e-12);
    }

    #[test]
    fn simulator_matches_reference() {
        let x = golden("sim_x");
        let run = Simulator::new(&x).run(&golden("sim_h1"), &golden("sim_h2"));
        assert!(rel_err(&run.u, &golden("sim_u")) < 1e-10, "u {}", rel_err(&run.u, &golden("sim_u")));
        assert!(rel_err(&run.y, &golden("sim_y")) < 1e-10, "y {}", rel_err(&run.y, &golden("sim_y")));
    }

    #[test]
    fn loss_matches_reference() {
        let m = manifest();
        let slices: Vec<(usize, usize)> = m["sim_segs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                let (a, l) = (s[0].as_u64().unwrap() as usize, s[1].as_u64().unwrap() as usize);
                (a + TRIM, a + l - TRIM)
            })
            .collect();
        let loss = SpecLoss::new(&golden("sim_tgt"), &slices);
        let v = loss.value(&golden("sim_y"));
        let want = m["loss"].as_f64().unwrap();
        assert!((v - want).abs() < 1e-10 * want, "{v} vs {want}");
    }

    #[test]
    fn target_matches_reference() {
        let mut model = Model::load(format!("{}/tests/data/nam/wavenet.nam", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let y = render_target(&mut model, &golden("tgt_x"));
        assert!(rel_err(&y, &golden("tgt_y")) < 1e-5, "{}", rel_err(&y, &golden("tgt_y")));
    }

    #[test]
    fn test_signal_matches_reference() {
        let m = manifest();
        let s = test_signal();
        let segs = m["sig_segs"].as_array().unwrap();
        for (ours, theirs) in s.segments.iter().zip(segs) {
            assert_eq!(ours.name, theirs["name"].as_str().unwrap());
            assert_eq!((ours.start, ours.len), (theirs["start"].as_u64().unwrap() as usize, theirs["len"].as_u64().unwrap() as usize));
        }
        // the re-amp part, then three 3.5 s pink segments and 0.5 s of silence
        let re_amp = m["sig_len"].as_u64().unwrap() as usize;
        assert_eq!(s.x.len(), re_amp + 3 * (SR / 2 + 3 * SR) + SR / 2);
        let (idx, v) = (golden("sig_idx"), golden("sig_v"));
        for (&i, &want) in idx.iter().zip(&v) {
            assert!((s.x[i as usize] - want).abs() < 1e-6, "sample {i}: {} vs {want}", s.x[i as usize]);
        }
        assert_eq!(s.segments.iter().filter(|s| s.train).count(), 22);
        // pink noise at the stated level
        let p = s.segments.iter().find(|s| s.name == "synth pink -12").unwrap();
        let rms = (s.x[p.start..p.start + p.len].iter().map(|v| v * v).sum::<f64>() / p.len as f64).sqrt();
        assert!((rms - 0.5 * 10f64.powf(-6.0 / 20.0)).abs() < 1e-12);
    }
}

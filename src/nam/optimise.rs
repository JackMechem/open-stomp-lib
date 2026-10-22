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
        par_chunks(Self::blocks(x.len()), |blocks| {
            blocks
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
                .collect::<Vec<_>>()
        })
        .concat()
    }

    /// `y = h ⊛ x` (first `len` samples), from the block spectra of `x`.
    fn apply(&self, blocks: &[Vec<Complex64>], h: &[Complex64], len: usize) -> Vec<f64> {
        let mut y = par_chunks(blocks.len(), |range| {
            let mut out = Vec::with_capacity(range.len() * HOP);
            let mut buf = vec![Complex64::default(); BLOCK];
            for xb in &blocks[range] {
                for ((o, a), c) in buf.iter_mut().zip(xb).zip(h) {
                    *o = a * c;
                }
                self.inv.process(&mut buf);
                out.extend(buf[HIST..].iter().map(|v| v.re / BLOCK as f64));
            }
            out
        })
        .concat();
        y.truncate(len);
        y
    }

    /// The adjoint of [`Conv::apply`] for the signal: `gx[m] = Σ_k h[k]·g[m + k]`.
    fn correlate(&self, g: &[f64], h: &[Complex64]) -> Vec<f64> {
        let mut gx = par_chunks(Self::blocks(g.len()), |range| {
            let mut out = Vec::with_capacity(range.len() * HOP);
            let mut buf = vec![Complex64::default(); BLOCK];
            for b in range {
                let s = b * HOP;
                for (j, o) in buf.iter_mut().enumerate() {
                    *o = Complex64::new(g.get(s + j).copied().unwrap_or(0.0), 0.0);
                }
                self.fwd.process(&mut buf);
                for (o, c) in buf.iter_mut().zip(h) {
                    *o *= c.conj();
                }
                self.inv.process(&mut buf);
                out.extend(buf[..HOP].iter().map(|v| v.re / BLOCK as f64));
            }
            out
        })
        .concat();
        gx.truncate(g.len());
        gx
    }

    /// The adjoint of [`Conv::apply`] for the filter: `gh[k] = Σ_n g[n]·x[n − k]`, `k <`
    /// [`PEDAL_TAPS`], from the block spectra of `x`.
    fn filter_grad(&self, blocks: &[Vec<Complex64>], g: &[f64]) -> Vec<f64> {
        let parts = par_chunks(blocks.len(), |range| {
            let mut acc = vec![Complex64::default(); BLOCK];
            let mut buf = vec![Complex64::default(); BLOCK];
            for b in range {
                let s = b * HOP;
                buf.fill(Complex64::default());
                for (j, &v) in g[s..(s + HOP).min(g.len())].iter().enumerate() {
                    buf[HIST + j].re = v;
                }
                self.fwd.process(&mut buf);
                for ((a, gb), xc) in acc.iter_mut().zip(&buf).zip(&blocks[b]) {
                    *a += gb * xc.conj();
                }
            }
            acc
        });
        let mut acc = vec![Complex64::default(); BLOCK];
        for p in parts {
            acc.iter_mut().zip(p).for_each(|(a, v)| *a += v);
        }
        self.inv.process(&mut acc);
        acc[..PEDAL_TAPS].iter().map(|c| c.re / BLOCK as f64).collect()
    }
}

/// `f` on contiguous ranges covering `0..n`, one per thread, results in order.
fn par_chunks<T: Send>(n: usize, f: impl Fn(std::ops::Range<usize>) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get()).clamp(1, 16).min(n.max(1));
    let per = n.div_ceil(threads);
    if threads == 1 {
        return vec![f(0..n)];
    }
    std::thread::scope(|s| {
        let f = &f;
        let handles: Vec<_> =
            (0..threads).map(|t| s.spawn(move || f((t * per).min(n)..((t + 1) * per).min(n)))).collect();
        handles.into_iter().map(|h| h.join().expect("optimiser thread")).collect()
    })
}

/// `f(i)` for `i` in `0..n` on all threads (work-stealing), results in order.
fn par_map<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get()).clamp(1, 16).min(n.max(1));
    let mut done: Vec<(usize, T)> = std::thread::scope(|s| {
        let (f, next) = (&f, &next);
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(move || {
                    let mut v = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= n {
                            return v;
                        }
                        v.push((i, f(i)));
                    }
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("optimiser thread")).collect()
    });
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, t)| t).collect()
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

    /// Gradients w.r.t. `h1` and `h2` of `Σ gy·y + PENALTY(u)` for a run.
    fn backward(&self, run: &Run, h2: &[f64], gy: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let gh2 = self.conv.filter_grad(&run.v, gy);
        let gv = self.conv.correlate(gy, &self.conv.filter(h2));
        let gu: Vec<f64> = gv.iter().zip(&run.u).map(|(g, &u)| g * saturation_slope(u) + penalty_grad(u, self.len)).collect();
        let gh1 = self.conv.filter_grad(&self.x, &gu);
        (gh1, gh2)
    }

    /// The pedal's output for impulse responses `h1` (G1) and `h2` (G2), up to [`PEDAL_TAPS`] each.
    pub fn output(&self, h1: &[f64], h2: &[f64]) -> Vec<f64> {
        self.run(h1, h2).y
    }
}

/// Keeps the input of the saturation inside the range the curve was fitted on:
/// `1e-3 · mean(max(|u| − 100, 0)²)`.
const PENALTY_LIMIT: f64 = 100.0;
const PENALTY_WEIGHT: f64 = 1e-3;

fn penalty(u: &[f64]) -> f64 {
    PENALTY_WEIGHT * u.iter().map(|v| (v.abs() - PENALTY_LIMIT).max(0.0).powi(2)).sum::<f64>() / u.len() as f64
}

fn penalty_grad(u: f64, len: usize) -> f64 {
    PENALTY_WEIGHT * 2.0 * (u.abs() - PENALTY_LIMIT).max(0.0) * u.signum() / len as f64
}

/// The pedal's output for `x` with user AMP `data` (2048 floats as uploaded).
pub fn simulate(data: &[f32], x: &[f64]) -> Vec<f64> {
    let (h1, h2) = super::convert::pedal_irs(data);
    Simulator::new(x).output(&h1, &h2)
}

// ---------------------------------------------------------------------------------------------
// the filters: smooth curves → minimum-phase responses

/// Control points of the filter curves: 41, a quarter octave apart from 20 Hz (to 20.48 kHz).
pub const CURVE_POINTS: usize = 41;
/// FFT length of the minimum-phase design (as the converter's).
const MP_N: usize = 8192;
const MP_BINS: usize = MP_N / 2 + 1;

/// Control-point frequencies, as log10(Hz).
fn control_points() -> [f64; CURVE_POINTS] {
    std::array::from_fn(|i| 20f64.log10() + i as f64 * 2f64.log10() / 4.0)
}

/// log10 of each design bin's frequency (bin 0 counts as 1 mHz).
fn bin_log_freqs() -> Vec<f64> {
    (0..MP_BINS).map(|k| (k as f64 * SR as f64 / MP_N as f64).max(1e-3).log10()).collect()
}

/// Curves (natural-log magnitude at the control points) → minimum-phase impulse responses, and back.
struct Curves {
    /// Per design bin: the lower control point and the weight of the upper one (linear in
    /// log-frequency, held outside the control points).
    interp: Vec<(usize, f64)>,
    fwd: Arc<dyn Fft<f64>>,
    inv: Arc<dyn Fft<f64>>,
}

/// One design, with what the gradient needs.
struct Design {
    /// `exp(C)`, the minimum-phase spectrum (bins 0..=N/2).
    e: Vec<Complex64>,
    h: Vec<f64>,
}

impl Curves {
    fn new() -> Self {
        let cp = control_points();
        let interp = bin_log_freqs()
            .iter()
            .map(|&lf| {
                let i = (cp.partition_point(|&c| c < lf).max(1) - 1).min(CURVE_POINTS - 2);
                (i, ((lf - cp[i]) / (cp[i + 1] - cp[i])).clamp(0.0, 1.0))
            })
            .collect();
        let (fwd, inv) = plan(MP_N);
        Curves { interp, fwd, inv }
    }

    /// Fold doubling the causal part of a real cepstrum.
    fn fold(n: usize) -> f64 {
        match n {
            0 => 1.0,
            n if n < MP_N / 2 => 2.0,
            n if n == MP_N / 2 => 1.0,
            _ => 0.0,
        }
    }

    /// Curve → [`PEDAL_TAPS`]-tap minimum-phase impulse response: the log magnitude interpolated
    /// over the design bins, folded cepstrum, `exp`, first taps.
    fn design(&self, curve: &[f64]) -> Design {
        let l: Vec<f64> = self.interp.iter().map(|&(i, t)| curve[i] * (1.0 - t) + curve[i + 1] * t).collect();
        // real cepstrum of the (even) log magnitude, folded onto positive quefrencies
        let mut c: Vec<Complex64> = (0..MP_N).map(|k| Complex64::new(l[k.min(MP_N - k)], 0.0)).collect();
        self.inv.process(&mut c);
        for (n, v) in c.iter_mut().enumerate() {
            *v = Complex64::new(v.re / MP_N as f64 * Self::fold(n), 0.0);
        }
        self.fwd.process(&mut c);
        let e: Vec<Complex64> = c[..MP_BINS].iter().map(|v| v.exp()).collect();
        // real inverse transform of the half spectrum (the imaginary parts at DC and Nyquist are dropped)
        let mut z = vec![Complex64::default(); MP_N];
        z[0] = Complex64::new(e[0].re, 0.0);
        z[MP_N / 2] = Complex64::new(e[MP_N / 2].re, 0.0);
        for k in 1..MP_N / 2 {
            z[k] = e[k];
            z[MP_N - k] = e[k].conj();
        }
        self.inv.process(&mut z);
        let h = z[..PEDAL_TAPS].iter().map(|v| v.re / MP_N as f64).collect();
        Design { e, h }
    }

    /// Gradient w.r.t. the curve, given the gradient `gh` w.r.t. the response of `design`.
    fn backward(&self, design: &Design, gh: &[f64]) -> Vec<f64> {
        let n = MP_N as f64;
        // through h = irfft(E)
        let mut g = complex(gh, MP_N);
        self.fwd.process(&mut g);
        let mut z = vec![Complex64::default(); MP_N];
        for k in 0..MP_BINS {
            let ge = if k == 0 || k == MP_N / 2 { Complex64::new(g[k].re / n, 0.0) } else { g[k] * (2.0 / n) };
            // through E = exp(C)
            z[k] = ge * design.e[k].conj();
        }
        // through C = rfft(folded cepstrum), the fold, and the cepstrum = irfft(l)
        self.inv.process(&mut z);
        let mut gc: Vec<Complex64> = z.iter().enumerate().map(|(i, v)| Complex64::new(v.re * Self::fold(i), 0.0)).collect();
        self.fwd.process(&mut gc);
        let mut out = vec![0.0; CURVE_POINTS];
        for (k, &(i, t)) in self.interp.iter().enumerate() {
            let gl = gc[k].re * if k == 0 || k == MP_N / 2 { 1.0 } else { 2.0 } / n;
            out[i] += gl * (1.0 - t);
            out[i + 1] += gl * t;
        }
        out
    }

    /// The curve of an existing response: the mean natural-log magnitude within ±1/16 decade of
    /// each control point.
    fn of(&self, h: &[f64]) -> Vec<f64> {
        let mut s = complex(h, MP_N);
        self.fwd.process(&mut s);
        let lf = bin_log_freqs();
        control_points()
            .iter()
            .map(|&c| {
                let (sum, n) = (0..MP_BINS)
                    .filter(|&k| lf[k] >= c - 0.0625 && lf[k] < c + 0.0625)
                    .fold((0.0, 0), |(a, n), k| (a + (s[k].norm() + 1e-9).ln(), n + 1));
                sum / n as f64
            })
            .collect()
    }
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

    /// The STFT (bins 0..=n/2), frame-major (`frames × (n/2 + 1)`).
    fn stft(&self, x: &[f64]) -> Vec<Complex64> {
        let (len, bins) = (x.len(), self.n / 2 + 1);
        let mut out = Vec::with_capacity(self.frames(len) * bins);
        let mut buf = vec![Complex64::default(); self.n];
        for f in 0..self.frames(len) {
            for (j, b) in buf.iter_mut().enumerate() {
                *b = Complex64::new(self.window[j] * x[self.source(f * self.hop + j, len)], 0.0);
            }
            self.fwd.process(&mut buf);
            out.extend_from_slice(&buf[..bins]);
        }
        out
    }

    /// The adjoint of [`Resolution::stft`]: adds to `gx` the gradient for STFT gradients `gs`.
    fn stft_adjoint(&self, gs: &[Complex64], gx: &mut [f64]) {
        let (len, bins) = (gx.len(), self.n / 2 + 1);
        let mut buf = vec![Complex64::default(); self.n];
        for (f, g) in gs.chunks_exact(bins).enumerate() {
            buf.fill(Complex64::default());
            buf[..bins].copy_from_slice(g);
            self.inv.process(&mut buf);
            for (j, b) in buf.iter().enumerate() {
                gx[self.source(f * self.hop + j, len)] += self.window[j] * b.re;
            }
        }
    }

    /// `|STFT(x)|`, frame-major (`frames × (n/2 + 1)`), as `torch.stft(center=True,
    /// pad_mode="reflect")` with a periodic Hann window.
    fn magnitude(&self, x: &[f64]) -> Vec<f64> {
        self.stft(x).iter().map(|c| c.norm()).collect()
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

    /// The loss of `y` and its gradient (added to `gy`).
    fn value_grad(&self, y: &[f64], gy: &mut [f64]) -> f64 {
        let scale = 1.0 / (self.parts.len() * self.res.len()) as f64;
        // one task per segment and resolution, biggest first
        let tasks: Vec<(usize, usize)> = (0..self.parts.len()).flat_map(|p| (0..self.res.len()).map(move |r| (p, r))).collect();
        let results = par_map(tasks.len(), |i| {
            let (p, r) = tasks[i];
            let ((a, b, eps, logs), res) = (&self.parts[p], &self.res[r]);
            let e = eps * (res.n as f64).sqrt();
            let mut s = res.stft(&y[*a..*b]);
            let k = scale / s.len() as f64;
            let mut sum = 0.0;
            for (c, t) in s.iter_mut().zip(&logs[r]) {
                let m = c.norm();
                let d = (m + e).ln() - t;
                sum += d.abs();
                // d|d|/dS = sign(d) / (|S| + e) · S / |S|
                *c = if m > 0.0 { *c * (k * d.signum() / ((m + e) * m)) } else { Complex64::default() };
            }
            let mut g = vec![0.0; b - a];
            res.stft_adjoint(&s, &mut g);
            (sum / s.len() as f64, g)
        });
        let mut tot = 0.0;
        for ((p, _), (v, g)) in tasks.iter().zip(results) {
            tot += v;
            let a = self.parts[*p].0;
            gy[a..a + g.len()].iter_mut().zip(g).for_each(|(o, v)| *o += v);
        }
        tot * scale
    }

    #[cfg(test)]
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

// ---------------------------------------------------------------------------------------------
// the fit

/// Optimiser settings (the defaults are the ones validated on hardware).
#[derive(Clone, Debug)]
pub struct Options {
    /// Adam steps.
    pub steps: usize,
    /// Peak learning rate (in natural-log magnitude per step), reached after `warmup` steps and
    /// then decayed to 0 along a half cosine.
    pub learning_rate: f64,
    pub warmup: usize,
    /// Weight of the curve smoothness term (mean squared second difference of each curve).
    pub smoothness: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options { steps: 300, learning_rate: 0.05, warmup: 10, smoothness: 1e-3 }
    }
}

/// The result of [`optimise`].
#[derive(Clone, Debug)]
pub struct Optimised {
    /// The 2048 floats to upload ([`upload_data`](super::convert::upload_data) of the responses).
    pub data: Vec<f32>,
    /// G1 and G2 as the pedal runs them ([`PEDAL_TAPS`] taps).
    pub g1_ir: Vec<f64>,
    pub g2_ir: Vec<f64>,
    /// The two curves: natural-log magnitude at the [`CURVE_POINTS`] control points.
    pub g1_curve: Vec<f64>,
    pub g2_curve: Vec<f64>,
    /// The objective at the start and per step; the result is the lowest one seen.
    pub initial_loss: f64,
    pub loss: Vec<f64>,
}

/// Something for G1 and G2.
type Pair = [Vec<f64>; 2];

/// A fitting problem: a signal, the target output for it and the parts to compare.
struct Problem {
    sim: Simulator,
    loss: SpecLoss,
    curves: Curves,
}

/// One evaluation of the objective.
struct Eval {
    value: f64,
    grads: [Vec<f64>; 2],
    irs: [Vec<f64>; 2],
}

impl Problem {
    fn new(x: &[f64], target: &[f64], slices: &[(usize, usize)]) -> Self {
        Problem { sim: Simulator::new(x), loss: SpecLoss::new(target, slices), curves: Curves::new() }
    }

    /// The objective for two curves, and its gradient w.r.t. them.
    fn evaluate(&self, c: &[Vec<f64>; 2], smoothness: f64) -> Eval {
        let d = [self.curves.design(&c[0]), self.curves.design(&c[1])];
        let run = self.sim.run(&d[0].h, &d[1].h);
        let mut gy = vec![0.0; run.y.len()];
        let mut value = self.loss.value_grad(&run.y, &mut gy) + penalty(&run.u);
        let (gh1, gh2) = self.sim.backward(&run, &d[1].h, &gy);
        let mut grads = [self.curves.backward(&d[0], &gh1), self.curves.backward(&d[1], &gh2)];
        for (cv, g) in c.iter().zip(&mut grads) {
            let n = (cv.len() - 2) as f64;
            for i in 0..cv.len() - 2 {
                let dd = cv[i] - 2.0 * cv[i + 1] + cv[i + 2];
                value += smoothness * dd * dd / n;
                let k = smoothness * 2.0 * dd / n;
                g[i] += k;
                g[i + 1] -= 2.0 * k;
                g[i + 2] += k;
            }
        }
        let [Design { h: h1, .. }, Design { h: h2, .. }] = d;
        Eval { value, grads, irs: [h1, h2] }
    }

    /// Adam from curves `c`; returns the best curves and responses seen, and the objective per step.
    fn fit(&self, mut c: Pair, o: &Options) -> (Pair, Pair, Vec<f64>) {
        let (b1, b2, eps) = (0.9, 0.999, 1e-8);
        let mut m = [vec![0.0; CURVE_POINTS], vec![0.0; CURVE_POINTS]];
        let mut v = m.clone();
        let mut history = Vec::with_capacity(o.steps);
        let mut best: Option<(f64, Pair, Pair)> = None;
        for it in 0..o.steps {
            let lr = o.learning_rate
                * ((it + 1) as f64 / o.warmup.max(1) as f64).min(1.0)
                * 0.5
                * (1.0 + (std::f64::consts::PI * it as f64 / o.steps as f64).cos());
            let e = self.evaluate(&c, o.smoothness);
            history.push(e.value);
            if best.as_ref().is_none_or(|b| e.value < b.0) {
                best = Some((e.value, c.clone(), e.irs));
            }
            let t = (it + 1) as i32;
            let (bc1, bc2) = (1.0 - f64::powi(b1, t), 1.0 - f64::powi(b2, t));
            for j in 0..2 {
                for i in 0..CURVE_POINTS {
                    let g = e.grads[j][i];
                    m[j][i] = b1 * m[j][i] + (1.0 - b1) * g;
                    v[j][i] = b2 * v[j][i] + (1.0 - b2) * g * g;
                    c[j][i] -= lr / bc1 * m[j][i] / ((v[j][i] / bc2).sqrt() + eps);
                }
            }
        }
        let (_, bc, birs) = best.unwrap_or_else(|| {
            let e = self.evaluate(&c, o.smoothness);
            (e.value, c.clone(), e.irs)
        });
        (bc, birs, history)
    }
}

/// Fit G1/G2 so the simulated pedal matches `model`, starting from the responses in `init` (2048
/// floats of upload data, normally a [`Measure::Clean`](super::convert::Measure::Clean)
/// conversion). Runs the model over the test signal once, then `options.steps` simulations; about
/// a minute per 300 steps in an optimised build.
pub fn optimise(model: &mut Model, init: &[f32], options: &Options) -> Optimised {
    let signal = test_signal();
    let target = render_target(model, &signal.x);
    let problem = Problem::new(&signal.x, &target, &train_slices(&signal.segments));
    let (h1, h2) = super::convert::pedal_irs(init);
    let c0 = [problem.curves.of(&h1), problem.curves.of(&h2)];
    let initial_loss = problem.evaluate(&c0, options.smoothness).value;
    let ([g1_curve, g2_curve], [g1_ir, g2_ir], loss) = problem.fit(c0, options);
    Optimised { data: super::convert::upload_data(&g1_ir, &g2_ir), g1_ir, g2_ir, g1_curve, g2_curve, initial_loss, loss }
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

    fn golden_slices() -> Vec<(usize, usize)> {
        manifest()["sim_segs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                let (a, l) = (s[0].as_u64().unwrap() as usize, s[1].as_u64().unwrap() as usize);
                (a + TRIM, a + l - TRIM)
            })
            .collect()
    }

    #[test]
    fn curve_of_matches_reference() {
        let c = Curves::new();
        for (h, want) in [("co_h1", "co_c1"), ("co_h2", "co_c2")] {
            let ours = c.of(&golden(h));
            assert!(rel_err(&ours, &golden(want)) < 1e-10, "{h}: {}", rel_err(&ours, &golden(want)));
        }
    }

    #[test]
    fn min_phase_design_and_gradient_match_reference() {
        let c = Curves::new();
        let d = c.design(&golden("mp_c"));
        assert!(rel_err(&d.h, &golden("mp_h")) < 1e-10, "h {}", rel_err(&d.h, &golden("mp_h")));
        let g = c.backward(&d, &golden("mp_gh"));
        assert!(rel_err(&g, &golden("mp_gc")) < 1e-9, "grad {}", rel_err(&g, &golden("mp_gc")));
    }

    #[test]
    fn min_phase_gradient_is_numerically_right() {
        let c = Curves::new();
        let curve: Vec<f64> = (0..CURVE_POINTS).map(|i| (i as f64 * 0.7).sin() - 2.0).collect();
        let gh: Vec<f64> = (0..PEDAL_TAPS).map(|k| ((k * 37 % 101) as f64 / 50.0 - 1.0) / (1.0 + k as f64 / 20.0)).collect();
        let f = |cv: &[f64]| c.design(cv).h.iter().zip(&gh).map(|(a, b)| a * b).sum::<f64>();
        let g = c.backward(&c.design(&curve), &gh);
        for i in [0, 1, 7, 20, 33, 40] {
            let (mut p, mut m) = (curve.clone(), curve.clone());
            p[i] += 1e-6;
            m[i] -= 1e-6;
            let num = (f(&p) - f(&m)) / 2e-6;
            assert!((num - g[i]).abs() < 1e-6 * g.iter().fold(0f64, |a, v| a.max(v.abs())), "point {i}: {num} vs {}", g[i]);
        }
    }

    #[test]
    fn simulator_gradients_match_reference() {
        let sim = Simulator::new(&golden("sim_x"));
        let (h1, h2) = (golden("sim_h1"), golden("sim_h2"));
        let run = sim.run(&h1, &h2);
        let pen = penalty(&run.u);
        let want = manifest()["sim_pen"].as_f64().unwrap();
        assert!((pen - want).abs() < 1e-10 * want, "penalty {pen} vs {want}");
        let (g1, g2) = sim.backward(&run, &h2, &golden("sim_gy"));
        assert!(rel_err(&g1, &golden("sim_gh1")) < 1e-9, "h1 {}", rel_err(&g1, &golden("sim_gh1")));
        assert!(rel_err(&g2, &golden("sim_gh2")) < 1e-9, "h2 {}", rel_err(&g2, &golden("sim_gh2")));
    }

    #[test]
    fn simulator_gradients_are_numerically_right() {
        // a signal that reaches the penalty range and several segments of the curve
        let x: Vec<f64> = (0..30000).map(|n| 300.0 * (n as f64 * 0.013).sin() * (n as f64 / 30000.0)).collect();
        let h1: Vec<f64> = (0..PEDAL_TAPS).map(|k| 0.5 * (-(k as f64) / 30.0).exp() * (k as f64 * 0.3).cos()).collect();
        let h2: Vec<f64> = (0..PEDAL_TAPS).map(|k| (-(k as f64) / 50.0).exp() * (k as f64 * 0.1).sin()).collect();
        let gy: Vec<f64> = (0..x.len()).map(|n| (n * 7919 % 1000) as f64 / 500.0 - 1.0).collect();
        let sim = Simulator::new(&x);
        let obj = |a: &[f64], b: &[f64]| {
            let r = sim.run(a, b);
            r.y.iter().zip(&gy).map(|(y, g)| y * g).sum::<f64>() + penalty(&r.u)
        };
        let run = sim.run(&h1, &h2);
        assert!(run.u.iter().any(|u| u.abs() > PENALTY_LIMIT));
        let (g1, g2) = sim.backward(&run, &h2, &gy);
        let d = 1e-7;
        for k in [0, 3, 100, 511] {
            let (mut p, mut m) = (h1.clone(), h1.clone());
            p[k] += d;
            m[k] -= d;
            let num = (obj(&p, &h2) - obj(&m, &h2)) / (2.0 * d);
            assert!((num - g1[k]).abs() < 1e-4 * num.abs().max(1.0), "h1[{k}]: {num} vs {}", g1[k]);
            let (mut p, mut m) = (h2.clone(), h2.clone());
            p[k] += d;
            m[k] -= d;
            let num = (obj(&h1, &p) - obj(&h1, &m)) / (2.0 * d);
            assert!((num - g2[k]).abs() < 1e-4 * num.abs().max(1.0), "h2[{k}]: {num} vs {}", g2[k]);
        }
    }

    #[test]
    fn loss_gradient_matches_reference() {
        let loss = SpecLoss::new(&golden("sim_tgt"), &golden_slices());
        let y = golden("sim_y");
        let mut gy = vec![0.0; y.len()];
        let v = loss.value_grad(&y, &mut gy);
        assert!((v - loss.value(&y)).abs() < 1e-12);
        assert!(rel_err(&gy, &golden("loss_gy")) < 1e-9, "{}", rel_err(&gy, &golden("loss_gy")));
    }

    #[test]
    fn loss_gradient_is_numerically_right() {
        let slices = golden_slices();
        let loss = SpecLoss::new(&golden("sim_tgt"), &slices);
        let y = golden("sim_y");
        let mut gy = vec![0.0; y.len()];
        loss.value_grad(&y, &mut gy);
        // inside, at the edges (reflect padding) and outside the slices
        for i in [slices[0].0, slices[0].0 + 1, slices[0].0 + 1000, slices[0].1 - 1, slices[1].0 + 3, 100] {
            let d = 1e-6 * y[i].abs().max(1e-3);
            let (mut p, mut m) = (y.clone(), y.clone());
            p[i] += d;
            m[i] -= d;
            let num = (loss.value(&p) - loss.value(&m)) / (2.0 * d);
            assert!((num - gy[i]).abs() < 1e-4 * num.abs().max(1e-6), "sample {i}: {num} vs {}", gy[i]);
        }
    }

    #[test]
    fn optimiser_steps_match_reference() {
        let problem = Problem::new(&golden("sim_x"), &golden("sim_tgt"), &golden_slices());
        let c0 = golden("opt_c0");
        let want = golden("opt_loss");
        let o = Options { steps: want.len(), ..Options::default() };
        let c = [c0[..CURVE_POINTS].to_vec(), c0[CURVE_POINTS..].to_vec()];
        // each loss after the first follows an Adam step, so this checks the updates too
        let (_, _, history) = problem.fit(c, &o);
        assert!(rel_err(&history, &want) < 1e-9, "{history:?} vs {want:?}");
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

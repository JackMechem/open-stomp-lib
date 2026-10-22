//! Compare IR conversions as the pedal plays them (the first 384 taps of the upload) with the full
//! IR: the official app's (`ir::convert`) and the optimised one (`ir::convert_optimised`).
//!
//! ```sh
//! cargo run --release --example ir_compare -- <dir or .wav files>
//! ```
//! Shape error: rms of the 1/3-octave difference over 31.5 Hz – 16 kHz with the level difference
//! removed; level: that difference (dB, against the full IR at the official gain).

use open_stomp::ir::{self, PEDAL_IR_TAPS, Wav};
use rustfft::{FftPlanner, num_complex::Complex64};

fn third_octaves(h: &[f64]) -> Vec<f64> {
    let n = h.len().next_power_of_two().max(1 << 16);
    let mut c: Vec<Complex64> = h.iter().map(|&v| Complex64::new(v, 0.0)).collect();
    c.resize(n, Complex64::default());
    FftPlanner::new().plan_fft_forward(n).process(&mut c);
    (0..28)
        .map(|i| {
            let fc = 31.5 * 2f64.powf(i as f64 / 3.0);
            let (lo, hi) = ((fc / 2f64.powf(1.0 / 6.0) * n as f64 / 48000.0) as usize, (fc * 2f64.powf(1.0 / 6.0) * n as f64 / 48000.0) as usize);
            let p = c[lo..=hi.max(lo)].iter().map(|v| v.norm_sqr()).sum::<f64>() / (hi.max(lo) - lo + 1) as f64;
            10.0 * (p + 1e-30).log10()
        })
        .collect()
}

/// (shape error, level difference, worst band difference) of `a` against `b`.
fn compare(a: &[f64], b: &[f64]) -> (f64, f64, f64) {
    let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let mean = d.iter().sum::<f64>() / d.len() as f64;
    let rms = (d.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / d.len() as f64).sqrt();
    let worst = d.iter().map(|v| v - mean).fold(0f64, |m, v| if v.abs() > m.abs() { v } else { m });
    (rms, mean, worst)
}

fn played(data: &[f32]) -> Vec<f64> {
    data.iter().rev().take(PEDAL_IR_TAPS).map(|&v| v as f64).collect()
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut files: Vec<std::path::PathBuf> = vec![];
    for a in std::env::args().skip(1) {
        let p = std::path::PathBuf::from(a);
        if p.is_dir() {
            files.extend(std::fs::read_dir(&p)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"))));
        } else {
            files.push(p);
        }
    }
    files.sort();
    println!("{:34} {:>22} {:>22}", "", "official (on pedal)", "optimised (on pedal)");
    println!("{:34} {:>8} {:>6} {:>6} {:>8} {:>6} {:>6}", "file", "shape", "level", "worst", "shape", "level", "worst");
    let (mut so, mut sp) = (vec![], vec![]);
    for f in &files {
        let wav = Wav::load(f)?;
        let off = ir::convert(&wav);
        let opt = ir::convert_optimised(&wav);
        // the full IR at the official gain: what the official app means the pedal to play
        let full: Vec<f64> = off.ir.iter().map(|&v| v as f64 * off.gain as f64).collect();
        let reference = third_octaves(&full);
        let (a, b) = (compare(&third_octaves(&played(&off.data)), &reference), compare(&third_octaves(&played(&opt.data)), &reference));
        let name: String = f.file_stem().unwrap_or_default().to_string_lossy().chars().take(34).collect();
        println!("{name:34} {:8.2} {:+6.1} {:+6.1} {:8.2} {:+6.1} {:+6.1}", a.0, a.1, a.2, b.0, b.1, b.2);
        if std::env::var("BANDS").is_ok() {
            let (po, pp) = (third_octaves(&played(&off.data)), third_octaves(&played(&opt.data)));
            let (mo, mp) = (a.1, b.1);
            println!("   band Hz:  {}", (0..28).step_by(2).map(|i| format!("{:>6.0}", 31.5 * 2f64.powf(i as f64 / 3.0))).collect::<Vec<_>>().join(""));
            println!("   official: {}", (0..28).step_by(2).map(|i| format!("{:+6.1}", po[i] - reference[i] - mo)).collect::<Vec<_>>().join(""));
            println!("   optimised:{}", (0..28).step_by(2).map(|i| format!("{:+6.1}", pp[i] - reference[i] - mp)).collect::<Vec<_>>().join(""));
        }
        so.push(a.0);
        sp.push(b.0);
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    println!("{:34} {:8.2} {:>13} {:8.2}", "mean shape error (dB rms)", mean(&so), "", mean(&sp));
    Ok(())
}

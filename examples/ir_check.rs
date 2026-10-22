//! Compare uploaded user IR data (2048 little-endian floats, e.g. from a capture) with IR audio
//! files: the best alignment of the data with each file, forward and time-reversed.
//!
//! ```sh
//! cargo run --release --example ir_check -- IR01.device_floats.bin a.wav [b.wav ...]
//! ```

use open_stomp::ir::Wav;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dev_path = args.first().ok_or("usage: ir_check <device floats> <file.wav>...")?;
    let dev: Vec<f64> = std::fs::read(dev_path)?.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c) as f64).collect();
    let e_dev: f64 = dev.iter().map(|v| v * v).sum();
    let peak = dev.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap();
    println!("{dev_path}: {} floats, peak {:.4} at {}, rms {:.4}", dev.len(), peak.1, peak.0, (e_dev / dev.len() as f64).sqrt());
    for path in &args[1..] {
        let w = Wav::load(path)?;
        let x: Vec<f64> = w.channels[0].iter().map(|&v| v as f64).collect();
        let xpeak = x.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap();
        println!("\n{path}: {} Hz, {} ch, {} samples, peak {:.4} at {}", w.sample_rate, w.channels.len(), x.len(), xpeak.1, xpeak.0);
        // candidate normalisations: what gain each would apply
        let n = dev.len().min(x.len());
        let (head, all) = (&x[..n], &x[..]);
        let peak = |v: &[f64]| v.iter().fold(0f64, |m, a| m.max(a.abs()));
        let energy = |v: &[f64]| v.iter().map(|a| a * a).sum::<f64>();
        let abs_sum = |v: &[f64]| v.iter().map(|a| a.abs()).sum::<f64>();
        println!(
            "  1/peak: all {:.4}, first {n} {:.4} | 1/sqrt(energy): all {:.4}, first {n} {:.4} | 1/sum|x|: all {:.4}, first {n} {:.4} | 1/sum x: first {n} {:.4}",
            1.0 / peak(all), 1.0 / peak(head), 1.0 / energy(all).sqrt(), 1.0 / energy(head).sqrt(), 1.0 / abs_sum(all), 1.0 / abs_sum(head), 1.0 / head.iter().sum::<f64>()
        );
        for (label, d) in [("forward", dev.clone()), ("reversed", dev.iter().rev().copied().collect::<Vec<_>>())] {
            // normalised correlation of d with every 2048-sample window of x
            let n = d.len();
            let mut best: Vec<(f64, usize, f64, f64)> = vec![];
            let mut e_win: f64 = x[..n.min(x.len())].iter().map(|v| v * v).sum();
            for lag in 0..x.len().saturating_sub(n) + 1 {
                if lag > 0 {
                    e_win += x[lag + n - 1].powi(2) - x[lag - 1].powi(2);
                }
                let dot: f64 = d.iter().zip(&x[lag..lag + n]).map(|(a, b)| a * b).sum();
                let r = dot / (e_dev * e_win.max(1e-30)).sqrt();
                let gain = dot / e_win.max(1e-30);
                let resid = 10.0 * (1.0 - r * r).max(1e-12).log10();
                best.push((r, lag, gain, resid));
            }
            best.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
            for (r, lag, gain, resid) in best.iter().take(3) {
                println!("  {label:8} lag {lag:7}: corr {r:+.4}, gain {gain:9.4}, unexplained {resid:6.1} dB");
            }
        }
    }
    Ok(())
}

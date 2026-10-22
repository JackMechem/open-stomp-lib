//! Output levels of `.nam` models: the loudness in their metadata, and the level measured on our
//! test signal (as the model runs, and as the pedal target the optimiser fits to).
//!
//! ```sh
//! cargo run --release --example nam_level -- <dir or .nam files>
//! ```

use open_stomp::nam::{Model, optimise};

fn db(x: &[f64]) -> f64 {
    10.0 * (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).log10()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut files: Vec<std::path::PathBuf> = vec![];
    for a in std::env::args().skip(1) {
        let p = std::path::PathBuf::from(a);
        if p.is_dir() {
            files.extend(std::fs::read_dir(&p)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "nam")));
        } else {
            files.push(p);
        }
    }
    files.sort();
    let sig = optimise::test_signal();
    let seg = sig.segments.iter().find(|s| s.name == "pink -18").expect("pink segment");
    let pink = &sig.x[seg.start..seg.start + seg.len];
    println!("{:44} {:>9} {:>14} {:>14} {:>12}", "model", "metadata", "raw output", "pedal target", "normalising");
    println!("{:44} {:>9} {:>14} {:>14} {:>12}", "", "loudness", "(pink -18 in)", "(optimiser)", "gain");
    for f in &files {
        let mut m = Model::load(f)?;
        // raw: the pink noise straight in; pedal target: what the optimiser matches the pedal to
        let input: Vec<f32> = pink.iter().map(|&v| v as f32).collect();
        let mut out = vec![0.0f32; input.len()];
        m.reset();
        m.process(&input, &mut out);
        let raw: Vec<f64> = out[4800..].iter().map(|&v| v as f64).collect();
        let target = optimise::render_target(&mut m, pink);
        let name: String = f.file_stem().unwrap_or_default().to_string_lossy().chars().take(44).collect();
        let level = open_stomp::nam::convert::pedal_level_db(&mut m);
        println!(
            "{name:44} {:>9} {:>11.1} dB {:>11.1} dB {:>+9.1} dB",
            m.loudness.map_or("—".into(), |l| format!("{l:.1} dB")),
            db(&raw),
            db(&target[4800..]),
            open_stomp::nam::convert::NORMALISED_LEVEL_DB - level
        );
    }
    Ok(())
}

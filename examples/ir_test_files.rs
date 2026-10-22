//! Write the designed test IRs used to pin down the official app's IR conversion (docs/09): each
//! isolates one question (gain rule, resampling, channels, length, format, name).
//!
//! ```sh
//! cargo run --example ir_test_files -- tests/data/ir
//! ```
//! Import `T1` … `T10` into USER IR 01 … 10 with the official app while capturing, then compare the
//! uploads with `ir_check`.

use open_stomp::ir::Wav;

fn impulses(len: usize, at: &[(usize, f32)]) -> Vec<f32> {
    let mut x = vec![0.0; len];
    for &(i, v) in at {
        x[i] = v;
    }
    x
}

fn main() -> std::io::Result<()> {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "tests/data/ir".into()));
    std::fs::create_dir_all(&dir)?;
    let mono = |rate, x| Wav { sample_rate: rate, channels: vec![x] };
    let files: Vec<(&str, Wav, u16, bool)> = vec![
        ("T1 impulse", mono(48000, impulses(2048, &[(0, 0.5)])), 24, false),
        ("T2 two impulses", mono(48000, impulses(2048, &[(0, 0.5), (1000, -0.25)])), 24, false),
        ("T3 tail", mono(48000, impulses(4800, &[(0, 0.5), (3000, 0.9)])), 24, false),
        ("T4 quiet", mono(48000, impulses(2048, &[(0, 0.01)])), 24, false),
        ("T5 44k", mono(44100, impulses(2048, &[(100, 0.5)])), 24, false),
        ("T6 stereo", Wav { sample_rate: 48000, channels: vec![impulses(2048, &[(0, 0.5)]), impulses(2048, &[(50, -0.5)])] }, 24, false),
        ("T7 short", mono(48000, impulses(100, &[(0, 0.5)])), 24, false),
        ("T8 float", mono(48000, impulses(2048, &[(0, 0.5), (10, 0.3)])), 32, true),
        ("T9 a name longer than sixteen", mono(48000, impulses(2048, &[(0, 0.25)])), 24, false),
        ("T10 96k", mono(96000, impulses(4096, &[(200, 0.5)])), 24, false),
    ];
    for (name, wav, bits, float) in files {
        let p = dir.join(format!("{name}.wav"));
        std::fs::write(&p, wav.to_bytes(bits, float))?;
        println!("wrote {}", p.display());
    }
    Ok(())
}

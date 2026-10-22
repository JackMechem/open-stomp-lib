//! The NAM → pedal converter (docs/05).
#![cfg(feature = "nam")]

use open_stomp::nam::{Model, convert};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/nam/");

#[test]
fn converter_properties() {
    let mut m = Model::load(format!("{DIR}wavenet.nam")).unwrap();
    let c = convert::convert(&mut m);
    assert_eq!((c.data.len(), c.factor), (2048, 1.0));
    assert!(c.data.iter().all(|v| v.is_finite()));
    assert_eq!((c.levels.len(), c.levels3.len(), c.g1_gains_db.len()), (94, 83, 94));
    // the upload is the first 1024 taps of each filter, reversed
    assert!((c.data[1023] as f64 - c.g1_ir[0]).abs() < 1e-6 && (c.data[2047] as f64 - c.g2_ir[0]).abs() < 1e-6);

    // G1 follows its per-tone targets. Not exactly: the app mirrors the spectrum as m[8191-k] = m[k]
    // (not m[N-k]) and keeps the real part, which blends neighbouring bins and so shaves kinks off
    // jagged curves; we copy that on purpose (the app's own G1 misses its targets by up to ~1 dB).
    // For the same reason |G1·G2| is not exactly the measured response H where H is jagged, so that
    // isn't checked here; the comparison with the app below covers G2.
    let spec = |ir: &[f64], k: usize| {
        let w = std::f64::consts::TAU * k as f64 / 8192.0;
        let (re, im) = ir.iter().enumerate().fold((0.0, 0.0), |(r, i), (t, v)| (r + v * (w * t as f64).cos(), i - v * (w * t as f64).sin()));
        10.0 * (re * re + im * im).log10()
    };
    let mut g1_err: Vec<f64> = convert::TONE_BINS
        .iter()
        .zip(&c.g1_gains_db)
        .map(|(&k, target)| (spec(&c.g1_ir, k) - target).abs())
        .collect();
    g1_err.sort_by(f64::total_cmp);
    let (median, max) = (g1_err[g1_err.len() / 2], g1_err[g1_err.len() - 1]);
    assert!(median < 0.2 && max < 3.0, "G1 vs targets: median {median:.3} dB, max {max:.3} dB");


    // deterministic
    let again = convert::convert(&mut m);
    assert_eq!(again.data, c.data);

    // Clean mode uploads only the taps the pedal runs
    let clean = convert::convert_measured(&mut m, &convert::excitation(), convert::Measure::Clean);
    assert!(clean.data[..512].iter().chain(&clean.data[1024..1536]).all(|&v| v == 0.0));
    let (h1, h2) = convert::pedal_irs(&clean.data);
    assert!((h1[0] - clean.g1_ir[0]).abs() < 1e-6 && (h2[511] - clean.g2_ir[511]).abs() < 1e-6);
}

/// Against the official app, when harness dumps are available locally (they contain vendor and
/// third-party data, so they aren't in the repository). Set OPEN_STOMP_NAMCONV to a directory with
/// `<name>.nam` and `<name>/` (a `tools/namconv/harness` dump) pairs.
#[test]
fn matches_official_app_when_dumps_available() {
    let Ok(root) = std::env::var("OPEN_STOMP_NAMCONV") else {
        eprintln!("OPEN_STOMP_NAMCONV not set: skipping the comparison with the official app");
        return;
    };
    let f32s = |p: std::path::PathBuf| -> Vec<f32> {
        std::fs::read(&p).unwrap_or_else(|_| panic!("{p:?}")).chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
    };
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "nam") {
            continue;
        }
        let dump = path.with_extension("");
        let mut m = Model::load(&path).unwrap();
        let c = convert::convert_with(&mut m, &f32s(dump.join("BLA_x.bin")));
        let theirs = [f32s(dump.join("cb_g1.bin")), f32s(dump.join("cb_g2.bin"))].concat();
        let num: f32 = c.data.iter().zip(&theirs).map(|(a, b)| (a - b).powi(2)).sum();
        let den: f32 = theirs.iter().map(|b| b * b).sum();
        let db = 10.0 * (num / den).log10();
        eprintln!("{path:?}: error energy {db:.1} dB");
        assert!(db < -45.0, "{path:?}: {db} dB");
    }
}

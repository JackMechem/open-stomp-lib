//! NAM inference against NeuralAmpModelerCore's output (tests/data/nam/README.md).
#![cfg(feature = "nam")]

use open_stomp::nam::Model;

fn f32s(path: &str) -> Vec<f32> {
    std::fs::read(path).expect(path).as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()
}

fn check(name: &str) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/nam/");
    let mut m = Model::load(format!("{dir}{name}.nam")).expect("model loads");
    let x = f32s(&format!("{dir}input.f32"));
    let want = f32s(&format!("{dir}{name}.out.f32"));
    let mut y = vec![0.0; x.len()];
    m.process(&x, &mut y);
    let err = y.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(err < 1e-4, "{name}: max error {err}");
    // processing in other block sizes gives the same result
    m.reset();
    let mut z = vec![0.0; x.len()];
    for (i, o) in x.chunks(333).zip(z.chunks_mut(333)) {
        m.process(i, o);
    }
    let err2 = z.iter().zip(&y).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(err2 < 1e-6, "{name}: block size changed the output by {err2}");
}

#[test]
fn wavenet_classic_small() {
    check("wavenet");
}

#[test]
fn wavenet_a1_standard() {
    check("wavenet_a1_standard");
}

#[test]
fn slimmable_container() {
    check("slimmable_container");
}

#[test]
fn a2_slimmable_container() {
    check("A2");
}

#[test]
fn unsupported_is_an_error() {
    let lstm = r#"{"version": "0.5.4", "architecture": "LSTM", "config": {}, "weights": []}"#;
    assert!(matches!(Model::from_json_str(lstm), Err(open_stomp::nam::NamError::Unsupported(_))));
    let bad = r#"{"version": "0.5.4", "architecture": "WaveNet", "config": {"layers": []}, "weights": []}"#;
    assert!(Model::from_json_str(bad).is_err());
}

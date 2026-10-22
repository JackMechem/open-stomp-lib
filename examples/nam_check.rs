//! Compare this crate's NAM inference with a recording of NeuralAmpModelerCore's output.
//!
//! `cargo run --release --example nam_check -- <model.nam> <model_io.bin>`
//!
//! `model_io.bin` is written by the open-stomp-manager `tools/namconv/harness --log-io` (records of
//! int32 n, f32 input[n], f32 output[n], in call order). The calls are replayed in order on one
//! model, so state carries over exactly as in the recording.

use open_stomp::nam::Model;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [model, log] = &args[..] else {
        return Err("usage: nam_check <model.nam> <model_io.bin>".into());
    };
    let mut m = Model::load(model)?;
    println!("{} ({}), {:?} Hz", m.name.as_deref().unwrap_or("?"), m.architecture, m.sample_rate());
    let data = std::fs::read(log)?;
    let f32s = |b: &[u8]| b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect::<Vec<_>>();
    let (mut pos, mut call, mut worst) = (0, 0, 0f32);
    let t0 = std::time::Instant::now();
    while pos < data.len() {
        let n = i32::from_le_bytes(data[pos..pos + 4].try_into()?) as usize;
        pos += 4;
        let x = f32s(&data[pos..pos + 4 * n]);
        pos += 4 * n;
        let y = f32s(&data[pos..pos + 4 * n]);
        pos += 4 * n;
        let mut out = vec![0.0; n];
        m.process(&x, &mut out);
        let err = out.iter().zip(&y).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        let peak = y.iter().map(|v| v.abs()).fold(0.0, f32::max);
        if call < 3 || err / peak > 1e-3 {
            println!("call {call:3} n={n:5}  max |ours - theirs| = {err:.3e}  (peak {peak:.3e}, rel {:.2e})", err / peak);
            if call == 0 {
                println!("           first outputs ours {:?} theirs {:?}", &out[..3], &y[..3]);
            }
        }
        worst = worst.max(err / peak);
        call += 1;
    }
    println!("{call} calls, worst relative error {worst:.2e}, {:.2?}", t0.elapsed());
    Ok(())
}

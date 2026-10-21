//! Run raw mono float32 audio (48 kHz, little-endian) through a `.nam` model.
//! `cargo run --release --example nam_render -- model.nam in.f32 out.f32`
use open_stomp::nam::Model;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [model, input, output] = &a[..] else { return Err("usage: nam_render <model.nam> <in.f32> <out.f32>".into()) };
    let mut m = Model::load(model)?;
    let x: Vec<f32> = std::fs::read(input)?.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
    let mut y = vec![0.0; x.len()];
    m.process(&x, &mut y);
    std::fs::write(output, y.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>())?;
    Ok(())
}

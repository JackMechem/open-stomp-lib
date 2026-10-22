//! Optimised conversion of a `.nam` file (`nam::optimise`): fit the pedal's filters with a
//! simulation of the pedal, starting from the clean conversion.
//!
//! ```sh
//! cargo run --release --example nam_optimise -- model.nam [out.bin] [--steps N] [--normalise]
//! ```
//!
//! `out.bin` gets the 2048 little-endian floats (`Device::import_user_amp` takes them as a slice).

use open_stomp::nam::{Model, convert, optimise};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut options = optimise::Options::default();
    if let Some(i) = args.iter().position(|a| a == "--steps") {
        options.steps = args.get(i + 1).ok_or("--steps N")?.parse()?;
        args.drain(i..i + 2);
    }
    let normalise = args.iter().position(|a| a == "--normalise").map(|i| args.remove(i)).is_some();
    let path = args.first().ok_or("usage: nam_optimise <model.nam> [out.bin] [--steps N] [--normalise]")?;
    let mut model = Model::load(path)?;
    let t = std::time::Instant::now();
    let clean = convert::convert_measured(&mut model, &convert::excitation(), convert::Measure::Clean);
    println!("clean conversion: {:.1} s", t.elapsed().as_secs_f64());
    let t = std::time::Instant::now();
    let mut r = optimise::optimise(&mut model, &clean.data, &options);
    if normalise {
        let mut c = clean.clone();
        c.data = r.data.clone();
        let db = convert::normalise_level(&mut c, &mut model);
        r.data = c.data;
        println!("normalised: {db:+.1} dB");
    }
    let best = r.loss.iter().copied().fold(f64::INFINITY, f64::min);
    for (i, l) in r.loss.iter().enumerate().filter(|(i, _)| i % 25 == 0 || *i + 1 == r.loss.len()) {
        println!("  step {i:4}: {l:.4}");
    }
    println!("optimised: loss {:.4} -> {best:.4} in {:.1} s", r.initial_loss, t.elapsed().as_secs_f64());
    if let Some(out) = args.get(1) {
        std::fs::write(out, r.data.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>())?;
        println!("wrote {out}");
    }
    Ok(())
}

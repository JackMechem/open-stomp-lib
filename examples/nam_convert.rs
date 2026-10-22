//! Convert a `.nam` file into the pedal's user AMP data, and optionally compare with the official
//! app's output.
//!
//! ```sh
//! cargo run --release --example nam_convert -- model.nam [out.bin]
//! cargo run --release --example nam_convert -- model.nam --official [app path] [out.bin]
//! cargo run --release --example nam_convert -- model.nam --compare <harness dump dir>
//! ```
//!
//! `out.bin` gets the 2048 little-endian floats (`Device::import_user_amp` takes them as a slice).
//! `--compare` takes a dump from the open-stomp-manager `tools/namconv/harness` and reports how
//! close our upload is to the vendor's, both with the vendor's BLA excitation (`BLA_x.bin`, checks
//! the algorithm) and with ours (what users get).

use open_stomp::nam::{Model, convert};

fn read_f32(path: &std::path::Path) -> std::io::Result<Vec<f32>> {
    Ok(std::fs::read(path)?.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect())
}

/// 1/3-octave band levels (dB) of a 1024-tap filter stored reversed, 50 Hz – 16 kHz.
fn third_octaves(taps_reversed: &[f32]) -> Vec<f64> {
    let n = 1 << 15;
    let h: Vec<f64> = taps_reversed.iter().rev().map(|&v| v as f64).collect();
    let mut bands = vec![];
    let mut fc = 50.0f64;
    while fc < 16000.0 {
        let (lo, hi) = (fc / 2f64.powf(1.0 / 6.0), fc * 2f64.powf(1.0 / 6.0));
        let (k0, k1) = ((lo / 48000.0 * n as f64) as usize, (hi / 48000.0 * n as f64) as usize);
        let p: f64 = (k0..=k1)
            .map(|k| {
                let w = std::f64::consts::TAU * k as f64 / n as f64;
                let (re, im) = h.iter().enumerate().fold((0.0, 0.0), |(r, i), (t, v)| (r + v * (w * t as f64).cos(), i - v * (w * t as f64).sin()));
                re * re + im * im
            })
            .sum::<f64>()
            / (k1 - k0 + 1) as f64;
        bands.push(10.0 * p.log10());
        fc *= 2f64.powf(1.0 / 3.0);
    }
    bands
}

fn compare(label: &str, ours: &[f32], theirs: &[f32]) {
    for (half, name) in [(0..1024, "g1"), (1024..2048, "g2")] {
        let (a, b) = (&ours[half.clone()], &theirs[half]);
        let num: f32 = a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum();
        let den: f32 = b.iter().map(|y| y * y).sum();
        let (ta, tb) = (third_octaves(a), third_octaves(b));
        let d: Vec<f64> = ta.iter().zip(&tb).map(|(x, y)| x - y).collect();
        let rms = (d.iter().map(|x| x * x).sum::<f64>() / d.len() as f64).sqrt();
        let max = d.iter().fold(0.0f64, |m, x| m.max(x.abs()));
        println!("  {label:28} {name}: error energy {:6.1} dB | 1/3-octave response: rms {rms:5.2} dB, max {max:5.2} dB", 10.0 * (num / den).log10());
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().ok_or("usage: nam_convert <model.nam> [out.bin | --compare <dump dir>]")?;
    let mut model = Model::load(path)?;
    let t0 = std::time::Instant::now();
    let mut rest: Vec<&str> = args[1..].iter().map(String::as_str).collect();
    let official = if rest.first() == Some(&"--official") {
        rest.remove(0);
        let x = match rest.first().filter(|a| !a.ends_with(".bin") && std::path::Path::new(a).exists()) {
            Some(p) => { let x = convert::official_excitation(p)?; rest.remove(0); x }
            None => convert::find_official_excitation().ok_or("Stomp Manager not found; pass its path after --official")?,
        };
        println!("using the official app's excitation");
        Some(x)
    } else {
        None
    };
    let ours = match &official {
        Some(x) => convert::convert_with(&mut model, x),
        None => convert::convert(&mut model),
    };
    println!("{}: converted in {:.2?}", model.name.as_deref().unwrap_or(path), t0.elapsed());
    match rest.first().copied() {
        Some("--compare") => {
            let dir = std::path::Path::new(*rest.get(1).ok_or("--compare needs a dump dir")?);
            let theirs: Vec<f32> = [read_f32(&dir.join("cb_g1.bin"))?, read_f32(&dir.join("cb_g2.bin"))?].concat();
            let vendor_x = read_f32(&dir.join("BLA_x.bin"))?;
            compare("vendor excitation", &convert::convert_with(&mut model, &vendor_x).data, &theirs);
            compare("our excitation", &ours.data, &theirs);
        }
        Some(out) => {
            let bytes: Vec<u8> = ours.data.iter().flat_map(|f| f.to_le_bytes()).collect();
            std::fs::write(out, bytes)?;
            println!("wrote {} floats to {out}", ours.data.len());
        }
        None => {}
    }
    Ok(())
}

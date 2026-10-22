//! Compare the official app's IR imports in a capture log with the IR files that were imported
//! (e.g. the designed test IRs in `tests/data/ir`): the gain the app applied, whether the rest of
//! the conversion matches ours, the slot and name records.
//!
//! ```sh
//! cargo run --release --example ir_capture_check -- captures/logs/<session>.log tests/data/ir
//! ```

use open_stomp::capture::{self, Entry};
use open_stomp::ir::{self, Wav};
use open_stomp::midi::Msg;
use open_stomp::state::Origin;
use open_stomp::upload::{Chunk, Reassembler};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (log, dir) = (args.first().ok_or("usage: ir_capture_check <log> <wav dir>")?, args.get(1).map_or("tests/data/ir", String::as_str));
    let mut files: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")))
        .map(|p| (p.file_stem().unwrap_or_default().to_string_lossy().into_owned(), p))
        .collect();
    // longest names first, so "T10 96k" isn't taken for "T1 …"
    files.sort_by_key(|(n, _)| std::cmp::Reverse(n.len()));

    let mut uploads: Vec<(u8, Vec<f32>)> = vec![];
    let mut names: Vec<(u8, [u8; 4], String)> = vec![];
    let mut asm = Reassembler::default();
    for e in capture::parse(&std::fs::read_to_string(log)?) {
        let Entry::Message { origin: Origin::Host, bytes, .. } = e else { continue };
        match Msg::parse(&bytes) {
            Msg::Nep { cmd: 0x10, dir: 1, payload } => {
                if let Some(c) = Chunk::parse(&payload)
                    && let Some(u) = asm.push(&c)
                {
                    uploads.push(u);
                }
            }
            Msg::Nep { cmd: 0x09, dir: 1, payload } if !payload.is_empty() => {
                let raw = open_stomp::pack::unpack(&payload[1..]);
                if raw.len() >= 36 {
                    let name: String = raw[4..36].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
                    names.push((payload[0], raw[..4].try_into().unwrap(), name));
                }
            }
            _ => {}
        }
    }
    println!("{} IR uploads, {} name records in {log}\n", uploads.len(), names.len());
    for (i, (slot, data)) in uploads.iter().enumerate() {
        let rec = names.get(i);
        let name = rec.map_or("?", |r| r.2.as_str());
        println!("upload {i}: data header slot {slot}, name record slot {:?}, field {:02x?}, name {name:?}", rec.map(|r| r.0), rec.map(|r| r.1));
        let nz: Vec<(usize, f32)> = data.iter().enumerate().filter(|(_, v)| v.abs() > 1e-7).map(|(k, &v)| (k, v)).collect();
        print!("  {} non-zero values; largest at", nz.len());
        let mut big = nz.clone();
        big.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        for (k, v) in big.iter().take(4) {
            print!("  [{k}] {v:+.6}");
        }
        println!();
        let Some((stem, path)) = files.iter().find(|(stem, _)| stem.starts_with(name) && !name.is_empty()) else {
            println!("  no matching file\n");
            continue;
        };
        let wav = Wav::load(path)?;
        let c = ir::convert(&wav);
        // ours without the gain (reversed head of the 48 kHz IR) vs the app's data
        let raw: Vec<f64> = c.data.iter().map(|&v| v as f64 / c.gain as f64).collect();
        let d: Vec<f64> = data.iter().map(|&v| v as f64).collect();
        let (dot, ee) = (raw.iter().zip(&d).map(|(a, b)| a * b).sum::<f64>(), raw.iter().map(|a| a * a).sum::<f64>());
        let g = dot / ee;
        let resid: f64 = raw.iter().zip(&d).map(|(a, b)| (b - g * a).powi(2)).sum::<f64>() / d.iter().map(|b| b * b).sum::<f64>();
        let head = &c.ir[..c.ir.len().min(ir::IR_TAPS)];
        let peak = head.iter().fold(0f32, |m, v| m.max(v.abs())) as f64;
        let energy: f64 = head.iter().map(|&v| v as f64 * v as f64).sum();
        let file_peak = wav.channels[0].iter().fold(0f32, |m, v| m.max(v.abs())) as f64;
        println!(
            "  file {stem:?} ({} Hz, {} ch, {} samples): app gain {g:.6} ({:+.3} dB); residual after our layout {:.1} dB",
            wav.sample_rate, wav.channels.len(), wav.channels[0].len(), 20.0 * g.abs().log10(), 10.0 * resid.max(1e-15).log10()
        );
        let err: f64 = c.data.iter().zip(data).map(|(a, b)| (*a as f64 - *b as f64).powi(2)).sum::<f64>()
            / data.iter().map(|b| (*b as f64).powi(2)).sum::<f64>();
        let worst = c.data.iter().zip(data).fold(0f64, |m, (a, b)| m.max((a - b).abs() as f64));
        println!("  ours vs the app: error energy {:.1} dB, largest difference {worst:.2e}", 10.0 * err.max(1e-20).log10());
        println!(
            "  gain × peak(head) {:.5}, × peak(file) {:.5}, × sqrt(energy head) {:.5}, ours {:.6}\n",
            g * peak, g * file_peak, g * energy.sqrt(), c.gain
        );
    }
    Ok(())
}

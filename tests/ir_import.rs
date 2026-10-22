//! The official app's IR imports, captured for ten designed test IRs (`captures/logs/02-ir-tests.log`,
//! files in `tests/data/ir`): our conversion and upload against every one.

use open_stomp::capture::{self, Entry};
use open_stomp::ir::{self, Wav};
use open_stomp::midi::Msg;
use open_stomp::state::Origin;
use open_stomp::upload::{self, Chunk, Reassembler};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// One captured import: the model select, the data frames, the name frame and the decoded floats.
struct Import {
    select: Vec<u8>,
    frames: Vec<Vec<u8>>,
    slot: u8,
    name: String,
    data: Vec<f32>,
}

fn imports() -> Vec<Import> {
    let log = std::fs::read_to_string(format!("{ROOT}/captures/logs/02-ir-tests.log")).unwrap();
    let (mut out, mut frames, mut select, mut asm, mut data) = (vec![], vec![], vec![], Reassembler::default(), None);
    for e in capture::parse(&log) {
        let Entry::Message { origin: Origin::Host, bytes, .. } = e else { continue };
        match Msg::parse(&bytes) {
            Msg::Nep { cmd: 0x10, dir: 1, payload } => {
                if let Some(u) = asm.push(&Chunk::parse(&payload).unwrap()) {
                    data = Some(u.1);
                }
                frames.push(bytes);
            }
            Msg::Nep { cmd: 0x09, dir: 1, payload } => {
                let raw = open_stomp::pack::unpack(&payload[1..]);
                let name = raw[4..36].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
                frames.push(bytes);
                out.push(Import { select: std::mem::take(&mut select), frames: std::mem::take(&mut frames), slot: payload[0], name, data: data.take().unwrap() });
            }
            // the cab model select (CC 2 on channel 2)
            _ if bytes.len() == 3 && bytes[..2] == [0xB1, 0x02] => select = bytes,
            _ => {}
        }
    }
    out
}

/// The test file an import came from (its name is the file name cut to 16 characters).
fn file_for(name: &str) -> std::path::PathBuf {
    let mut files: Vec<_> = std::fs::read_dir(format!("{ROOT}/tests/data/ir")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "wav")).collect();
    files.sort_by_key(|p| std::cmp::Reverse(p.as_os_str().len()));
    files.into_iter().find(|p| ir::slot_name(p) == name).unwrap_or_else(|| panic!("no file for {name:?}"))
}

#[test]
fn conversion_matches_the_official_app() {
    let imports = imports();
    assert_eq!(imports.len(), 11);
    for imp in &imports {
        let c = ir::convert(&Wav::load(file_for(&imp.name)).unwrap());
        let worst = c.data.iter().zip(&imp.data).fold(0f32, |m, (a, b)| m.max((a - b).abs()));
        // bit-identical for 48 kHz files; within float rounding where the app resampled
        assert!(worst < 2e-6, "{}: largest difference {worst:e}", imp.name);
    }
}

#[test]
fn upload_frames_match_the_official_app() {
    let imports = imports();
    assert!(imports.iter().filter(|i| !i.select.is_empty()).count() >= 9);
    for imp in imports {
        // the official app's own data, framed by us: byte for byte, slot 0 in the data header,
        // the real slot in the name record
        let path = file_for(&imp.name);
        assert_eq!(imp.name, ir::slot_name(&path));
        let ours = upload::user_ir_upload(imp.slot, &imp.name, &imp.data);
        assert_eq!(ours.len(), imp.frames.len(), "{}: frame count", imp.name);
        for (k, (a, b)) in ours.iter().zip(&imp.frames).enumerate() {
            assert_eq!(a, b, "{}: frame {k} of {}", imp.name, ours.len());
        }
        // before each import the app selects the slot's cab model (not when it already is)
        assert!(imp.select.is_empty() || imp.select == [0xB1, 0x02, 30 + imp.slot], "{}: {:02x?}", imp.name, imp.select);
    }
}

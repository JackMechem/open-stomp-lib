//! User AMP (NAM, SysEx `0x58`) and user IR (SysEx `0x10` + `0x09`) uploads (docs/05).
//!
//! Both send a converted model as 2048 little-endian `f32`s (8192 bytes) in chunks of 136
//! bytes, each a packed SysEx write acknowledged before the next. Every chunk starts with a
//! 12-byte header:
//!
//! ```text
//! [0] 0   [1] slot   [2] sub-command   [3] 0   [4..8] byte offset (u32 LE)   [8..12] total length (u32 LE)
//! ```
//!
//! A user AMP upload is preceded by a 128-byte slot-info record (sub 3) carrying the name and a
//! gain factor; a user IR upload is followed by a `0x09` name write. Converting a `.nam` or
//! audio file into the 2048 floats is not reversed yet (docs/05, docs/09).

use crate::midi::{Cmd, Dir, frame};
use crate::pack::{pack, unpack};

/// Bytes of payload per chunk (34 floats).
pub const CHUNK_BYTES: usize = 136;
/// Floats per upload, as the official app sends them.
pub const UPLOAD_FLOATS: usize = 2048;
/// Number of user AMP slots (amp models 30–39) and user IR slots (cab models 30–39).
pub const USER_SLOTS: u8 = 10;
/// Model number of user slot 0 (USER AMP 01 / USER IR 01).
pub const USER_MODEL_BASE: u8 = 30;

/// Why an upload was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadError {
    /// Slots are 0–9.
    Slot(u8),
    /// The pedal takes exactly [`UPLOAD_FLOATS`] floats.
    Length(usize),
    /// NaN or infinite values.
    NotFinite,
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UploadError::Slot(s) => write!(f, "user slot {s} out of range (0-9)"),
            UploadError::Length(n) => write!(f, "{n} floats, the pedal takes exactly {UPLOAD_FLOATS}"),
            UploadError::NotFinite => write!(f, "model data contains NaN or infinite values"),
        }
    }
}

impl std::error::Error for UploadError {}

/// Check a slot and model data before uploading.
pub fn check(slot: u8, data: &[f32]) -> Result<(), UploadError> {
    if slot >= USER_SLOTS {
        return Err(UploadError::Slot(slot));
    }
    if data.len() != UPLOAD_FLOATS {
        return Err(UploadError::Length(data.len()));
    }
    if !data.iter().all(|f| f.is_finite()) {
        return Err(UploadError::NotFinite);
    }
    Ok(())
}

/// `0x58` sub-commands (unpacked header byte 2).
pub mod amp_sub {
    pub const DATA: u8 = 1;
    pub const WRITE_INFO: u8 = 3;
    pub const READ_INFO: u8 = 4;
    pub const DELETE: u8 = 6;
}

/// `0x10` sub-command for IR data.
pub const IR_DATA_SUB: u8 = 0;

/// One upload chunk (or a header-only request/ack), unpacked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub slot: u8,
    pub sub: u8,
    /// Byte offset of `data` in the whole upload.
    pub offset: u32,
    /// Total upload length in bytes (8192 for model data, 128 for a slot-info record).
    pub total: u32,
    pub data: Vec<u8>,
}

impl Chunk {
    /// Decode a packed `0x58`/`0x10` body (the bytes after `<dir>`).
    pub fn parse(payload: &[u8]) -> Option<Chunk> {
        let raw = unpack(payload);
        let h = raw.get(..12)?;
        Some(Chunk {
            slot: h[1],
            sub: h[2],
            offset: u32::from_le_bytes(h[4..8].try_into().ok()?),
            total: u32::from_le_bytes(h[8..12].try_into().ok()?),
            data: raw[12..].to_vec(),
        })
    }

    fn header(slot: u8, sub: u8, offset: u32, total: u32) -> [u8; 12] {
        let mut h = [0u8; 12];
        h[1] = slot;
        h[2] = sub;
        h[4..8].copy_from_slice(&offset.to_le_bytes());
        h[8..12].copy_from_slice(&total.to_le_bytes());
        h
    }

    /// The packed body: header + data.
    pub fn to_packed(&self) -> Vec<u8> {
        let mut raw = Self::header(self.slot, self.sub, self.offset, self.total).to_vec();
        raw.extend_from_slice(&self.data);
        pack(&raw)
    }
}

/// Split `data` into `CHUNK_BYTES` write frames for `cmd` (`0x58` NAM data, `0x10` IR data).
pub fn data_frames(cmd: Cmd, slot: u8, sub: u8, data: &[u8]) -> Vec<Vec<u8>> {
    let total = data.len() as u32;
    data.chunks(CHUNK_BYTES)
        .enumerate()
        .map(|(i, part)| {
            let c = Chunk { slot, sub, offset: (i * CHUNK_BYTES) as u32, total, data: part.to_vec() };
            frame(cmd, Dir::Write, &c.to_packed())
        })
        .collect()
}

/// Model data as the little-endian bytes the pedal receives.
pub fn floats_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub fn bytes_to_floats(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().expect("4 bytes"))).collect()
}

/// Collects the data chunks of one upload (in any order) back into its floats.
#[derive(Clone, Debug, Default)]
pub struct Reassembler {
    buf: Vec<u8>,
    filled: usize,
    slot: Option<u8>,
}

impl Reassembler {
    /// Add a data chunk. Returns the slot and floats once every byte has arrived.
    pub fn push(&mut self, c: &Chunk) -> Option<(u8, Vec<f32>)> {
        let (start, total) = (c.offset as usize, c.total as usize);
        // a chunk at offset 0, or for another slot/size, starts a new upload
        if start == 0 || self.slot != Some(c.slot) || self.buf.len() != total {
            self.buf = vec![0; total];
            self.filled = 0;
            self.slot = Some(c.slot);
        }
        let end = (start + c.data.len()).min(total);
        if start >= end {
            return None;
        }
        self.buf[start..end].copy_from_slice(&c.data[..end - start]);
        self.filled += end - start;
        (self.filled >= total).then(|| {
            self.filled = 0;
            (c.slot, bytes_to_floats(&self.buf))
        })
    }
}

pub const AMP_INFO_LEN: usize = 128;
const AMP_NAME_LEN: usize = 32;
const AMP_FACTOR_AT: usize = 52;

/// The 128-byte user AMP slot-info record (`0x58` sub 3 write, sub 4 reply), kept whole so
/// unknown bytes survive.
///
/// Known: bytes 0–31 name (ASCII, zero-padded), 32–35 `u32` = 256 in every record seen,
/// 52–55 `f32` factor (1.0 in both captured imports). An empty slot reads back as
/// `"USER AMP NN"` with `1.0` in each of the eleven floats at 52–95.
#[derive(Clone, Debug, PartialEq)]
pub struct UserAmpInfo {
    pub slot: u8,
    pub name: String,
    pub factor: f32,
    pub raw: [u8; AMP_INFO_LEN],
}

impl UserAmpInfo {
    /// The record the official app writes for a fresh import.
    pub fn new(slot: u8, name: &str, factor: f32) -> Self {
        let mut raw = [0u8; AMP_INFO_LEN];
        raw[32..36].copy_from_slice(&256u32.to_le_bytes());
        UserAmpInfo { slot, name: name.to_owned(), factor, raw }
    }

    /// Decode a sub 3 write or sub 4 reply body.
    pub fn parse(payload: &[u8]) -> Option<Self> {
        let c = Chunk::parse(payload)?;
        if c.sub != amp_sub::WRITE_INFO && c.sub != amp_sub::READ_INFO {
            return None;
        }
        let raw: [u8; AMP_INFO_LEN] = c.data.get(..AMP_INFO_LEN)?.try_into().ok()?;
        Some(UserAmpInfo {
            slot: c.slot,
            name: crate::records::ascii(&raw[..AMP_NAME_LEN]),
            factor: f32::from_le_bytes(raw[AMP_FACTOR_AT..AMP_FACTOR_AT + 4].try_into().ok()?),
            raw,
        })
    }

    pub fn to_raw(&self) -> [u8; AMP_INFO_LEN] {
        let mut raw = self.raw;
        raw[..AMP_NAME_LEN].copy_from_slice(&name_field::<AMP_NAME_LEN>(&self.name));
        raw[AMP_FACTOR_AT..AMP_FACTOR_AT + 4].copy_from_slice(&self.factor.to_le_bytes());
        raw
    }

    /// The sub 3 write frame.
    pub fn write_frame(&self) -> Vec<u8> {
        let c = Chunk {
            slot: self.slot,
            sub: amp_sub::WRITE_INFO,
            offset: 0,
            total: AMP_INFO_LEN as u32,
            data: self.to_raw().to_vec(),
        };
        frame(Cmd::UserAmp, Dir::Write, &c.to_packed())
    }
}

/// A name as an `N`-byte zero-padded ASCII field (non-ASCII becomes `?`, one byte kept for NUL).
fn name_field<const N: usize>(name: &str) -> [u8; N] {
    let mut b = [0u8; N];
    for (d, c) in b.iter_mut().take(N - 1).zip(name.chars()) {
        *d = if (' '..='~').contains(&c) { c as u8 } else { b'?' };
    }
    b
}

/// The official app's user AMP import: slot-info record, then the model data. The app selects
/// the slot's amp model (`host::head(Module::Amp, 30 + slot, true)`) just before.
pub fn user_amp_upload(info: &UserAmpInfo, data: &[f32]) -> Vec<Vec<u8>> {
    let mut v = vec![info.write_frame()];
    v.extend(data_frames(Cmd::UserAmp, info.slot, amp_sub::DATA, &floats_to_bytes(data)));
    v
}

/// Delete a user AMP slot (the app re-reads its info afterwards).
pub fn user_amp_delete(slot: u8) -> Vec<u8> {
    let c = Chunk { slot, sub: amp_sub::DELETE, offset: 0, total: 0, data: vec![] };
    frame(Cmd::UserAmp, Dir::Write, &c.to_packed())
}

const IR_NAME_LEN: usize = 32;

/// The `0x09` user IR name write: `<slot> <packed: 01 00 00 00, 32-byte name>`. The first field
/// was `1` after an upload (meaning unknown, docs/09).
pub fn user_ir_name(slot: u8, name: &str) -> Vec<u8> {
    let mut raw = vec![1, 0, 0, 0];
    raw.extend_from_slice(&name_field::<IR_NAME_LEN>(name));
    let mut p = vec![slot & 0x7F];
    p.extend(pack(&raw));
    frame(Cmd::IrSlot, Dir::Write, &p)
}

/// The official app's user IR import: the data, then the name. Only slot 0 has been captured;
/// the slot in the data header is assumed to work like the NAM upload's. The app selects the
/// CAB page and the slot's cab model (`host::head(Module::Cab, 30 + slot, true)`) just before.
pub fn user_ir_upload(slot: u8, name: &str, data: &[f32]) -> Vec<Vec<u8>> {
    let mut v = data_frames(Cmd::IrData, slot, IR_DATA_SUB, &floats_to_bytes(data));
    v.push(user_ir_name(slot, name));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::{Msg, hex};

    #[test]
    fn chunking() {
        let data: Vec<f32> = (0..UPLOAD_FLOATS).map(|i| i as f32 * 0.5).collect();
        let frames = data_frames(Cmd::UserAmp, 2, amp_sub::DATA, &floats_to_bytes(&data));
        assert_eq!(frames.len(), 61);
        let mut r = Reassembler::default();
        let mut out = None;
        for f in &frames {
            let Msg::Nep { cmd: 0x58, dir: 1, payload } = Msg::parse(f) else { panic!() };
            let c = Chunk::parse(&payload).unwrap();
            assert_eq!((c.slot, c.sub, c.total), (2, 1, 8192));
            out = r.push(&c);
        }
        assert_eq!(out, Some((2, data)));
        // last chunk: offset 8160, 32 bytes
        let Msg::Nep { payload, .. } = Msg::parse(frames.last().unwrap()) else { panic!() };
        let c = Chunk::parse(&payload).unwrap();
        assert_eq!((c.offset, c.data.len()), (8160, 32));
    }

    #[test]
    fn info_round_trip() {
        let i = UserAmpInfo::new(2, "Jcm800", 1.0);
        let Msg::Nep { payload, .. } = Msg::parse(&i.write_frame()) else { panic!() };
        let p = UserAmpInfo::parse(&payload).unwrap();
        assert_eq!(p, UserAmpInfo { raw: i.to_raw(), ..i });
        assert_eq!(hex(&user_amp_delete(0)), "F0 00 21 7A 70 58 01 00 00 18 00 00 00 00 00 00 00 00 00 00 00 F7");
    }
}

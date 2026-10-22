//! `open_stomp.upload`: user AMP (NAM) and user IR upload framing (docs/05).

use open_stomp::midi::Cmd;
use open_stomp::upload as up;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::util::{bytes, bytes_arg, bytes_list};

pub fn err(e: up::UploadError) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// One upload chunk (or header-only request/ack), unpacked.
#[pyclass(frozen, module = "open_stomp.upload")]
pub struct Chunk(up::Chunk);

#[pymethods]
impl Chunk {
    /// Decode a packed `0x58`/`0x10` body (the bytes after `<dir>`, e.g. `Msg.payload`).
    #[staticmethod]
    fn parse(#[pyo3(from_py_with = bytes_arg)] payload: Vec<u8>) -> Option<Chunk> {
        up::Chunk::parse(&payload).map(Chunk)
    }

    #[getter]
    fn slot(&self) -> u8 {
        self.0.slot
    }

    /// Header byte 0: the user IR slot in IR data chunks (0 in NAM uploads).
    #[getter]
    fn ir_slot(&self) -> u8 {
        self.0.ir_slot
    }

    /// 0x58: 1 data, 3 write info, 4 read info, 6 delete. 0x10: 0 data.
    #[getter]
    fn sub(&self) -> u8 {
        self.0.sub
    }

    #[getter]
    fn offset(&self) -> u32 {
        self.0.offset
    }

    #[getter]
    fn total(&self) -> u32 {
        self.0.total
    }

    #[getter]
    fn data<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        bytes(py, &self.0.data)
    }

    fn __repr__(&self) -> String {
        let c = &self.0;
        format!("Chunk(ir_slot={}, slot={}, sub={}, offset={}, total={}, data=<{} bytes>)", c.ir_slot, c.slot, c.sub, c.offset, c.total, c.data.len())
    }
}

/// Collects one upload's data chunks back into its floats.
#[pyclass(module = "open_stomp.upload")]
#[derive(Default)]
pub struct Reassembler(up::Reassembler);

#[pymethods]
impl Reassembler {
    #[new]
    fn new() -> Self {
        Self::default()
    }

    /// Add a data chunk; returns `(slot, floats)` once the upload is complete.
    fn push(&mut self, chunk: PyRef<'_, Chunk>) -> Option<(u8, Vec<f32>)> {
        self.0.push(&chunk.0)
    }
}

/// The 128-byte user AMP slot-info record (name, factor; unknown bytes kept).
#[pyclass(from_py_object, module = "open_stomp.upload")]
#[derive(Clone)]
pub struct UserAmpInfo(pub up::UserAmpInfo);

#[pymethods]
impl UserAmpInfo {
    /// The record the official app writes for a fresh import.
    #[new]
    #[pyo3(signature = (slot, name, factor = 1.0))]
    fn new(slot: u8, name: &str, factor: f32) -> Self {
        UserAmpInfo(up::UserAmpInfo::new(slot, name, factor))
    }

    /// Decode a sub 3 write or sub 4 reply body; `None` if it isn't one.
    #[staticmethod]
    fn parse(#[pyo3(from_py_with = bytes_arg)] payload: Vec<u8>) -> Option<UserAmpInfo> {
        up::UserAmpInfo::parse(&payload).map(UserAmpInfo)
    }

    #[getter]
    fn slot(&self) -> u8 {
        self.0.slot
    }

    #[setter]
    fn set_slot(&mut self, v: u8) {
        self.0.slot = v;
    }

    #[getter]
    fn name(&self) -> &str {
        &self.0.name
    }

    /// Up to 31 printable ASCII characters are stored; others become `?`.
    #[setter]
    fn set_name(&mut self, v: String) {
        self.0.name = v;
    }

    #[getter]
    fn factor(&self) -> f32 {
        self.0.factor
    }

    #[setter]
    fn set_factor(&mut self, v: f32) {
        self.0.factor = v;
    }

    #[getter]
    fn raw<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        bytes(py, &self.0.raw)
    }

    fn to_raw<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        bytes(py, &self.0.to_raw())
    }

    /// The sub 3 write frame.
    fn write_frame<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        bytes(py, &self.0.write_frame())
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.0 == other.0
    }

    fn __repr__(&self) -> String {
        format!("UserAmpInfo(slot={}, name={:?}, factor={})", self.0.slot, self.0.name, self.0.factor)
    }
}

/// Raise `ValueError` unless `slot` is 0–9 and `data` is 2048 finite floats.
#[pyfunction]
fn check(slot: u8, data: Vec<f32>) -> PyResult<()> {
    up::check(slot, &data).map_err(err)
}

/// Frames of a user AMP import: the slot-info record, then the data chunks.
#[pyfunction]
fn user_amp_upload<'py>(py: Python<'py>, info: PyRef<'_, UserAmpInfo>, data: Vec<f32>) -> Vec<Bound<'py, PyBytes>> {
    bytes_list(py, &up::user_amp_upload(&info.0, &data))
}

#[pyfunction]
fn user_amp_delete<'py>(py: Python<'py>, slot: u8) -> Bound<'py, PyBytes> {
    bytes(py, &up::user_amp_delete(slot))
}

/// Frames of a user IR import: the data chunks, then the `0x09` name write.
#[pyfunction]
fn user_ir_upload<'py>(py: Python<'py>, slot: u8, name: &str, data: Vec<f32>) -> Vec<Bound<'py, PyBytes>> {
    bytes_list(py, &up::user_ir_upload(slot, name, &data))
}

#[pyfunction]
fn user_ir_name<'py>(py: Python<'py>, slot: u8, name: &str) -> Bound<'py, PyBytes> {
    bytes(py, &up::user_ir_name(slot, name))
}

/// Split raw bytes into chunk write frames for `cmd` (0x58 NAM data, 0x10 IR data).
#[pyfunction]
fn data_frames<'py>(py: Python<'py>, cmd: u8, slot: u8, sub: u8, #[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> PyResult<Vec<Bound<'py, PyBytes>>> {
    let cmd = match Cmd::from_u8(cmd) {
        Some(c @ (Cmd::UserAmp | Cmd::IrData)) => c,
        _ => return Err(PyValueError::new_err("cmd must be 0x58 or 0x10")),
    };
    Ok(bytes_list(py, &up::data_frames(cmd, slot, sub, &data)))
}

#[pyfunction]
fn floats_to_bytes<'py>(py: Python<'py>, data: Vec<f32>) -> Bound<'py, PyBytes> {
    bytes(py, &up::floats_to_bytes(&data))
}

#[pyfunction]
fn bytes_to_floats(#[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> Vec<f32> {
    up::bytes_to_floats(&data)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Chunk>()?;
    m.add_class::<Reassembler>()?;
    m.add_class::<UserAmpInfo>()?;
    for f in [
        wrap_pyfunction!(check, m)?,
        wrap_pyfunction!(user_amp_upload, m)?,
        wrap_pyfunction!(user_amp_delete, m)?,
        wrap_pyfunction!(user_ir_upload, m)?,
        wrap_pyfunction!(user_ir_name, m)?,
        wrap_pyfunction!(data_frames, m)?,
        wrap_pyfunction!(floats_to_bytes, m)?,
        wrap_pyfunction!(bytes_to_floats, m)?,
    ] {
        m.add_function(f)?;
    }
    m.add("CHUNK_BYTES", up::CHUNK_BYTES)?;
    m.add("UPLOAD_FLOATS", up::UPLOAD_FLOATS)?;
    m.add("USER_SLOTS", up::USER_SLOTS)?;
    m.add("USER_MODEL_BASE", up::USER_MODEL_BASE)?;
    m.add("AMP_INFO_LEN", up::AMP_INFO_LEN)?;
    m.add("IR_DATA_SUB", up::IR_DATA_SUB)?;
    m.add("AMP_SUB_DATA", up::amp_sub::DATA)?;
    m.add("AMP_SUB_WRITE_INFO", up::amp_sub::WRITE_INFO)?;
    m.add("AMP_SUB_READ_INFO", up::amp_sub::READ_INFO)?;
    m.add("AMP_SUB_DELETE", up::amp_sub::DELETE)?;
    Ok(())
}

//! Conversions shared by the binding modules.

use std::time::{SystemTime, UNIX_EPOCH};

use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes};

/// Accept `bytes`, `bytearray`, `memoryview`-free sequences of ints (`[0xB1, 0x12, 64]`).
pub fn bytes_arg(obj: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(b) = obj.cast::<PyBytes>() {
        return Ok(b.as_bytes().to_vec());
    }
    if let Ok(b) = obj.cast::<PyByteArray>() {
        return Ok(b.to_vec());
    }
    obj.extract::<Vec<u8>>()
}

pub fn bytes<'py>(py: Python<'py>, b: &[u8]) -> Bound<'py, PyBytes> {
    PyBytes::new(py, b)
}

pub fn bytes_list<'py>(py: Python<'py>, v: &[Vec<u8>]) -> Vec<Bound<'py, PyBytes>> {
    v.iter().map(|b| PyBytes::new(py, b)).collect()
}

/// Seconds since the Unix epoch, like `time.time()`.
pub fn unix_seconds(t: SystemTime) -> f64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

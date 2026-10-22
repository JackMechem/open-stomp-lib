//! `open_stomp.ir`: read IR audio files and convert them for the pedal's user CAB (IR) slots.

use open_stomp::ir as i;
use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;

pub fn err(e: Box<dyn std::error::Error + Send + Sync>) -> PyErr {
    match e.downcast::<std::io::Error>() {
        Ok(io) => PyOSError::new_err(io.to_string()),
        Err(other) => PyValueError::new_err(other.to_string()),
    }
}

/// An IR converted for upload: `data` is what the pedal gets.
#[pyclass(frozen, get_all, module = "open_stomp.ir")]
pub struct ConvertedIr {
    /// The 2048 floats to upload (reversed, scaled).
    data: Vec<f32>,
    /// The gain applied (provisional, see docs/09).
    gain: f32,
    /// The IR at 48 kHz (first channel), before truncation.
    ir: Vec<f32>,
}

impl From<i::ConvertedIr> for ConvertedIr {
    fn from(c: i::ConvertedIr) -> Self {
        ConvertedIr { data: c.data, gain: c.gain, ir: c.ir }
    }
}

#[pymethods]
impl ConvertedIr {
    fn __repr__(&self) -> String {
        format!("ConvertedIr(data=<{} floats>, gain={})", self.data.len(), self.gain)
    }
}

/// Read a WAV file: `(sample_rate, channels)`, one list of floats (±1.0) per channel.
#[pyfunction]
fn load_wav(path: std::path::PathBuf) -> PyResult<(u32, Vec<Vec<f32>>)> {
    let w = i::Wav::load(path).map_err(err)?;
    Ok((w.sample_rate, w.channels))
}

/// Write a WAV file (24-bit PCM, or 32-bit float with `float=True`).
#[pyfunction]
#[pyo3(signature = (path, sample_rate, channels, float = false))]
fn save_wav(path: std::path::PathBuf, sample_rate: u32, channels: Vec<Vec<f32>>, float: bool) -> PyResult<()> {
    let bytes = i::Wav { sample_rate, channels }.to_bytes(if float { 32 } else { 24 }, float);
    std::fs::write(path, bytes).map_err(|e| PyOSError::new_err(e.to_string()))
}

/// Convert an IR WAV file (any sample rate; first channel) like the official app.
#[pyfunction]
fn convert(path: std::path::PathBuf) -> PyResult<ConvertedIr> {
    Ok(i::convert(&i::Wav::load(path).map_err(err)?).into())
}

/// Resample samples from `src` Hz to `dst` Hz (windowed sinc).
#[pyfunction]
fn resample(samples: Vec<f32>, src: u32, dst: u32) -> Vec<f32> {
    i::resample(&samples, src, dst)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ConvertedIr>()?;
    m.add_function(wrap_pyfunction!(load_wav, m)?)?;
    m.add_function(wrap_pyfunction!(save_wav, m)?)?;
    m.add_function(wrap_pyfunction!(convert, m)?)?;
    m.add_function(wrap_pyfunction!(resample, m)?)?;
    m.add("IR_TAPS", i::IR_TAPS)?;
    m.add("SAMPLE_RATE", i::IR_SAMPLE_RATE)?;
    Ok(())
}

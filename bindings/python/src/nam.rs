//! `open_stomp.nam`: load and run `.nam` models, and convert them for the pedal.

use open_stomp::nam as n;
use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;

pub fn err(e: n::NamError) -> PyErr {
    match e {
        n::NamError::Io(e) => PyOSError::new_err(e.to_string()),
        other => PyValueError::new_err(other.to_string()),
    }
}

/// A loaded Neural Amp Modeler model (mono, at its sample rate, normally 48 kHz).
#[pyclass(module = "open_stomp.nam")]
pub struct Model(pub n::Model);

#[pymethods]
impl Model {
    #[staticmethod]
    fn load(path: std::path::PathBuf) -> PyResult<Model> {
        n::Model::load(path).map(Model).map_err(err)
    }

    #[staticmethod]
    fn from_json(text: &str) -> PyResult<Model> {
        n::Model::from_json_str(text).map(Model).map_err(err)
    }

    /// Process samples (state carries over between calls); returns the output.
    fn process(&mut self, py: Python<'_>, samples: Vec<f32>) -> Vec<f32> {
        let mut out = vec![0.0; samples.len()];
        let m = &mut self.0;
        py.detach(|| m.process(&samples, &mut out));
        out
    }

    /// Back to the state after loading.
    fn reset(&mut self) {
        self.0.reset();
    }

    #[getter]
    fn name(&self) -> Option<&str> {
        self.0.name.as_deref()
    }

    #[getter]
    fn architecture(&self) -> &str {
        &self.0.architecture
    }

    #[getter]
    fn sample_rate(&self) -> Option<f64> {
        self.0.sample_rate()
    }

    fn __repr__(&self) -> String {
        format!("Model(name={:?}, architecture={:?})", self.0.name, self.0.architecture)
    }
}

/// The result of a conversion: `data` is what the pedal gets.
#[pyclass(frozen, get_all, module = "open_stomp.nam")]
pub struct Converted {
    /// The 2048 floats to upload (`g1 + g2`).
    data: Vec<f32>,
    /// Slot-info factor (always 1.0).
    factor: f32,
    levels: Vec<f64>,
    levels3: Vec<f64>,
    bla_db: Vec<f64>,
    g1_gains_db: Vec<f64>,
    g1_ir: Vec<f64>,
    g2_ir: Vec<f64>,
}


impl From<n::Converted> for Converted {
    fn from(c: n::Converted) -> Self {
        Converted {
            data: c.data,
            factor: c.factor,
            levels: c.levels,
            levels3: c.levels3,
            bla_db: c.bla_db,
            g1_gains_db: c.g1_gains_db,
            g1_ir: c.g1_ir,
            g2_ir: c.g2_ir,
        }
    }
}

#[pymethods]
impl Converted {
    fn __repr__(&self) -> String {
        format!("Converted(data=<{} floats>, factor={})", self.data.len(), self.factor)
    }
}

/// Convert a model (a `Model`, or a path to a `.nam` file) into the pedal's user AMP data.
/// `excitation`: 8192 samples to use instead of ours (e.g. the official app's `BLA_x`).
/// `measure`: `"app"` (the official algorithm, default), `"clean"` (steady-state small-signal
/// measurement, no DC-blocker transient) or `"optimised"` (clean, then fitted with a simulation of
/// the pedal; about a minute). Releases the GIL; takes a few seconds.
#[pyfunction]
#[pyo3(signature = (model, excitation = None, measure = "app"))]
fn convert(py: Python<'_>, model: &Bound<'_, PyAny>, excitation: Option<Vec<f32>>, measure: &str) -> PyResult<Converted> {
    let measure = parse_measure(measure)?;
    let mut owned;
    let mut borrowed;
    let m: &mut n::Model = if let Ok(m) = model.cast::<Model>() {
        borrowed = m.borrow_mut();
        &mut borrowed.0
    } else {
        let path: std::path::PathBuf = model.extract()?;
        owned = n::Model::load(path).map_err(err)?;
        &mut owned
    };
    if let Some(x) = &excitation
        && x.len() != n::convert::N
    {
        return Err(PyValueError::new_err(format!("excitation must be {} samples", n::convert::N)));
    }
    let x = excitation.unwrap_or_else(n::convert::excitation);
    let c = py.detach(|| n::convert::convert_measured(m, &x, measure));
    Ok(c.into())
}

/// `"app"`, `"clean"` or `"optimised"` → [`n::convert::Measure`].
pub fn parse_measure(measure: &str) -> PyResult<n::convert::Measure> {
    Ok(match measure {
        "app" => n::convert::Measure::App,
        "clean" => n::convert::Measure::Clean,
        "optimised" => n::convert::Measure::Optimised,
        other => {
            return Err(PyValueError::new_err(format!(
                "measure must be \"app\", \"clean\" or \"optimised\", not {other:?}"
            )));
        }
    })
}

/// The official app's BLA excitation, read from an installed Stomp Manager (`path`: the app bundle,
/// install folder or library file; `None` searches the standard install locations). With it,
/// `convert(model, excitation=...)` matches the official app's conversion exactly.
#[pyfunction]
#[pyo3(signature = (path = None))]
fn official_excitation(path: Option<std::path::PathBuf>) -> PyResult<Vec<f32>> {
    match path {
        Some(p) => n::convert::official_excitation(p).map_err(err),
        None => n::convert::find_official_excitation()
            .ok_or_else(|| PyValueError::new_err("Stomp Manager not found in the standard install locations")),
    }
}

/// Our BLA excitation (8192 samples, pink multisine at about -90 dBFS).
#[pyfunction]
fn excitation() -> Vec<f32> {
    n::convert::excitation()
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Model>()?;
    m.add_class::<Converted>()?;
    m.add_function(wrap_pyfunction!(convert, m)?)?;
    m.add_function(wrap_pyfunction!(excitation, m)?)?;
    m.add_function(wrap_pyfunction!(official_excitation, m)?)?;
    m.add("TONE_BINS", n::convert::TONE_BINS.to_vec())?;
    m.add("FFT_SIZE", n::convert::N)?;
    Ok(())
}

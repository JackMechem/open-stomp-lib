//! Neural Amp Modeler (`.nam`) models (feature `nam`): loading and inference.
//!
//! A pure-Rust port of the parts of [NeuralAmpModelerCore](https://github.com/sdatkinson/NeuralAmpModelerCore)
//! (MIT) that the pedal's converter needs: WaveNet models, plain or inside a `SlimmableContainer`
//! (NAM 0.5 "classic" and 0.7 files). The converter (docs/05) runs test signals through the model,
//! so this must match NeuralAmpModelerCore numerically; `tests/nam_inference.rs` checks it against
//! the vendor converter's recorded model output where available.
//!
//! ```no_run
//! # fn main() -> Result<(), open_stomp::nam::NamError> {
//! let mut model = open_stomp::nam::Model::load("amp.nam")?;
//! let input = vec![0.0f32; 512];
//! let mut output = vec![0.0f32; 512];
//! model.process(&input, &mut output);
//! # Ok(()) }
//! ```

mod activation;
pub mod convert;
mod wavenet;

use std::path::Path;

use serde_json::Value;

pub use activation::Activation;
pub use convert::{Converted, convert};
pub use wavenet::WaveNet;

#[derive(Debug)]
pub enum NamError {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// The file is not a valid model.
    Format(String),
    /// A valid model using a feature this port doesn't implement.
    Unsupported(String),
    /// The weight list doesn't fit the architecture.
    Weights { expected: usize, got: usize },
}

impl std::fmt::Display for NamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NamError::Io(e) => write!(f, "{e}"),
            NamError::Json(e) => write!(f, "invalid JSON: {e}"),
            NamError::Format(s) => write!(f, "invalid model: {s}"),
            NamError::Unsupported(s) => write!(f, "unsupported model feature: {s}"),
            NamError::Weights { expected, got } => write!(f, "weight count mismatch: model needs {expected}, file has {got}"),
        }
    }
}

impl std::error::Error for NamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            NamError::Io(e) => Some(e),
            NamError::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for NamError {
    fn from(e: std::io::Error) -> Self {
        NamError::Io(e)
    }
}

impl From<serde_json::Error> for NamError {
    fn from(e: serde_json::Error) -> Self {
        NamError::Json(e)
    }
}

/// A loaded model, ready to process audio (mono, at its sample rate, normally 48 kHz).
#[derive(Clone, Debug)]
pub struct Model {
    net: WaveNet,
    /// Display name from the file's metadata, if any.
    pub name: Option<String>,
    /// The architecture as written in the file (`"WaveNet"`, `"SlimmableContainer"`).
    pub architecture: String,
}

fn weights_of(v: &Value) -> Result<Vec<f32>, NamError> {
    match v.get("weights") {
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| x.as_f64().map(|f| f as f32).ok_or_else(|| NamError::Format("non-numeric weight".into())))
            .collect(),
        None => Ok(vec![]),
        _ => Err(NamError::Format("weights must be an array".into())),
    }
}

fn build(v: &Value) -> Result<WaveNet, NamError> {
    let arch = v.get("architecture").and_then(Value::as_str).unwrap_or("");
    let config = v.get("config").ok_or_else(|| NamError::Format("missing config".into()))?;
    let sample_rate = v.get("sample_rate").and_then(Value::as_f64);
    match arch {
        "WaveNet" => WaveNet::from_json(config, &weights_of(v)?, sample_rate),
        "SlimmableContainer" => {
            // NeuralAmpModelerCore's ContainerModel defaults to the last (full-size) submodel.
            let subs = config
                .get("submodels")
                .and_then(Value::as_array)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| NamError::Format("SlimmableContainer without submodels".into()))?;
            let last = subs.last().and_then(|s| s.get("model")).ok_or_else(|| NamError::Format("submodel".into()))?;
            build(last)
        }
        other => Err(NamError::Unsupported(format!("architecture {other:?}"))),
    }
}

impl Model {
    pub fn load(path: impl AsRef<Path>) -> Result<Model, NamError> {
        Model::from_json_str(&std::fs::read_to_string(path)?)
    }

    pub fn from_json_str(s: &str) -> Result<Model, NamError> {
        let v: Value = serde_json::from_str(s)?;
        let net = build(&v)?;
        let name = v.get("metadata").and_then(|m| m.get("name")).and_then(Value::as_str).map(str::to_owned);
        let architecture = v.get("architecture").and_then(Value::as_str).unwrap_or("").to_owned();
        Ok(Model { net, name, architecture })
    }

    /// Process a block (any length; state carries over between calls).
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        for (i, o) in input.chunks(4096).zip(output.chunks_mut(4096)) {
            self.net.process(i, o);
        }
    }

    /// Back to the state after loading (history cleared, then prewarmed with silence).
    pub fn reset(&mut self) {
        self.net.reset();
    }

    /// The sample rate the model was trained at, if the file says.
    pub fn sample_rate(&self) -> Option<f64> {
        self.net.sample_rate()
    }
}

//! Activation functions, as NeuralAmpModelerCore defines them (`NAM/activations.h`).

use serde_json::Value;

use super::NamError;

#[derive(Clone, Debug, PartialEq)]
pub enum Activation {
    Tanh,
    Hardtanh,
    Fasttanh,
    ReLU,
    LeakyReLU(f32),
    /// One slope per channel (or a single slope for all).
    PReLU(Vec<f32>),
    Sigmoid,
    SiLU,
    Hardswish,
    LeakyHardtanh { min_val: f32, max_val: f32, min_slope: f32, max_slope: f32 },
    Softsign,
}

fn fast_tanh(x: f32) -> f32 {
    let ax = x.abs();
    let x2 = x * x;
    (x * (2.455_507_5 + 2.455_507_5 * ax + (0.893_229_85 + 0.821_226_7 * ax) * x2))
        / (2.445_066_3 + (2.445_066_3 + x2) * (x + 0.814_642_7 * x * ax).abs())
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

impl Activation {
    /// From a config entry: a name (`"Tanh"`) or an object (`{"type": "LeakyReLU", "negative_slope": 0.01}`).
    pub fn from_json(v: &Value) -> Result<Activation, NamError> {
        let (name, obj) = match v {
            Value::String(s) => (s.as_str(), None),
            Value::Object(o) => (o.get("type").and_then(Value::as_str).unwrap_or(""), Some(o)),
            _ => return Err(NamError::Unsupported(format!("activation {v}"))),
        };
        let num = |key: &str, default: f32| {
            obj.and_then(|o| o.get(key)).and_then(Value::as_f64).map_or(default, |x| x as f32)
        };
        Ok(match name {
            "Tanh" => Activation::Tanh,
            "Hardtanh" => Activation::Hardtanh,
            "Fasttanh" => Activation::Fasttanh,
            "ReLU" => Activation::ReLU,
            "LeakyReLU" => Activation::LeakyReLU(num("negative_slope", 0.01)),
            "PReLU" => {
                let slopes = obj
                    .and_then(|o| o.get("negative_slopes"))
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_f64).map(|x| x as f32).collect())
                    .unwrap_or_else(|| vec![num("negative_slope", 0.01)]);
                Activation::PReLU(slopes)
            }
            "Sigmoid" => Activation::Sigmoid,
            "SiLU" => Activation::SiLU,
            "Hardswish" => Activation::Hardswish,
            "LeakyHardtanh" | "LeakyHardTanh" => Activation::LeakyHardtanh {
                min_val: num("min_val", -1.0),
                max_val: num("max_val", 1.0),
                min_slope: num("min_slope", 0.01),
                max_slope: num("max_slope", 0.01),
            },
            "Softsign" => Activation::Softsign,
            other => return Err(NamError::Unsupported(format!("activation {other:?}"))),
        })
    }

    /// Apply in place to frame-major data with `channels` values per frame.
    pub fn apply(&self, data: &mut [f32], channels: usize) {
        match self {
            Activation::Tanh => data.iter_mut().for_each(|x| *x = x.tanh()),
            Activation::Hardtanh => data.iter_mut().for_each(|x| *x = x.clamp(-1.0, 1.0)),
            Activation::Fasttanh => data.iter_mut().for_each(|x| *x = fast_tanh(*x)),
            Activation::ReLU => data.iter_mut().for_each(|x| *x = if *x > 0.0 { *x } else { 0.0 }),
            Activation::LeakyReLU(s) => data.iter_mut().for_each(|x| *x = if *x > 0.0 { *x } else { s * *x }),
            Activation::PReLU(slopes) => {
                for (i, x) in data.iter_mut().enumerate() {
                    let s = slopes[if slopes.len() == 1 { 0 } else { i % channels }];
                    *x = if *x > 0.0 { *x } else { s * *x };
                }
            }
            Activation::Sigmoid => data.iter_mut().for_each(|x| *x = sigmoid(*x)),
            Activation::SiLU => data.iter_mut().for_each(|x| *x *= sigmoid(*x)),
            Activation::Hardswish => data.iter_mut().for_each(|x| *x = *x * (*x + 3.0).clamp(0.0, 6.0) / 6.0),
            Activation::LeakyHardtanh { min_val, max_val, min_slope, max_slope } => data.iter_mut().for_each(|x| {
                *x = if *x < *min_val {
                    (*x - min_val) * min_slope + min_val
                } else if *x > *max_val {
                    (*x - max_val) * max_slope + max_val
                } else {
                    *x
                }
            }),
            Activation::Softsign => data.iter_mut().for_each(|x| *x /= 1.0 + x.abs()),
        }
    }

    /// Apply to one value of channel `ch`.
    pub fn apply_one(&self, x: f32, ch: usize) -> f32 {
        let mut v = [x];
        match self {
            Activation::PReLU(slopes) => {
                let s = slopes[if slopes.len() == 1 { 0 } else { ch }];
                if x > 0.0 { x } else { s * x }
            }
            _ => {
                self.apply(&mut v, 1);
                v[0]
            }
        }
    }
}

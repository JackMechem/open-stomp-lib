//! NAM WaveNet inference, ported from NeuralAmpModelerCore (`NAM/wavenet/model.cpp`, MIT).
//!
//! Supported: any number of layer arrays; per-layer kernel sizes, dilations and activations;
//! gating `none` and `gated` (including the legacy `"gated": true`); bottleneck; `layer1x1` and
//! `head1x1`; heads with a kernel (`"head": {...}`) or the legacy `head_size`/`head_bias`.
//! Not supported (refused with an error): FiLM, grouped convolutions, blended gating, a
//! post-stack head and a condition DSP.
//!
//! Data is frame-major: `x[t * channels + c]`.

use serde_json::Value;

use super::NamError;
use super::activation::Activation;

/// Pull `n` weights from the flat weight list.
struct Weights<'a> {
    w: &'a [f32],
    pos: usize,
}

impl Weights<'_> {
    fn take(&mut self, n: usize) -> Result<&[f32], NamError> {
        let s = self.w.get(self.pos..self.pos + n).ok_or(NamError::Weights { expected: self.pos + n, got: self.w.len() })?;
        self.pos += n;
        Ok(s)
    }
}

/// A causal dilated 1-D convolution with its own input history.
#[derive(Clone, Debug)]
struct Conv1d {
    cin: usize,
    cout: usize,
    kernel: usize,
    dilation: usize,
    /// `w[(k * cout + o) * cin + i]`; tap `k = kernel - 1` is the current sample.
    w: Vec<f32>,
    bias: Option<Vec<f32>>,
    /// The last `(kernel - 1) * dilation` input frames.
    history: Vec<f32>,
    ext: Vec<f32>,
}

impl Conv1d {
    fn new(cin: usize, cout: usize, kernel: usize, dilation: usize, bias: bool) -> Self {
        Conv1d {
            cin,
            cout,
            kernel,
            dilation,
            w: vec![0.0; kernel * cout * cin],
            bias: bias.then(|| vec![0.0; cout]),
            history: vec![0.0; (kernel - 1) * dilation * cin],
            ext: vec![],
        }
    }

    /// NeuralAmpModelerCore's flattening: for out, for in, for tap; then the bias.
    fn load(&mut self, w: &mut Weights) -> Result<(), NamError> {
        let src = w.take(self.cout * self.cin * self.kernel)?;
        let mut it = src.iter();
        for o in 0..self.cout {
            for i in 0..self.cin {
                for k in 0..self.kernel {
                    self.w[(k * self.cout + o) * self.cin + i] = *it.next().expect("sized");
                }
            }
        }
        if let Some(b) = &mut self.bias {
            let n = b.len();
            b.copy_from_slice(w.take(n)?);
        }
        Ok(())
    }

    fn receptive_field(&self) -> usize {
        (self.kernel - 1) * self.dilation
    }

    /// `out` gets `n` frames of `cout` values.
    fn process(&mut self, input: &[f32], n: usize, out: &mut Vec<f32>) {
        let (cin, cout, h) = (self.cin, self.cout, self.history.len() / self.cin.max(1));
        self.ext.clear();
        self.ext.extend_from_slice(&self.history);
        self.ext.extend_from_slice(&input[..n * cin]);
        out.clear();
        out.resize(n * cout, 0.0);
        for t in 0..n {
            let o_row = &mut out[t * cout..(t + 1) * cout];
            if let Some(b) = &self.bias {
                o_row.copy_from_slice(b);
            }
            for k in 0..self.kernel {
                // frame in ext: h + t - (kernel - 1 - k) * dilation
                let src_t = h + t - (self.kernel - 1 - k) * self.dilation;
                let x = &self.ext[src_t * cin..(src_t + 1) * cin];
                let wk = &self.w[k * cout * cin..(k + 1) * cout * cin];
                for (o, acc) in o_row.iter_mut().enumerate() {
                    let wr = &wk[o * cin..(o + 1) * cin];
                    *acc += wr.iter().zip(x).map(|(a, b)| a * b).sum::<f32>();
                }
            }
        }
        if h > 0 {
            let total = self.ext.len();
            self.history.copy_from_slice(&self.ext[total - h * cin..]);
        }
    }

    fn reset(&mut self) {
        self.history.iter_mut().for_each(|x| *x = 0.0);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gating {
    None,
    Gated,
}

#[derive(Clone, Debug)]
struct Layer {
    conv: Conv1d,
    mixin: Conv1d,
    activation: Activation,
    gating: Gating,
    secondary: Activation,
    bottleneck: usize,
    layer1x1: Option<Conv1d>,
    head1x1: Option<Conv1d>,
    // scratch
    z: Vec<f32>,
    mix: Vec<f32>,
    tmp: Vec<f32>,
}

impl Layer {
    /// `input`: n × channels; `cond`: n × condition_size. Writes the head contribution and the
    /// next layer's input.
    fn process(&mut self, input: &[f32], cond: &[f32], n: usize, head: &mut Vec<f32>, next: &mut Vec<f32>) {
        let zc = self.conv.cout;
        let b = self.bottleneck;
        self.conv.process(input, n, &mut self.z);
        self.mixin.process(cond, n, &mut self.mix);
        for (z, m) in self.z.iter_mut().zip(&self.mix) {
            *z += m;
        }
        // activated layer output: n × bottleneck
        let act: &mut Vec<f32> = &mut self.tmp;
        match self.gating {
            Gating::None => {
                self.activation.apply(&mut self.z, zc);
                act.clear();
                act.extend_from_slice(&self.z);
            }
            Gating::Gated => {
                act.clear();
                act.resize(n * b, 0.0);
                for t in 0..n {
                    for c in 0..b {
                        let p = self.activation.apply_one(self.z[t * zc + c], c);
                        let g = self.secondary.apply_one(self.z[t * zc + b + c], c);
                        act[t * b + c] = p * g;
                    }
                }
            }
        }
        match &mut self.head1x1 {
            Some(h) => h.process(act, n, head),
            None => {
                head.clear();
                head.extend_from_slice(act);
            }
        }
        let channels = self.conv.cin;
        next.clear();
        next.extend_from_slice(&input[..n * channels]);
        if let Some(l) = &mut self.layer1x1 {
            l.process(act, n, &mut self.mix);
            for (x, y) in next.iter_mut().zip(&self.mix) {
                *x += y;
            }
        }
    }

    fn reset(&mut self) {
        self.conv.reset();
    }
}

#[derive(Clone, Debug)]
struct LayerArray {
    rechannel: Conv1d,
    layers: Vec<Layer>,
    head_rechannel: Conv1d,
    head_in: usize,
    // scratch
    a: Vec<f32>,
    b: Vec<f32>,
    head_sum: Vec<f32>,
    head_part: Vec<f32>,
}

impl LayerArray {
    fn from_json(cfg: &Value) -> Result<LayerArray, NamError> {
        let int = |k: &str| cfg.get(k).and_then(Value::as_u64).map(|x| x as usize);
        let req = |k: &str| int(k).ok_or_else(|| NamError::Format(format!("layer array: missing {k}")));
        let input_size = req("input_size")?;
        let condition_size = req("condition_size")?;
        let channels = req("channels")?;
        let bottleneck = int("bottleneck").unwrap_or(channels);
        for key in ["groups_input", "groups_input_mixin"] {
            if int(key).is_some_and(|g| g != 1) {
                return Err(NamError::Unsupported(format!("{key} != 1")));
            }
        }
        for key in [
            "conv_pre_film",
            "conv_post_film",
            "input_mixin_pre_film",
            "input_mixin_post_film",
            "activation_pre_film",
            "activation_post_film",
            "layer1x1_post_film",
            "head1x1_post_film",
        ] {
            if cfg.get(key).and_then(|f| f.get("active")).and_then(Value::as_bool) == Some(true) {
                return Err(NamError::Unsupported(format!("FiLM ({key})")));
            }
        }
        let sub_active = |key: &str, default: bool| -> Result<(bool, usize), NamError> {
            match cfg.get(key) {
                Some(v) if !v.is_null() => {
                    if v.get("groups").and_then(Value::as_u64).is_some_and(|g| g != 1) {
                        return Err(NamError::Unsupported(format!("{key} groups != 1")));
                    }
                    let active = v.get("active").and_then(Value::as_bool).unwrap_or(default);
                    let out = v.get("out_channels").and_then(Value::as_u64).map_or(0, |x| x as usize);
                    Ok((active, out))
                }
                _ => Ok((default, 0)),
            }
        };
        let (layer1x1, _) = sub_active("layer1x1", true)?;
        let (head1x1, head1x1_out) = sub_active("head1x1", false)?;
        if !layer1x1 && bottleneck != channels {
            return Err(NamError::Format("bottleneck must equal channels without layer1x1".into()));
        }

        let (head_size, head_kernel, head_bias, head_dilation) = match cfg.get("head") {
            Some(h) if !h.is_null() => (
                h.get("out_channels").and_then(Value::as_u64).ok_or_else(|| NamError::Format("head.out_channels".into()))?
                    as usize,
                h.get("kernel_size").and_then(Value::as_u64).unwrap_or(1) as usize,
                h.get("bias").and_then(Value::as_bool).unwrap_or(false),
                h.get("head_dilation").and_then(Value::as_u64).unwrap_or(1) as usize,
            ),
            _ => (req("head_size")?, 1, cfg.get("head_bias").and_then(Value::as_bool).unwrap_or(false), 1),
        };

        let dilations: Vec<usize> = cfg
            .get("dilations")
            .and_then(Value::as_array)
            .ok_or_else(|| NamError::Format("layer array: missing dilations".into()))?
            .iter()
            .map(|d| d.as_u64().map(|x| x as usize).ok_or_else(|| NamError::Format("dilation".into())))
            .collect::<Result<_, _>>()?;
        let n = dilations.len();
        let kernels: Vec<usize> = match (cfg.get("kernel_sizes").and_then(Value::as_array), int("kernel_size")) {
            (Some(a), _) => a.iter().map(|k| k.as_u64().unwrap_or(1) as usize).collect(),
            (None, Some(k)) => vec![k; n],
            _ => return Err(NamError::Format("layer array: missing kernel_size(s)".into())),
        };
        let per_layer = |key: &str| -> Option<Vec<Value>> {
            match cfg.get(key) {
                Some(Value::Array(a)) => Some(a.clone()),
                Some(v) if !v.is_null() => Some(vec![v.clone(); n]),
                _ => None,
            }
        };
        let activations: Vec<Activation> = per_layer("activation")
            .ok_or_else(|| NamError::Format("layer array: missing activation".into()))?
            .iter()
            .map(Activation::from_json)
            .collect::<Result<_, _>>()?;
        let gatings: Vec<Gating> = match per_layer("gating_mode") {
            Some(v) => v
                .iter()
                .map(|g| match g.as_str() {
                    Some("none") => Ok(Gating::None),
                    Some("gated") => Ok(Gating::Gated),
                    other => Err(NamError::Unsupported(format!("gating_mode {other:?}"))),
                })
                .collect::<Result<_, _>>()?,
            None => vec![if cfg.get("gated").and_then(Value::as_bool) == Some(true) { Gating::Gated } else { Gating::None }; n],
        };
        let secondaries: Vec<Activation> = match per_layer("secondary_activation") {
            Some(v) => v
                .iter()
                .map(|a| if a.is_null() { Ok(Activation::Sigmoid) } else { Activation::from_json(a) })
                .collect::<Result<_, _>>()?,
            None => vec![Activation::Sigmoid; n],
        };
        if [kernels.len(), activations.len(), gatings.len(), secondaries.len()].iter().any(|&l| l != n) {
            return Err(NamError::Format("layer array: per-layer lists differ in length".into()));
        }

        let layers = (0..n)
            .map(|i| {
                let zc = if gatings[i] == Gating::None { bottleneck } else { 2 * bottleneck };
                Layer {
                    conv: Conv1d::new(channels, zc, kernels[i], dilations[i], true),
                    mixin: Conv1d::new(condition_size, zc, 1, 1, false),
                    activation: activations[i].clone(),
                    gating: gatings[i],
                    secondary: secondaries[i].clone(),
                    bottleneck,
                    layer1x1: layer1x1.then(|| Conv1d::new(bottleneck, channels, 1, 1, true)),
                    head1x1: head1x1.then(|| Conv1d::new(bottleneck, head1x1_out, 1, 1, true)),
                    z: vec![],
                    mix: vec![],
                    tmp: vec![],
                }
            })
            .collect();
        let head_in = if head1x1 { head1x1_out } else { bottleneck };
        Ok(LayerArray {
            rechannel: Conv1d::new(input_size, channels, 1, 1, false),
            layers,
            head_rechannel: Conv1d::new(head_in, head_size, head_kernel, head_dilation, head_bias),
            head_in,
            a: vec![],
            b: vec![],
            head_sum: vec![],
            head_part: vec![],
        })
    }

    fn load(&mut self, w: &mut Weights) -> Result<(), NamError> {
        self.rechannel.load(w)?;
        for l in &mut self.layers {
            l.conv.load(w)?;
            l.mixin.load(w)?;
            if let Some(c) = &mut l.layer1x1 {
                c.load(w)?;
            }
            if let Some(c) = &mut l.head1x1 {
                c.load(w)?;
            }
        }
        self.head_rechannel.load(w)
    }

    fn receptive_field(&self) -> usize {
        self.layers.iter().map(|l| l.conv.receptive_field()).sum::<usize>() + self.head_rechannel.receptive_field()
    }

    /// Returns (layer outputs, head outputs), both n frames.
    fn process(&mut self, input: &[f32], cond: &[f32], head_in: Option<&[f32]>, n: usize) -> (&[f32], &[f32]) {
        self.rechannel.process(input, n, &mut self.a);
        self.head_sum.clear();
        match head_in {
            Some(h) => self.head_sum.extend_from_slice(&h[..n * self.head_in]),
            None => self.head_sum.resize(n * self.head_in, 0.0),
        }
        for l in &mut self.layers {
            l.process(&self.a, cond, n, &mut self.head_part, &mut self.b);
            for (s, p) in self.head_sum.iter_mut().zip(&self.head_part) {
                *s += p;
            }
            std::mem::swap(&mut self.a, &mut self.b);
        }
        self.head_rechannel.process(&self.head_sum, n, &mut self.b);
        (&self.a, &self.b)
    }

    fn reset(&mut self) {
        for l in &mut self.layers {
            l.reset();
        }
        self.head_rechannel.reset();
    }
}

/// A NAM WaveNet (mono in, mono out).
#[derive(Clone, Debug)]
pub struct WaveNet {
    arrays: Vec<LayerArray>,
    head_scale: f32,
    sample_rate: Option<f64>,
    // scratch: previous array's layer outputs and head outputs
    layer_out: Vec<f32>,
    head_out: Vec<f32>,
}

impl WaveNet {
    pub fn from_json(config: &Value, weights: &[f32], sample_rate: Option<f64>) -> Result<WaveNet, NamError> {
        if config.get("condition_dsp").is_some_and(|c| !c.is_null()) {
            return Err(NamError::Unsupported("condition_dsp".into()));
        }
        if config.get("head").is_some_and(|h| !h.is_null()) {
            return Err(NamError::Unsupported("post-stack head".into()));
        }
        let mut arrays: Vec<LayerArray> = config
            .get("layers")
            .and_then(Value::as_array)
            .ok_or_else(|| NamError::Format("WaveNet: missing layers".into()))?
            .iter()
            .map(LayerArray::from_json)
            .collect::<Result<_, _>>()?;
        if arrays.is_empty() {
            return Err(NamError::Format("WaveNet: no layer arrays".into()));
        }
        if arrays[0].rechannel.cin != 1 || arrays[0].layers.first().is_some_and(|l| l.mixin.cin != 1) {
            return Err(NamError::Unsupported("multi-channel input".into()));
        }
        if arrays.last().is_some_and(|a| a.head_rechannel.cout != 1) {
            return Err(NamError::Unsupported("multi-channel output".into()));
        }
        let mut w = Weights { w: weights, pos: 0 };
        for a in &mut arrays {
            a.load(&mut w)?;
        }
        let head_scale = *w.take(1)?.first().expect("1");
        if w.pos != weights.len() {
            return Err(NamError::Weights { expected: w.pos, got: weights.len() });
        }
        let mut net = WaveNet { arrays, head_scale, sample_rate, layer_out: vec![], head_out: vec![] };
        net.reset();
        Ok(net)
    }

    /// Samples of silence needed to settle every buffer (NeuralAmpModelerCore's prewarm length).
    pub fn prewarm_samples(&self) -> usize {
        1 + self.arrays.iter().map(LayerArray::receptive_field).sum::<usize>()
    }

    pub fn sample_rate(&self) -> Option<f64> {
        self.sample_rate
    }

    /// Clear the history, then run silence through it, like NeuralAmpModelerCore's `Reset` + `prewarm`.
    pub fn reset(&mut self) {
        for a in &mut self.arrays {
            a.reset();
        }
        let zeros = vec![0.0; 4096];
        let mut out = vec![0.0; 4096];
        let mut left = self.prewarm_samples();
        while left > 0 {
            let n = left.min(4096);
            self.process(&zeros[..n], &mut out[..n]);
            left -= n;
        }
    }

    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let n = input.len().min(output.len());
        for i in 0..self.arrays.len() {
            let (prev, rest) = self.arrays.split_at_mut(i);
            let arr = &mut rest[0];
            let (lo, ho) = match prev.last() {
                None => arr.process(input, input, None, n),
                Some(_) => arr.process(&self.layer_out, input, Some(&self.head_out), n),
            };
            let (lo, ho) = (lo.to_vec(), ho.to_vec());
            self.layer_out = lo;
            self.head_out = ho;
        }
        for (o, h) in output[..n].iter_mut().zip(&self.head_out) {
            *o = self.head_scale * h;
        }
    }
}

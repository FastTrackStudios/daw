//! Pure-Rust WaveNet inference, structurally mirroring
//! `NAM/wavenet/{model.cpp,detail.h,params.h}` from NeuralAmpModelerCore.
//!
//! Supports the standard trainer output (legacy 0.5.x `kernel_size`/`gated`
//! configs and modern 0.7.x per-layer `kernel_sizes` / activation-object /
//! layer-head configs). Unsupported exotica (active FiLM, blended gating,
//! condition DSP, slimmable slicing) returns a load error rather than wrong
//! audio.

use serde_json::Value;

use super::activations::Activation;
use super::mat::Mat;
use super::nn::{Conv1D, Conv1x1, Weights};

#[derive(Clone, Copy, PartialEq)]
enum GatingMode {
    None,
    Gated,
}

// Layer ======================================================================

struct Layer {
    conv: Conv1D,
    input_mixin: Conv1x1,
    layer1x1: Option<Conv1x1>,
    head1x1: Option<Conv1x1>,
    activation: Activation,
    secondary_activation: Option<Activation>,
    gating: GatingMode,
    bottleneck: usize,
    channels: usize,
    /// z = conv(input) + input_mixin(condition); rows = bottleneck (or
    /// 2*bottleneck when gated). Post-activation, the top `bottleneck` rows
    /// hold the activated output.
    z: Mat,
    /// Residual output to the next layer (channels rows).
    out_next: Mat,
}

impl Layer {
    fn set_max_buffer_size(&mut self, max_frames: usize) {
        self.conv.set_max_buffer_size(max_frames);
        self.input_mixin.set_max_buffer_size(max_frames);
        let z_channels = match self.gating {
            GatingMode::None => self.bottleneck,
            GatingMode::Gated => 2 * self.bottleneck,
        };
        self.z.reset(z_channels, max_frames);
        if let Some(l) = &mut self.layer1x1 {
            l.set_max_buffer_size(max_frames);
        }
        if let Some(h) = &mut self.head1x1 {
            h.set_max_buffer_size(max_frames);
        }
        self.out_next.reset(self.channels, max_frames);
    }

    fn set_weights(&mut self, w: &mut Weights) -> Result<(), String> {
        self.conv.set_weights(w)?;
        self.input_mixin.set_weights(w)?;
        if let Some(l) = &mut self.layer1x1 {
            l.set_weights(w)?;
        }
        if let Some(h) = &mut self.head1x1 {
            h.set_weights(w)?;
        }
        Ok(())
    }

    fn process(&mut self, input: &Mat, condition: &Mat, num_frames: usize) {
        self.conv.process(input, num_frames);
        self.input_mixin.process(condition, num_frames);

        // z = conv out + input mixin out
        let z_rows = self.z.rows();
        for f in 0..num_frames {
            let a = self.conv.out.col(f);
            let b = self.input_mixin.out.col(f);
            let z = self.z.col_mut(f);
            for r in 0..z_rows {
                z[r] = a[r] + b[r];
            }
        }

        match self.gating {
            GatingMode::None => {
                // Contiguous leftCols apply, matching the C++ flat apply.
                self.activation.apply(self.z.left_cols_mut(num_frames));
            }
            GatingMode::Gated => {
                // Per-column: top = act(top) * secondary(bottom); matches
                // GatingActivation (per-column buffers, so PReLU-style pos
                // indexing restarts each column).
                let bn = self.bottleneck;
                let secondary = self
                    .secondary_activation
                    .as_ref()
                    .unwrap_or(&Activation::Sigmoid);
                let mut top = vec![0.0f32; bn];
                let mut bottom = vec![0.0f32; bn];
                for f in 0..num_frames {
                    {
                        let z = self.z.col(f);
                        top.copy_from_slice(&z[..bn]);
                        bottom.copy_from_slice(&z[bn..2 * bn]);
                    }
                    self.activation.apply(&mut top);
                    secondary.apply(&mut bottom);
                    let z = self.z.col_mut(f);
                    for c in 0..bn {
                        z[c] = top[c] * bottom[c];
                    }
                }
            }
        }

        // layer1x1 reads the activated top `bottleneck` rows of z.
        if let Some(l) = &mut self.layer1x1 {
            l.process(&self.z, num_frames);
        }

        // head1x1 also reads the activated top rows of z.
        if let Some(h) = &mut self.head1x1 {
            h.process(&self.z, num_frames);
        }

        // Residual: out_next = input + layer1x1(z), or input if inactive.
        for f in 0..num_frames {
            let in_col = &input.col(f)[..self.channels];
            let out_col = self.out_next.col_mut(f);
            match &self.layer1x1 {
                Some(l) => {
                    let lc = l.out.col(f);
                    for c in 0..self.channels {
                        out_col[c] = in_col[c] + lc[c];
                    }
                }
                None => out_col.copy_from_slice(in_col),
            }
        }
    }
}

// LayerArray =================================================================

struct LayerArray {
    rechannel: Conv1x1,
    layers: Vec<Layer>,
    /// Accumulated skip connections (head_output_size rows).
    head_inputs: Mat,
    /// Projects accumulated head inputs to head_size (causal Conv1D).
    head_rechannel: Conv1D,
    head_output_size: usize,
    head_size: usize,
}

impl LayerArray {
    fn receptive_field(&self) -> usize {
        let mut rf = 0;
        for layer in &self.layers {
            rf += layer.conv.dilation() * (layer.conv.kernel_size() - 1);
        }
        rf += self.head_rechannel.dilation() * (self.head_rechannel.kernel_size() - 1);
        rf
    }

    fn set_max_buffer_size(&mut self, max_frames: usize) {
        self.rechannel.set_max_buffer_size(max_frames);
        self.head_rechannel.set_max_buffer_size(max_frames);
        for layer in &mut self.layers {
            layer.set_max_buffer_size(max_frames);
        }
        self.head_inputs.reset(self.head_output_size, max_frames);
    }

    fn set_weights(&mut self, w: &mut Weights) -> Result<(), String> {
        self.rechannel.set_weights(w)?;
        for layer in &mut self.layers {
            layer.set_weights(w)?;
        }
        self.head_rechannel.set_weights(w)
    }

    /// `head_in`: None for the first layer array (accumulator zeroed),
    /// Some(prev head outputs) for subsequent ones.
    fn process(&mut self, input: &Mat, condition: &Mat, head_in: Option<&Mat>, num_frames: usize) {
        match head_in {
            None => self.head_inputs.zero(),
            Some(h) => {
                for f in 0..num_frames {
                    let src = &h.col(f)[..self.head_output_size];
                    self.head_inputs.col_mut(f).copy_from_slice(src);
                }
            }
        }

        self.rechannel.process(input, num_frames);

        for i in 0..self.layers.len() {
            let (before, rest) = self.layers.split_at_mut(i);
            let layer = &mut rest[0];
            if i == 0 {
                layer.process(&self.rechannel.out, condition, num_frames);
            } else {
                let prev_out = &before[i - 1].out_next;
                layer.process(prev_out, condition, num_frames);
            }
            // Accumulate skip connection: head1x1 output if active, else the
            // activated top rows of z.
            let src: &Mat = match &layer.head1x1 {
                Some(h) => &h.out,
                None => &layer.z,
            };
            for f in 0..num_frames {
                let s = &src.col(f)[..self.head_output_size];
                let d = self.head_inputs.col_mut(f);
                for r in 0..s.len() {
                    d[r] += s[r];
                }
            }
        }

        self.head_rechannel.process(&self.head_inputs, num_frames);
    }

    fn layer_outputs(&self) -> &Mat {
        &self.layers.last().expect("layer array is non-empty").out_next
    }

    fn head_outputs(&self) -> &Mat {
        &self.head_rechannel.out
    }
}

// Post-stack head ============================================================

struct PostHead {
    convs: Vec<Conv1D>,
    activations: Vec<Activation>,
    in_channels: usize,
    scratch: Mat,
}

impl PostHead {
    fn receptive_field(&self) -> usize {
        let mut rf = 1;
        for c in &self.convs {
            rf += c.kernel_size() - 1;
        }
        rf
    }

    fn set_max_buffer_size(&mut self, max_frames: usize) {
        for c in &mut self.convs {
            c.set_max_buffer_size(max_frames);
        }
        self.scratch.reset(self.in_channels, max_frames);
    }

    fn set_weights(&mut self, w: &mut Weights) -> Result<(), String> {
        for c in &mut self.convs {
            c.set_weights(w)?;
        }
        Ok(())
    }

    /// Input is `scratch` (already scaled by head_scale). Applies
    /// activation → conv per stage; output is the last conv's out.
    fn process(&mut self, num_frames: usize) {
        for i in 0..self.convs.len() {
            if i == 0 {
                self.activations[i].apply(self.scratch.left_cols_mut(num_frames));
                self.convs[i].process(&self.scratch, num_frames);
            } else {
                let (before, rest) = self.convs.split_at_mut(i);
                let prev_out = &mut before[i - 1].out;
                self.activations[i].apply(prev_out.left_cols_mut(num_frames));
                rest[0].process(prev_out, num_frames);
            }
        }
    }
}

// WaveNet ====================================================================

pub(crate) struct WaveNet {
    layer_arrays: Vec<LayerArray>,
    head_scale: f32,
    post_head: Option<PostHead>,
    in_channels: usize,
    out_channels: usize,
    condition: Mat,
    prewarm_samples: usize,
    max_frames: usize,
}

impl WaveNet {
    pub fn prewarm_samples(&self) -> usize {
        self.prewarm_samples
    }

    pub fn in_channels(&self) -> usize {
        self.in_channels
    }

    pub fn out_channels(&self) -> usize {
        self.out_channels
    }

    pub fn set_max_buffer_size(&mut self, max_frames: usize) {
        self.max_frames = max_frames;
        self.condition.reset(self.in_channels, max_frames);
        for la in &mut self.layer_arrays {
            la.set_max_buffer_size(max_frames);
        }
        if let Some(h) = &mut self.post_head {
            h.set_max_buffer_size(max_frames);
        }
    }

    /// Process one block (mono). `num_frames` must be <= max buffer size.
    pub fn process_block(&mut self, input: &[f64], output: &mut [f64]) {
        let num_frames = input.len();
        debug_assert!(num_frames <= self.max_frames);

        // condition = raw input (no condition DSP support).
        for (f, &x) in input.iter().enumerate() {
            self.condition.col_mut(f)[0] = x as f32;
        }

        for i in 0..self.layer_arrays.len() {
            let (before, rest) = self.layer_arrays.split_at_mut(i);
            let la = &mut rest[0];
            if i == 0 {
                let cond = &self.condition;
                la.process(cond, cond, None, num_frames);
            } else {
                // `before` and `la` are disjoint borrows from split_at_mut.
                let prev = &before[i - 1];
                la.process(
                    prev.layer_outputs(),
                    &self.condition,
                    Some(prev.head_outputs()),
                    num_frames,
                );
            }
        }

        let final_head = self.layer_arrays.last().expect("non-empty").head_outputs();

        match &mut self.post_head {
            Some(head) => {
                // scratch = head_scale * final head outputs
                let scale = self.head_scale;
                for f in 0..num_frames {
                    let src = final_head.col(f);
                    let dst = head.scratch.col_mut(f);
                    for (d, &s) in dst.iter_mut().zip(src.iter()) {
                        *d = scale * s;
                    }
                }
                head.process(num_frames);
                let out_mat = &head.convs.last().expect("non-empty").out;
                for (f, o) in output.iter_mut().enumerate().take(num_frames) {
                    *o = out_mat.col(f)[0] as f64;
                }
            }
            None => {
                for (f, o) in output.iter_mut().enumerate().take(num_frames) {
                    *o = (self.head_scale * final_head.col(f)[0]) as f64;
                }
            }
        }
    }
}

// Parsing ====================================================================

fn as_usize(v: &Value, what: &str) -> Result<usize, String> {
    v.as_u64()
        .map(|v| v as usize)
        .ok_or_else(|| format!("expected non-negative integer for {what}, got {v}"))
}

fn get<'a>(obj: &'a Value, key: &str) -> Option<&'a Value> {
    obj.as_object().and_then(|o| o.get(key)).filter(|v| !v.is_null())
}

/// Parse a WaveNet `config` object (the value of the top-level "config" key)
/// and build the model with `weights`.
pub(crate) fn parse(config: &Value, weights: &[f32]) -> Result<WaveNet, String> {
    if get(config, "condition_dsp").is_some() {
        return Err("WaveNet condition_dsp is not supported by the pure-Rust engine".into());
    }

    let layers_json = get(config, "layers")
        .and_then(|v| v.as_array())
        .ok_or("WaveNet config missing 'layers' array")?;
    if layers_json.is_empty() {
        return Err("WaveNet requires at least one layer array".into());
    }

    let mut layer_arrays = Vec::new();
    let mut prev_head_size: Option<usize> = None;

    for (idx, lc) in layers_json.iter().enumerate() {
        let err_ctx = |m: &str| format!("layer array {idx}: {m}");

        // Reject unsupported features up front.
        for film_key in [
            "conv_pre_film",
            "conv_post_film",
            "input_mixin_pre_film",
            "input_mixin_post_film",
            "activation_pre_film",
            "activation_post_film",
            "layer1x1_post_film",
            "head1x1_post_film",
        ] {
            if let Some(fc) = get(lc, film_key) {
                let active = match fc {
                    Value::Bool(b) => *b,
                    Value::Object(o) => o
                        .get("active")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                    _ => false,
                };
                if active {
                    return Err(err_ctx(&format!(
                        "active FiLM ({film_key}) is not supported by the pure-Rust engine"
                    )));
                }
            }
        }
        if let Some(s) = get(lc, "slimmable") {
            if s.is_object() {
                let method = s.get("method").and_then(|v| v.as_str()).unwrap_or("");
                if method == "slice_channels_uniform" {
                    return Err(err_ctx(
                        "slimmable WaveNet is not supported by the pure-Rust engine",
                    ));
                }
            }
        }

        let channels = as_usize(
            get(lc, "channels").ok_or_else(|| err_ctx("missing channels"))?,
            "channels",
        )?;
        let bottleneck = match get(lc, "bottleneck") {
            Some(v) => as_usize(v, "bottleneck")?,
            None => channels,
        };
        let input_size = as_usize(
            get(lc, "input_size").ok_or_else(|| err_ctx("missing input_size"))?,
            "input_size",
        )?;
        let condition_size = as_usize(
            get(lc, "condition_size").ok_or_else(|| err_ctx("missing condition_size"))?,
            "condition_size",
        )?;
        let groups_input = match get(lc, "groups_input") {
            Some(v) => as_usize(v, "groups_input")?,
            None => 1,
        };
        let groups_input_mixin = match get(lc, "groups_input_mixin") {
            Some(v) => as_usize(v, "groups_input_mixin")?,
            None => 1,
        };

        // layer1x1: defaults to active with groups 1.
        let (layer1x1_active, layer1x1_groups) = match get(lc, "layer1x1") {
            Some(v) => (
                v.get("active").and_then(|b| b.as_bool()).unwrap_or(true),
                v.get("groups")
                    .and_then(|g| g.as_u64())
                    .map(|g| g as usize)
                    .unwrap_or(1),
            ),
            None => (true, 1),
        };
        // head1x1: defaults to inactive.
        let (head1x1_active, head1x1_out, head1x1_groups) = match get(lc, "head1x1") {
            Some(v) => (
                v.get("active").and_then(|b| b.as_bool()).unwrap_or(false),
                v.get("out_channels")
                    .and_then(|g| g.as_u64())
                    .map(|g| g as usize)
                    .unwrap_or(channels),
                v.get("groups")
                    .and_then(|g| g.as_u64())
                    .map(|g| g as usize)
                    .unwrap_or(1),
            ),
            None => (false, channels, 1),
        };

        // Layer-array head (rechannel to head_size).
        let (head_size, head_kernel_size, head_bias, head_dilation) =
            if let Some(hj) = get(lc, "head") {
                if !hj.is_object() {
                    return Err(err_ctx("'head' must be a JSON object"));
                }
                (
                    as_usize(
                        hj.get("out_channels").ok_or_else(|| err_ctx("head missing out_channels"))?,
                        "head.out_channels",
                    )?,
                    as_usize(
                        hj.get("kernel_size").ok_or_else(|| err_ctx("head missing kernel_size"))?,
                        "head.kernel_size",
                    )?,
                    hj.get("bias")
                        .and_then(|v| v.as_bool())
                        .ok_or_else(|| err_ctx("head missing bias"))?,
                    match hj.get("head_dilation").filter(|v| !v.is_null()) {
                        Some(v) => as_usize(v, "head.head_dilation")?,
                        None => 1,
                    },
                )
            } else if let Some(hs) = get(lc, "head_size") {
                (
                    as_usize(hs, "head_size")?,
                    1,
                    get(lc, "head_bias")
                        .and_then(|v| v.as_bool())
                        .ok_or_else(|| err_ctx("missing head_bias"))?,
                    1,
                )
            } else {
                return Err(err_ctx("expected 'head' object or legacy 'head_size'/'head_bias'"));
            };

        let dilations: Vec<usize> = get(lc, "dilations")
            .and_then(|v| v.as_array())
            .ok_or_else(|| err_ctx("missing dilations"))?
            .iter()
            .map(|v| as_usize(v, "dilation"))
            .collect::<Result<_, _>>()?;
        let num_layers = dilations.len();

        // Kernel sizes: per-layer array or single legacy value.
        let kernel_sizes: Vec<usize> = if let Some(ks) = get(lc, "kernel_sizes") {
            let arr = ks
                .as_array()
                .ok_or_else(|| err_ctx("kernel_sizes must be an array"))?;
            if arr.len() != num_layers {
                return Err(err_ctx("kernel_sizes size must match dilations size"));
            }
            arr.iter()
                .map(|v| as_usize(v, "kernel_size"))
                .collect::<Result<_, _>>()?
        } else if let Some(k) = get(lc, "kernel_size") {
            vec![as_usize(k, "kernel_size")?; num_layers]
        } else {
            return Err(err_ctx("either kernel_size or kernel_sizes must be provided"));
        };

        // Activations: single config or per-layer array.
        let activation_json = get(lc, "activation").ok_or_else(|| err_ctx("missing activation"))?;
        let activations: Vec<Activation> = if let Some(arr) = activation_json.as_array() {
            if arr.len() != num_layers {
                return Err(err_ctx("activation array size must match dilations size"));
            }
            arr.iter().map(Activation::from_json).collect::<Result<_, _>>()?
        } else {
            vec![Activation::from_json(activation_json)?; num_layers]
        };

        // Gating: gating_mode (string or array) or legacy "gated" bool.
        let parse_mode = |s: &str| -> Result<GatingMode, String> {
            match s {
                "none" => Ok(GatingMode::None),
                "gated" => Ok(GatingMode::Gated),
                "blended" => Err("blended gating is not supported by the pure-Rust engine".into()),
                other => Err(format!("invalid gating_mode: {other}")),
            }
        };
        let secondary_json = get(lc, "secondary_activation");
        let (gating_modes, secondary_activations): (Vec<GatingMode>, Vec<Option<Activation>>) =
            if let Some(gm) = get(lc, "gating_mode") {
                if let Some(arr) = gm.as_array() {
                    if arr.len() != num_layers {
                        return Err(err_ctx("gating_mode array size must match dilations size"));
                    }
                    let mut modes = Vec::new();
                    let mut secs = Vec::new();
                    for (li, m) in arr.iter().enumerate() {
                        let mode = parse_mode(m.as_str().unwrap_or("")).map_err(|e| err_ctx(&e))?;
                        modes.push(mode);
                        let sec = if mode == GatingMode::None {
                            None
                        } else if let Some(sa) = secondary_json {
                            if let Some(sarr) = sa.as_array() {
                                match sarr.get(li).filter(|v| !v.is_null()) {
                                    Some(v) => Some(Activation::from_json(v)?),
                                    None => Some(Activation::Sigmoid),
                                }
                            } else {
                                Some(Activation::from_json(sa)?)
                            }
                        } else {
                            Some(Activation::Sigmoid)
                        };
                        secs.push(sec);
                    }
                    (modes, secs)
                } else {
                    let mode = parse_mode(gm.as_str().unwrap_or("")).map_err(|e| err_ctx(&e))?;
                    let sec = if mode == GatingMode::None {
                        None
                    } else if let Some(sa) = secondary_json {
                        Some(Activation::from_json(sa)?)
                    } else {
                        Some(Activation::Sigmoid)
                    };
                    (vec![mode; num_layers], vec![sec; num_layers])
                }
            } else if let Some(g) = get(lc, "gated") {
                let gated = g.as_bool().unwrap_or(false);
                if gated {
                    (
                        vec![GatingMode::Gated; num_layers],
                        vec![Some(Activation::Sigmoid); num_layers],
                    )
                } else {
                    (vec![GatingMode::None; num_layers], vec![None; num_layers])
                }
            } else {
                (vec![GatingMode::None; num_layers], vec![None; num_layers])
            };

        if !layer1x1_active && bottleneck != channels {
            return Err(err_ctx("when layer1x1 is inactive, bottleneck must equal channels"));
        }

        // Build layers.
        let mut layers = Vec::with_capacity(num_layers);
        for li in 0..num_layers {
            let gated = gating_modes[li] != GatingMode::None;
            let z_channels = if gated { 2 * bottleneck } else { bottleneck };
            let layer = Layer {
                conv: Conv1D::new(channels, z_channels, kernel_sizes[li], true, dilations[li], groups_input)?,
                input_mixin: Conv1x1::new(condition_size, z_channels, false, groups_input_mixin)?,
                layer1x1: if layer1x1_active {
                    Some(Conv1x1::new(bottleneck, channels, true, layer1x1_groups)?)
                } else {
                    None
                },
                head1x1: if head1x1_active {
                    Some(Conv1x1::new(bottleneck, head1x1_out, true, head1x1_groups)?)
                } else {
                    None
                },
                activation: activations[li].clone(),
                secondary_activation: secondary_activations[li].clone(),
                gating: gating_modes[li],
                bottleneck,
                channels,
                z: Mat::new(z_channels, 0),
                out_next: Mat::new(channels, 0),
            };
            layers.push(layer);
        }

        let head_output_size = if head1x1_active { head1x1_out } else { bottleneck };

        // Head-input chaining requires size match with the previous array.
        if let Some(prev) = prev_head_size {
            if prev != head_output_size {
                return Err(err_ctx(&format!(
                    "head chaining size mismatch: previous head_size {prev} vs head accumulator {head_output_size}"
                )));
            }
        }
        prev_head_size = Some(head_size);

        layer_arrays.push(LayerArray {
            rechannel: Conv1x1::new(input_size, channels, false, 1)?,
            layers,
            head_inputs: Mat::new(head_output_size, 0),
            head_rechannel: Conv1D::new(
                head_output_size,
                head_size,
                head_kernel_size,
                head_bias,
                head_dilation,
                1,
            )?,
            head_output_size,
            head_size,
        });
    }

    let head_scale = get(config, "head_scale")
        .and_then(|v| v.as_f64())
        .ok_or("WaveNet config missing head_scale")? as f32;
    let in_channels = match get(config, "in_channels") {
        Some(v) => as_usize(v, "in_channels")?,
        None => 1,
    };

    // Post-stack head (top-level "head", non-null).
    let post_head = match get(config, "head") {
        Some(hj) => {
            let hp_in = layer_arrays.last().expect("non-empty").head_size;
            if let Some(legacy_in) = hj.get("in_channels").filter(|v| !v.is_null()) {
                if as_usize(legacy_in, "head.in_channels")? != hp_in {
                    return Err("WaveNet head.in_channels must equal last layer's head_size".into());
                }
            }
            let hp_channels = as_usize(
                hj.get("channels").ok_or("head missing channels")?,
                "head.channels",
            )?;
            let hp_out = as_usize(
                hj.get("out_channels").ok_or("head missing out_channels")?,
                "head.out_channels",
            )?;
            let ks: Vec<usize> = hj
                .get("kernel_sizes")
                .and_then(|v| v.as_array())
                .ok_or("head missing kernel_sizes")?
                .iter()
                .map(|v| as_usize(v, "head kernel_size"))
                .collect::<Result<_, _>>()?;
            if ks.is_empty() {
                return Err("head.kernel_sizes must be non-empty".into());
            }
            let act_json = hj.get("activation").ok_or("head missing activation")?;
            let n = ks.len();
            let mut convs = Vec::new();
            let mut acts = Vec::new();
            let mut cin = hp_in;
            for (i, &k) in ks.iter().enumerate() {
                let cout = if i + 1 == n { hp_out } else { hp_channels };
                acts.push(Activation::from_json(act_json)?);
                convs.push(Conv1D::new(cin, cout, k, true, 1, 1)?);
                cin = cout;
            }
            Some(PostHead {
                convs,
                activations: acts,
                in_channels: hp_in,
                scratch: Mat::new(hp_in, 0),
            })
        }
        None => None,
    };

    let out_channels = match &post_head {
        Some(_) => get(config, "head")
            .and_then(|h| h.get("out_channels"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as usize,
        None => layer_arrays.last().expect("non-empty").head_size,
    };

    let mut net = WaveNet {
        layer_arrays,
        head_scale,
        post_head,
        in_channels,
        out_channels,
        condition: Mat::new(in_channels, 0),
        prewarm_samples: 0,
        max_frames: 0,
    };

    // Set weights: layer arrays, post head, then the trailing head_scale.
    let mut w = Weights::new(weights);
    for la in &mut net.layer_arrays {
        la.set_weights(&mut w)?;
    }
    if let Some(h) = &mut net.post_head {
        h.set_weights(&mut w)?;
    }
    net.head_scale = w.next()?;
    w.finish()?;

    // Prewarm samples: 1 + sum of receptive fields (+ post head rf - 1).
    let mut prewarm = 1usize;
    for la in &net.layer_arrays {
        prewarm += la.receptive_field();
    }
    if let Some(h) = &net.post_head {
        prewarm += h.receptive_field() - 1;
    }
    net.prewarm_samples = prewarm;

    Ok(net)
}

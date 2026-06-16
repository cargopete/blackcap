//! Host state and the host-side DSP that cartridges import.
//!
//! Effect state (filter/reverb/delay/shaper) lives here, in a `ResourceTable`;
//! the cartridge holds opaque `Resource<…>` handles. Oscillators are stateless
//! free functions (phase supplied by the caller).

use wasmtime::component::{Resource, ResourceTable};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::wit::jukebox::cartridge::dsp::ShapeKind;
use crate::wit::jukebox::cartridge::{dsp, log, types};

/// Store data. Carries the WASI context (std cartridges need it), the resource
/// table shared by WASI and our DSP/sampler resources, and the device sample
/// rate (for resampling library samples).
pub struct HostState {
    ctx: WasiCtx,
    pub(crate) table: ResourceTable,
    pub(crate) sample_rate: u32,
}

impl HostState {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            ctx: WasiCtxBuilder::new().inherit_stdio().build(),
            table: ResourceTable::new(),
            sample_rate,
        }
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

// ---------------------------------------------------------------------------
// DSP building blocks
// ---------------------------------------------------------------------------

#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return 0.0;
    }
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[inline]
fn xorshift(state: &mut u64) -> f32 {
    let mut x = *state;
    if x == 0 {
        x = 0x9E37_79B9_7F4A_7C15;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    ((x >> 40) as f32 / (1u64 << 23) as f32) - 1.0
}

/// One-pole lowpass with a bypass when cutoff <= 0.
#[derive(Clone)]
struct OnePoleLp {
    a: f32,
    y: f32,
    bypass: bool,
}

impl OnePoleLp {
    fn new(sample_rate: u32, cutoff_hz: f32) -> Self {
        let mut f = Self {
            a: 0.0,
            y: 0.0,
            bypass: true,
        };
        f.set_cutoff(sample_rate, cutoff_hz);
        f
    }

    fn set_cutoff(&mut self, sample_rate: u32, cutoff_hz: f32) {
        if cutoff_hz <= 0.0 {
            self.bypass = true;
            return;
        }
        self.bypass = false;
        let dt = 1.0 / sample_rate as f32;
        let rc = 1.0 / (core::f32::consts::TAU * cutoff_hz);
        self.a = dt / (rc + dt);
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        if self.bypass {
            return x;
        }
        self.y += self.a * (x - self.y);
        self.y
    }

    fn reset(&mut self) {
        self.y = 0.0;
    }
}

/// One-pole highpass (complement of the lowpass), with bypass when cutoff <= 0.
#[derive(Clone)]
struct OnePoleHp {
    lp: OnePoleLp,
    bypass: bool,
}

impl OnePoleHp {
    fn new(sample_rate: u32, cutoff_hz: f32) -> Self {
        Self {
            lp: OnePoleLp::new(sample_rate, cutoff_hz.max(0.0)),
            bypass: cutoff_hz <= 0.0,
        }
    }

    fn set_cutoff(&mut self, sample_rate: u32, cutoff_hz: f32) {
        self.bypass = cutoff_hz <= 0.0;
        self.lp.set_cutoff(sample_rate, cutoff_hz);
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        if self.bypass {
            return x;
        }
        x - self.lp.process(x)
    }

    fn reset(&mut self) {
        self.lp.reset();
    }
}

/// A DC blocker (one-pole highpass at ~DC) for asymmetric waveshaping.
#[derive(Clone)]
struct DcBlocker {
    x1: f32,
    y1: f32,
}

impl DcBlocker {
    fn new() -> Self {
        Self { x1: 0.0, y1: 0.0 }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + 0.999 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

/// Windowed-sinc (Hamming) lowpass. Generated, never transcribed by hand.
fn design_lowpass(num_taps: usize, fc: f32) -> Vec<f32> {
    let m = (num_taps - 1) as f32;
    let mut h = Vec::with_capacity(num_taps);
    let mut sum = 0.0;
    for n in 0..num_taps {
        let k = n as f32 - m / 2.0;
        let sinc = if k.abs() < 1e-6 {
            2.0 * fc
        } else {
            (2.0 * core::f32::consts::PI * fc * k).sin() / (core::f32::consts::PI * k)
        };
        let w = 0.54 - 0.46 * (2.0 * core::f32::consts::PI * n as f32 / m).cos();
        let v = sinc * w;
        h.push(v);
        sum += v;
    }
    for v in &mut h {
        *v /= sum; // unity DC gain
    }
    h
}

/// FIR filter with a circular buffer.
#[derive(Clone)]
struct Fir {
    coeffs: Vec<f32>,
    buf: Vec<f32>,
    pos: usize,
}

impl Fir {
    fn new(coeffs: Vec<f32>) -> Self {
        let n = coeffs.len();
        Self {
            coeffs,
            buf: vec![0.0; n],
            pos: 0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let n = self.coeffs.len();
        self.buf[self.pos] = x;
        let mut acc = 0.0;
        let mut idx = self.pos;
        for &c in &self.coeffs {
            acc += c * self.buf[idx];
            idx = if idx == 0 { n - 1 } else { idx - 1 };
        }
        self.pos = (self.pos + 1) % n;
        acc
    }

    fn reset(&mut self) {
        self.buf.iter_mut().for_each(|b| *b = 0.0);
        self.pos = 0;
    }
}

/// Integer-factor oversampler (single-stage windowed-sinc up/down).
struct Oversampler {
    factor: usize,
    up: Fir,
    down: Fir,
}

impl Oversampler {
    fn new(factor: usize) -> Self {
        let fc = 0.5 / factor as f32;
        let taps = design_lowpass(63, fc);
        let up_taps: Vec<f32> = taps.iter().map(|c| c * factor as f32).collect();
        Self {
            factor,
            up: Fir::new(up_taps),
            down: Fir::new(taps),
        }
    }

    fn upsample(&mut self, x: f32, out: &mut [f32]) {
        out[0] = self.up.process(x);
        for slot in out.iter_mut().take(self.factor).skip(1) {
            *slot = self.up.process(0.0);
        }
    }

    fn downsample(&mut self, samples: &[f32]) -> f32 {
        let mut y = 0.0;
        for &s in samples {
            y = self.down.process(s);
        }
        y
    }

    fn reset(&mut self) {
        self.up.reset();
        self.down.reset();
    }
}

// ---------------------------------------------------------------------------
// Resource node types (mapped from WIT resources via bindgen `with:`)
// ---------------------------------------------------------------------------

/// `biquad-svf`: Chamberlin state-variable filter.
pub struct SvfNode {
    sr: f32,
    f: f32,
    q: f32,
    low: f32,
    band: f32,
}

impl SvfNode {
    fn new(sample_rate: u32) -> Self {
        Self {
            sr: sample_rate as f32,
            f: 0.2,
            q: 0.7,
            low: 0.0,
            band: 0.0,
        }
    }

    fn set_params(&mut self, cutoff_hz: f32, q: f32) {
        let fc = cutoff_hz.clamp(20.0, self.sr * 0.45);
        self.f = 2.0 * (core::f32::consts::PI * fc / self.sr).sin();
        self.q = q.max(0.5);
    }

    #[inline]
    fn step(&mut self, x: f32) -> (f32, f32, f32) {
        let damp = 1.0 / self.q;
        let high = x - self.low - damp * self.band;
        self.band += self.f * high;
        self.low += self.f * self.band;
        (self.low, high, self.band)
    }
}

/// `reverb`: freeverb-style stereo reverb.
pub struct ReverbNode {
    fv: freeverb::Freeverb,
}

impl ReverbNode {
    fn new(sample_rate: u32) -> Self {
        let mut fv = freeverb::Freeverb::new(sample_rate as usize);
        fv.set_room_size(0.5);
        fv.set_dampening(0.5);
        fv.set_wet(0.3);
        fv.set_dry(0.0); // host treats reverb as a send; cartridge mixes dry
        Self { fv }
    }
}

/// `delay`: feedback delay line.
pub struct DelayNode {
    buf: Vec<f32>,
    write: usize,
    sr: f32,
    delay_samples: usize,
    feedback: f32,
    mix: f32,
}

impl DelayNode {
    fn new(sample_rate: u32, max_seconds: f32) -> Self {
        let cap = ((max_seconds.max(0.01) * sample_rate as f32) as usize).max(1);
        Self {
            buf: vec![0.0; cap],
            write: 0,
            sr: sample_rate as f32,
            delay_samples: cap / 2,
            feedback: 0.3,
            mix: 0.3,
        }
    }

    fn set_params(&mut self, time_seconds: f32, feedback: f32, mix: f32) {
        let d = (time_seconds.max(0.0) * self.sr) as usize;
        self.delay_samples = d.clamp(1, self.buf.len() - 1);
        self.feedback = feedback.clamp(0.0, 0.99);
        self.mix = mix.clamp(0.0, 1.0);
    }

    #[inline]
    fn step(&mut self, x: f32) -> f32 {
        let read = (self.write + self.buf.len() - self.delay_samples) % self.buf.len();
        let delayed = self.buf[read];
        self.buf[self.write] = x + delayed * self.feedback;
        self.write = (self.write + 1) % self.buf.len();
        x * (1.0 - self.mix) + delayed * self.mix
    }
}

/// `waveshaper`: oversampled, anti-aliased, with tightening EQ.
pub struct ShaperNode {
    sr: u32,
    os: Option<Oversampler>,
    pre_hp: OnePoleHp,
    post_lp: OnePoleLp,
    dc: DcBlocker,
    shape: ShapeKind,
    drive: f32,
    bias: f32,
}

impl ShaperNode {
    fn new(sample_rate: u32, oversample: u8) -> Self {
        let factor = match oversample {
            2 => 2,
            4 => 4,
            _ => 1,
        };
        Self {
            sr: sample_rate,
            os: if factor > 1 {
                Some(Oversampler::new(factor))
            } else {
                None
            },
            pre_hp: OnePoleHp::new(sample_rate, 0.0),
            post_lp: OnePoleLp::new(sample_rate, 0.0),
            dc: DcBlocker::new(),
            shape: ShapeKind::SoftTanh,
            drive: 1.0,
            bias: 0.0,
        }
    }

    #[inline]
    fn shape_one(&mut self, x0: f32) -> f32 {
        let x = self.pre_hp.process(x0);
        let (shape, drive, bias) = (self.shape, self.drive, self.bias);
        let y = match &mut self.os {
            None => apply_curve(shape, x * drive + bias),
            Some(os) => {
                let mut acc = [0.0f32; 4];
                let f = os.factor;
                os.upsample(x, &mut acc[..f]);
                for s in acc[..f].iter_mut() {
                    *s = apply_curve(shape, *s * drive + bias);
                }
                os.downsample(&acc[..f])
            }
        };
        self.post_lp.process(self.dc.process(y))
    }
}

#[inline]
fn apply_curve(shape: ShapeKind, x: f32) -> f32 {
    match shape {
        // bias (for asym) is applied by the caller before this point
        ShapeKind::SoftTanh | ShapeKind::AsymTanh => x.tanh(),
        ShapeKind::HardClip => x.clamp(-1.0, 1.0),
        ShapeKind::Cubic => {
            let u = x.clamp(-1.0, 1.0);
            1.5 * u - 0.5 * u * u * u
        }
        ShapeKind::Wavefold => x.sin(),
    }
}

// ---------------------------------------------------------------------------
// Generated-trait implementations
// ---------------------------------------------------------------------------

impl dsp::Host for HostState {
    fn osc_sine(&mut self, freq_hz: f32, phase: f32, num_frames: u32, sample_rate: u32) -> (Vec<f32>, f32) {
        let dt = freq_hz / sample_rate as f32;
        let mut p = phase;
        let block = (0..num_frames)
            .map(|_| {
                let s = (core::f32::consts::TAU * p).sin();
                p = (p + dt).fract();
                s
            })
            .collect();
        (block, p)
    }

    fn osc_saw(&mut self, freq_hz: f32, phase: f32, num_frames: u32, sample_rate: u32) -> (Vec<f32>, f32) {
        let dt = freq_hz / sample_rate as f32;
        let mut p = phase;
        let block = (0..num_frames)
            .map(|_| {
                let s = (2.0 * p - 1.0) - poly_blep(p, dt);
                p = (p + dt).fract();
                s
            })
            .collect();
        (block, p)
    }

    fn osc_square(&mut self, freq_hz: f32, phase: f32, pw: f32, num_frames: u32, sample_rate: u32) -> (Vec<f32>, f32) {
        let dt = freq_hz / sample_rate as f32;
        let mut p = phase;
        let block = (0..num_frames)
            .map(|_| {
                let mut s = if p < pw { 1.0 } else { -1.0 };
                s += poly_blep(p, dt);
                s -= poly_blep((p + 1.0 - pw).fract(), dt);
                p = (p + dt).fract();
                s
            })
            .collect();
        (block, p)
    }

    fn osc_triangle(&mut self, freq_hz: f32, phase: f32, num_frames: u32, sample_rate: u32) -> (Vec<f32>, f32) {
        let dt = freq_hz / sample_rate as f32;
        let mut p = phase;
        let block = (0..num_frames)
            .map(|_| {
                let s = if p < 0.5 { 4.0 * p - 1.0 } else { 3.0 - 4.0 * p };
                p = (p + dt).fract();
                s
            })
            .collect();
        (block, p)
    }

    fn osc_noise(&mut self, seed: u64, num_frames: u32) -> (Vec<f32>, u64) {
        let mut state = seed;
        let block = (0..num_frames).map(|_| xorshift(&mut state)).collect();
        (block, state)
    }
}

impl dsp::HostBiquadSvf for HostState {
    fn new(&mut self, sample_rate: u32) -> Resource<SvfNode> {
        self.table.push(SvfNode::new(sample_rate)).expect("resource table push")
    }

    fn set_params(&mut self, self_: Resource<SvfNode>, cutoff_hz: f32, q: f32) {
        if let Ok(node) = self.table.get_mut(&self_) {
            node.set_params(cutoff_hz, q);
        }
    }

    fn process(&mut self, self_: Resource<SvfNode>, input: Vec<f32>) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let node = match self.table.get_mut(&self_) {
            Ok(n) => n,
            Err(_) => return (vec![], vec![], vec![]),
        };
        let mut lp = Vec::with_capacity(input.len());
        let mut hp = Vec::with_capacity(input.len());
        let mut bp = Vec::with_capacity(input.len());
        for x in input {
            let (l, h, b) = node.step(x);
            lp.push(l);
            hp.push(h);
            bp.push(b);
        }
        (lp, hp, bp)
    }

    fn reset(&mut self, self_: Resource<SvfNode>) {
        if let Ok(node) = self.table.get_mut(&self_) {
            node.low = 0.0;
            node.band = 0.0;
        }
    }

    fn drop(&mut self, rep: Resource<SvfNode>) -> wasmtime::Result<()> {
        self.table.delete(rep)?;
        Ok(())
    }
}

impl dsp::HostReverb for HostState {
    fn new(&mut self, sample_rate: u32) -> Resource<ReverbNode> {
        self.table.push(ReverbNode::new(sample_rate)).expect("resource table push")
    }

    fn set_params(&mut self, self_: Resource<ReverbNode>, room_size: f32, damping: f32, wet: f32) {
        if let Ok(node) = self.table.get_mut(&self_) {
            node.fv.set_room_size(room_size.clamp(0.0, 1.0) as f64);
            node.fv.set_dampening(damping.clamp(0.0, 1.0) as f64);
            node.fv.set_wet(wet.clamp(0.0, 1.0) as f64);
        }
    }

    fn process(&mut self, self_: Resource<ReverbNode>, input_l: Vec<f32>, input_r: Vec<f32>) -> (Vec<f32>, Vec<f32>) {
        let node = match self.table.get_mut(&self_) {
            Ok(n) => n,
            Err(_) => return (vec![], vec![]),
        };
        let n = input_l.len().min(input_r.len());
        let mut out_l = Vec::with_capacity(n);
        let mut out_r = Vec::with_capacity(n);
        for i in 0..n {
            let (l, r) = node.fv.tick((input_l[i] as f64, input_r[i] as f64));
            out_l.push(l as f32);
            out_r.push(r as f32);
        }
        (out_l, out_r)
    }

    fn drop(&mut self, rep: Resource<ReverbNode>) -> wasmtime::Result<()> {
        self.table.delete(rep)?;
        Ok(())
    }
}

impl dsp::HostDelay for HostState {
    fn new(&mut self, sample_rate: u32, max_seconds: f32) -> Resource<DelayNode> {
        self.table.push(DelayNode::new(sample_rate, max_seconds)).expect("resource table push")
    }

    fn set_params(&mut self, self_: Resource<DelayNode>, time_seconds: f32, feedback: f32, mix: f32) {
        if let Ok(node) = self.table.get_mut(&self_) {
            node.set_params(time_seconds, feedback, mix);
        }
    }

    fn process(&mut self, self_: Resource<DelayNode>, input: Vec<f32>) -> Vec<f32> {
        let node = match self.table.get_mut(&self_) {
            Ok(n) => n,
            Err(_) => return vec![],
        };
        input.into_iter().map(|x| node.step(x)).collect()
    }

    fn drop(&mut self, rep: Resource<DelayNode>) -> wasmtime::Result<()> {
        self.table.delete(rep)?;
        Ok(())
    }
}

impl dsp::HostWaveshaper for HostState {
    fn new(&mut self, sample_rate: u32, oversample: u8) -> Resource<ShaperNode> {
        self.table.push(ShaperNode::new(sample_rate, oversample)).expect("resource table push")
    }

    fn set_shape(&mut self, self_: Resource<ShaperNode>, shape: ShapeKind, drive: f32, bias: f32) {
        if let Ok(node) = self.table.get_mut(&self_) {
            node.shape = shape;
            node.drive = drive;
            node.bias = bias;
        }
    }

    fn set_tone(&mut self, self_: Resource<ShaperNode>, pre_hp_hz: f32, post_lp_hz: f32) {
        if let Ok(node) = self.table.get_mut(&self_) {
            let sr = node.sr;
            node.pre_hp.set_cutoff(sr, pre_hp_hz);
            node.post_lp.set_cutoff(sr, post_lp_hz);
        }
    }

    fn process(&mut self, self_: Resource<ShaperNode>, input: Vec<f32>) -> Vec<f32> {
        let node = match self.table.get_mut(&self_) {
            Ok(n) => n,
            Err(_) => return vec![],
        };
        input.into_iter().map(|x| node.shape_one(x)).collect()
    }

    fn reset(&mut self, self_: Resource<ShaperNode>) {
        if let Ok(node) = self.table.get_mut(&self_) {
            if let Some(os) = &mut node.os {
                os.reset();
            }
            node.pre_hp.reset();
            node.post_lp.reset();
            node.dc.reset();
        }
    }

    fn drop(&mut self, rep: Resource<ShaperNode>) -> wasmtime::Result<()> {
        self.table.delete(rep)?;
        Ok(())
    }
}

impl log::Host for HostState {
    fn log(&mut self, msg: String) {
        eprintln!("[cartridge] {msg}");
    }
}

/// `types` is a type-only interface, but the world's `add_to_linker` still
/// requires its (empty) `Host` trait.
impl types::Host for HostState {}

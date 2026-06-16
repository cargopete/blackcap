//! Small DSP utilities: clipping, interleaving, one-pole filters, an SVF.

/// Interleave two equal-length mono buffers into stereo (L, R, L, R, …).
pub fn interleave(left: &[f32], right: &[f32]) -> Vec<f32> {
    debug_assert_eq!(left.len(), right.len(), "interleave needs equal-length channels");
    let mut out = Vec::with_capacity(left.len() * 2);
    for (l, r) in left.iter().zip(right.iter()) {
        out.push(*l);
        out.push(*r);
    }
    out
}

/// tanh soft clip — the metal flavour, and a cheap brickwall.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    x.tanh()
}

/// One-pole lowpass (RC). `y += a * (x - y)`.
#[derive(Clone)]
pub struct OnePoleLp {
    a: f32,
    y: f32,
}

impl OnePoleLp {
    pub fn new(sample_rate: u32, cutoff_hz: f32) -> Self {
        let mut f = Self { a: 0.0, y: 0.0 };
        f.set_cutoff(sample_rate, cutoff_hz);
        f
    }

    pub fn set_cutoff(&mut self, sample_rate: u32, cutoff_hz: f32) {
        let dt = 1.0 / sample_rate as f32;
        let rc = 1.0 / (core::f32::consts::TAU * cutoff_hz.max(1.0));
        self.a = dt / (rc + dt);
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.y += self.a * (x - self.y);
        self.y
    }

    pub fn reset(&mut self) {
        self.y = 0.0;
    }
}

/// One-pole highpass: the complement of the lowpass.
#[derive(Clone)]
pub struct OnePoleHp {
    lp: OnePoleLp,
}

impl OnePoleHp {
    pub fn new(sample_rate: u32, cutoff_hz: f32) -> Self {
        Self {
            lp: OnePoleLp::new(sample_rate, cutoff_hz),
        }
    }

    pub fn set_cutoff(&mut self, sample_rate: u32, cutoff_hz: f32) {
        self.lp.set_cutoff(sample_rate, cutoff_hz);
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        x - self.lp.process(x)
    }

    pub fn reset(&mut self) {
        self.lp.reset();
    }
}

/// Chamberlin state-variable filter. Cheap and stable well below `sr/6`; gives
/// lowpass, highpass and bandpass from one structure. A full host-side SVF with
/// proper oversampling lands at M3 — this is the inline SDK version.
#[derive(Clone)]
pub struct Svf {
    sr: f32,
    f: f32,
    q: f32,
    low: f32,
    band: f32,
}

impl Svf {
    pub fn new(sample_rate: u32) -> Self {
        let mut svf = Self {
            sr: sample_rate as f32,
            f: 0.0,
            q: 1.0,
            low: 0.0,
            band: 0.0,
        };
        svf.set_params(1000.0, 0.7);
        svf
    }

    pub fn set_params(&mut self, cutoff_hz: f32, q: f32) {
        let fc = cutoff_hz.clamp(20.0, self.sr * 0.45);
        self.f = 2.0 * (core::f32::consts::PI * fc / self.sr).sin();
        self.q = q.max(0.5);
    }

    /// Returns `(low, high, band)`.
    #[inline]
    pub fn process_one(&mut self, x: f32) -> (f32, f32, f32) {
        let damp = 1.0 / self.q;
        let high = x - self.low - damp * self.band;
        self.band += self.f * high;
        self.low += self.f * self.band;
        (self.low, high, self.band)
    }

    pub fn reset(&mut self) {
        self.low = 0.0;
        self.band = 0.0;
    }
}

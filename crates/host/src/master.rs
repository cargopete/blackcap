//! The master bus chain — glue compressor → brickwall limiter. Applied in the
//! mixer thread, after the crossfade and before the output ring, so every
//! cartridge gets consistent loudness and the limiter protects the device from
//! a cartridge that returns hot samples.

#[inline]
fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[inline]
fn time_coef(seconds: f32, sample_rate: f32) -> f32 {
    if seconds <= 0.0 {
        0.0
    } else {
        (-1.0 / (seconds * sample_rate)).exp()
    }
}

/// Stereo-linked feed-forward compressor for bus glue.
pub struct Compressor {
    sr: f32,
    threshold: f32, // linear
    ratio: f32,
    attack: f32,
    release: f32,
    makeup: f32, // linear
    gain: f32,   // smoothed gain reduction, linear
}

impl Compressor {
    pub fn new(sample_rate: u32) -> Self {
        let mut c = Self {
            sr: sample_rate as f32,
            threshold: 1.0,
            ratio: 1.0,
            attack: 0.0,
            release: 0.0,
            makeup: 1.0,
            gain: 1.0,
        };
        // Gentle glue by default: -18 dB, 2:1, 10/120 ms, +3 dB makeup.
        c.set_params(-18.0, 2.0, 10.0, 120.0, 3.0);
        c
    }

    pub fn set_params(
        &mut self,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_db: f32,
    ) {
        self.threshold = db_to_lin(threshold_db);
        self.ratio = ratio.max(1.0);
        self.attack = time_coef(attack_ms * 0.001, self.sr);
        self.release = time_coef(release_ms * 0.001, self.sr);
        self.makeup = db_to_lin(makeup_db);
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let level = l.abs().max(r.abs());
        // Static gain curve: how much to reduce, in linear gain.
        let target = if level > self.threshold && level > 0.0 {
            let over_db = 20.0 * (level / self.threshold).log10();
            db_to_lin(over_db * (1.0 / self.ratio - 1.0)) // negative dB -> < 1.0
        } else {
            1.0
        };
        // Attack when clamping down, release when letting go.
        let coef = if target < self.gain {
            self.attack
        } else {
            self.release
        };
        self.gain = coef * self.gain + (1.0 - coef) * target;
        let g = self.gain * self.makeup;
        (l * g, r * g)
    }

    /// Reserved for cartridge-reload state reset (wired at M6).
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.gain = 1.0;
    }
}

/// Look-ahead brickwall limiter. The gain envelope is derived from the *future*
/// sample (peek) and applied to a delayed copy, so reduction precedes the peak.
pub struct Limiter {
    ceiling: f32,
    release: f32,
    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    pos: usize,
    gain: f32,
}

impl Limiter {
    pub fn new(sample_rate: u32) -> Self {
        let mut lim = Self {
            ceiling: 1.0,
            release: 0.0,
            delay_l: Vec::new(),
            delay_r: Vec::new(),
            pos: 0,
            gain: 1.0,
        };
        // Ceiling -0.3 dBFS, 50 ms release, ~2 ms look-ahead.
        lim.set_params(sample_rate, -0.3, 50.0, 2.0);
        lim
    }

    pub fn set_params(&mut self, sample_rate: u32, ceiling_db: f32, release_ms: f32, lookahead_ms: f32) {
        self.ceiling = db_to_lin(ceiling_db);
        self.release = time_coef(release_ms * 0.001, sample_rate as f32);
        let n = ((lookahead_ms * 0.001 * sample_rate as f32) as usize).max(1);
        self.delay_l = vec![0.0; n];
        self.delay_r = vec![0.0; n];
        self.pos = 0;
        self.gain = 1.0;
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        // Target gain from the incoming (future) peak.
        let peak = l.abs().max(r.abs());
        let target = if peak > self.ceiling {
            self.ceiling / peak
        } else {
            1.0
        };
        // Instant attack (clamp now), smooth release.
        if target < self.gain {
            self.gain = target;
        } else {
            self.gain = self.release * self.gain + (1.0 - self.release) * target;
        }

        // Read the delayed sample, write the current one.
        let dl = self.delay_l[self.pos];
        let dr = self.delay_r[self.pos];
        self.delay_l[self.pos] = l;
        self.delay_r[self.pos] = r;
        self.pos = (self.pos + 1) % self.delay_l.len();

        // Apply gain to the delayed sample; hard clamp as a final safety net.
        let out_l = (dl * self.gain).clamp(-self.ceiling, self.ceiling);
        let out_r = (dr * self.gain).clamp(-self.ceiling, self.ceiling);
        (out_l, out_r)
    }

    /// Reserved for cartridge-reload state reset (wired at M6).
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.delay_l.iter_mut().for_each(|s| *s = 0.0);
        self.delay_r.iter_mut().for_each(|s| *s = 0.0);
        self.pos = 0;
        self.gain = 1.0;
    }
}

/// The full master chain: compressor → limiter.
pub struct MasterChain {
    comp: Compressor,
    limiter: Limiter,
    enabled: bool,
}

impl MasterChain {
    pub fn new(sample_rate: u32, enabled: bool) -> Self {
        Self {
            comp: Compressor::new(sample_rate),
            limiter: Limiter::new(sample_rate),
            enabled,
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        if !self.enabled {
            return (l, r);
        }
        let (l, r) = self.comp.process(l, r);
        self.limiter.process(l, r)
    }
}

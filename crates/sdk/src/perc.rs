//! Synthesised percussion (RFC part 2). Drum *design* is per-song creative
//! choice with trivial state, so it lives cartridge-side, not in the host.

use crate::dsp::{OnePoleHp, OnePoleLp, Svf};
use crate::osc::white;

/// Kick: a pitch-dropped sine body, a short low-passed noise "beater" click on
/// top, and tanh saturation for beef. `metal()` / `doom()` give sensible tunings.
#[derive(Clone)]
pub struct KickVoice {
    sr: f32,
    phase: f32,
    t: f32,
    active: bool,
    f_start: f32,
    f_end: f32,
    pitch_decay: f32,
    attack: f32,
    amp_decay: f32,
    click_amt: f32,
    click_lp: OnePoleLp,
    rng: u64,
    drive: f32,
}

impl KickVoice {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sample_rate: u32,
        f_start: f32,
        f_end: f32,
        pitch_decay: f32,
        amp_decay: f32,
        click_amt: f32,
        drive: f32,
    ) -> Self {
        Self {
            sr: sample_rate as f32,
            phase: 0.0,
            t: 0.0,
            active: false,
            f_start,
            f_end,
            pitch_decay,
            attack: 0.0005,
            amp_decay,
            click_amt,
            click_lp: OnePoleLp::new(sample_rate, 4000.0),
            rng: 0x9E37_79B9_7F4A_7C15,
            drive,
        }
    }

    /// Tight, clicky djent kick.
    pub fn metal(sample_rate: u32) -> Self {
        Self::new(sample_rate, 140.0, 45.0, 0.02, 0.16, 0.7, 2.2)
    }

    /// Slow, boomy doom/sludge kick.
    pub fn doom(sample_rate: u32) -> Self {
        Self::new(sample_rate, 90.0, 40.0, 0.05, 0.4, 0.2, 1.5)
    }

    pub fn trigger(&mut self) {
        self.t = 0.0;
        self.phase = 0.0;
        self.active = true;
    }

    pub fn render_block(&mut self, num_frames: u32) -> Vec<f32> {
        let dt = 1.0 / self.sr;
        let mut out = vec![0.0f32; num_frames as usize];
        if !self.active {
            return out;
        }
        for o in out.iter_mut() {
            let f = self.f_end + (self.f_start - self.f_end) * (-self.t / self.pitch_decay).exp();
            self.phase = (self.phase + f * dt).fract();
            let body = (core::f32::consts::TAU * self.phase).sin();

            let a = if self.t < self.attack {
                self.t / self.attack
            } else {
                (-(self.t - self.attack) / self.amp_decay).exp()
            };

            let click_env = (-self.t / 0.004).exp();
            let click = self.click_lp.process(white(&mut self.rng)) * click_env * self.click_amt;

            *o = (self.drive * body * a).tanh() + click;

            self.t += dt;
            if self.t > self.amp_decay * 6.0 {
                self.active = false;
            }
        }
        out
    }

    pub fn reset(&mut self) {
        self.active = false;
        self.t = 0.0;
        self.phase = 0.0;
        self.click_lp.reset();
    }
}

/// Snare: a tonal body with its own little pitch drop, summed with a
/// band-passed noise burst (the "snares"), both on steep decays.
#[derive(Clone)]
pub struct SnareVoice {
    sr: f32,
    t: f32,
    active: bool,
    tone_phase: f32,
    f_tone_start: f32,
    f_tone_end: f32,
    tone_pitch_decay: f32,
    tone_decay: f32,
    tone_amt: f32,
    noise_bp: Svf,
    noise_decay: f32,
    noise_amt: f32,
    rng: u64,
    drive: f32,
}

impl SnareVoice {
    pub fn new(sample_rate: u32) -> Self {
        let mut noise_bp = Svf::new(sample_rate);
        noise_bp.set_params(3200.0, 3.0);
        Self {
            sr: sample_rate as f32,
            t: 0.0,
            active: false,
            tone_phase: 0.0,
            f_tone_start: 210.0,
            f_tone_end: 180.0,
            tone_pitch_decay: 0.02,
            tone_decay: 0.09,
            tone_amt: 0.5,
            noise_bp,
            noise_decay: 0.14,
            noise_amt: 0.7,
            rng: 0xD1B5_4A32_D192_ED03,
            drive: 1.5,
        }
    }

    /// Noise-forward crack for metalcore.
    pub fn metalcore(sample_rate: u32) -> Self {
        let mut s = Self::new(sample_rate);
        s.noise_amt = 0.85;
        s.tone_amt = 0.4;
        s.noise_decay = 0.12;
        s.noise_bp.set_params(3500.0, 3.0);
        s
    }

    pub fn trigger(&mut self) {
        self.t = 0.0;
        self.tone_phase = 0.0;
        self.active = true;
    }

    pub fn render_block(&mut self, num_frames: u32) -> Vec<f32> {
        let dt = 1.0 / self.sr;
        let mut out = vec![0.0f32; num_frames as usize];
        if !self.active {
            return out;
        }
        for o in out.iter_mut() {
            let f = self.f_tone_end
                + (self.f_tone_start - self.f_tone_end) * (-self.t / self.tone_pitch_decay).exp();
            self.tone_phase = (self.tone_phase + f * dt).fract();
            let tone = (core::f32::consts::TAU * self.tone_phase).sin()
                * (-self.t / self.tone_decay).exp()
                * self.tone_amt;

            let (_, _, bp) = self.noise_bp.process_one(white(&mut self.rng));
            let noise = bp * (-self.t / self.noise_decay).exp() * self.noise_amt;

            *o = (self.drive * (tone + noise)).tanh();

            self.t += dt;
            if self.t > self.noise_decay * 6.0 {
                self.active = false;
            }
        }
        out
    }

    pub fn reset(&mut self) {
        self.active = false;
        self.t = 0.0;
        self.noise_bp.reset();
    }
}

/// Cymbal: high-passed white noise with a slow exponential decay. `crash()` is
/// wideband; `china()` is brighter and shorter.
#[derive(Clone)]
pub struct CymbalVoice {
    sr: f32,
    t: f32,
    active: bool,
    hp: OnePoleHp,
    decay: f32,
    amt: f32,
    rng: u64,
}

impl CymbalVoice {
    pub fn new(sample_rate: u32, hp_hz: f32, decay: f32, amt: f32) -> Self {
        Self {
            sr: sample_rate as f32,
            t: 0.0,
            active: false,
            hp: OnePoleHp::new(sample_rate, hp_hz),
            decay,
            amt,
            rng: 0x2545_F491_4F6C_DD1D,
        }
    }

    pub fn crash(sample_rate: u32) -> Self {
        Self::new(sample_rate, 5000.0, 1.2, 0.5)
    }

    pub fn china(sample_rate: u32) -> Self {
        Self::new(sample_rate, 7000.0, 0.9, 0.6)
    }

    pub fn trigger(&mut self) {
        self.t = 0.0;
        self.active = true;
    }

    pub fn render_block(&mut self, num_frames: u32) -> Vec<f32> {
        let dt = 1.0 / self.sr;
        let mut out = vec![0.0f32; num_frames as usize];
        if !self.active {
            return out;
        }
        for o in out.iter_mut() {
            let env = (-self.t / self.decay).exp();
            *o = self.hp.process(white(&mut self.rng)) * env * self.amt;
            self.t += dt;
            if self.t > self.decay * 6.0 {
                self.active = false;
            }
        }
        out
    }

    pub fn reset(&mut self) {
        self.active = false;
        self.t = 0.0;
        self.hp.reset();
    }
}

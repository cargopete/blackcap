//! Convenience voices: thin wrappers over the oscillators with a `note_on`/
//! `render_block` shape that reads well in a cartridge.

use crate::osc::{Noise, Osc, SuperSaw, Waveform};

/// `N`-voice detuned supersaw lead.
#[derive(Clone)]
pub struct SawSuperVoice<const N: usize> {
    saw: SuperSaw<N>,
}

impl<const N: usize> SawSuperVoice<N> {
    pub fn new(sample_rate: u32, detune_cents: f32) -> Self {
        Self {
            saw: SuperSaw::new(sample_rate, detune_cents),
        }
    }

    pub fn note_on(&mut self, hz: f32) {
        self.saw.set_freq(hz);
    }

    pub fn render_block(&mut self, num_frames: u32) -> Vec<f32> {
        self.saw.render_block(num_frames)
    }

    pub fn reset(&mut self) {
        self.saw.hard_reset();
    }
}

/// Pulse-width square voice — handy for retro bass.
#[derive(Clone)]
pub struct SquareVoice {
    osc: Osc,
}

impl SquareVoice {
    pub fn new(sample_rate: u32, pulse_width: f32) -> Self {
        let mut osc = Osc::new(sample_rate, Waveform::Square);
        osc.pulse_width = pulse_width;
        Self { osc }
    }

    pub fn note_on(&mut self, hz: f32) {
        self.osc.set_freq(hz);
    }

    pub fn render_block(&mut self, num_frames: u32) -> Vec<f32> {
        self.osc.render_block(num_frames)
    }

    pub fn reset(&mut self) {
        self.osc.reset();
    }
}

/// Raw white-noise source for hats; shape it with an [`crate::env::Adsr`] and a
/// band-pass in the cartridge.
#[derive(Clone)]
pub struct NoiseHat {
    noise: Noise,
}

impl NoiseHat {
    pub fn new(seed: u64) -> Self {
        Self {
            noise: Noise::new(seed),
        }
    }

    pub fn render_block(&mut self, num_frames: u32) -> Vec<f32> {
        self.noise.render_block(num_frames)
    }

    pub fn reset(&mut self) {}
}

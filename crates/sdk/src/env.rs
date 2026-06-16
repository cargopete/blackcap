//! Envelopes: a gated ADSR and a de-clicking rhythmic [`Gate`].

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Linear ADSR. `trigger()` opens the gate (attack→decay→sustain), `release()`
/// closes it (→release→idle). For a pluck/percussion envelope set
/// `sustain = 0.0` and it self-finishes after decay.
#[derive(Clone)]
pub struct Adsr {
    sr: f32,
    stage: Stage,
    level: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
}

impl Adsr {
    /// Times in seconds; `sustain` is a level in `0..1`.
    pub fn new(sample_rate: u32, attack: f32, decay: f32, sustain: f32, release: f32) -> Self {
        Self {
            sr: sample_rate as f32,
            stage: Stage::Idle,
            level: 0.0,
            attack,
            decay,
            sustain: sustain.clamp(0.0, 1.0),
            release,
        }
    }

    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    pub fn active(&self) -> bool {
        self.stage != Stage::Idle
    }

    #[inline]
    fn per_sample(secs: f32, sr: f32) -> f32 {
        if secs <= 0.0 {
            1.0
        } else {
            1.0 / (secs * sr)
        }
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => self.level = 0.0,
            Stage::Attack => {
                self.level += Self::per_sample(self.attack, self.sr);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level -= Self::per_sample(self.decay, self.sr) * (1.0 - self.sustain).max(0.001);
                if self.level <= self.sustain {
                    self.level = self.sustain;
                    self.stage = if self.sustain <= 0.0 {
                        Stage::Idle
                    } else {
                        Stage::Sustain
                    };
                }
            }
            Stage::Sustain => self.level = self.sustain,
            Stage::Release => {
                self.level -= Self::per_sample(self.release, self.sr) * self.sustain.max(0.001);
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }

    pub fn render_block(&mut self, num_frames: u32) -> Vec<f32> {
        (0..num_frames).map(|_| self.next()).collect()
    }
}

/// A de-clicking gate: open/closed states applied as short linear ramps rather
/// than a binary multiply, so palm-mute chugs don't click at every edge. A
/// slightly longer release than attack reads as "choke" rather than "cut".
#[derive(Clone)]
pub struct Gate {
    level: f32,
    target: f32,
    atk_per_sample: f32,
    rel_per_sample: f32,
}

impl Gate {
    pub fn new(sample_rate: u32, atk_ms: f32, rel_ms: f32) -> Self {
        let sr = sample_rate as f32;
        Self {
            level: 0.0,
            target: 0.0,
            atk_per_sample: 1.0 / (atk_ms.max(0.01) * 0.001 * sr),
            rel_per_sample: 1.0 / (rel_ms.max(0.01) * 0.001 * sr),
        }
    }

    /// Open or close the gate. `accent` scales the open level (e.g. 1.0 normal,
    /// ~1.4 for an accented hit, ~0.3 for a ghost).
    pub fn set(&mut self, open: bool, accent: f32) {
        self.target = if open { accent } else { 0.0 };
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let step = if self.target > self.level {
            self.atk_per_sample
        } else {
            self.rel_per_sample
        };
        let diff = self.target - self.level;
        if diff.abs() <= step {
            self.level = self.target;
        } else {
            self.level += step.copysign(diff);
        }
        self.level
    }

    pub fn reset(&mut self) {
        self.level = 0.0;
        self.target = 0.0;
    }
}

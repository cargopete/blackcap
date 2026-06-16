//! Oscillators: band-limited (PolyBLEP) saw/square, naive sine/triangle, a
//! detuned supersaw, and white noise.

/// PolyBLEP residual for band-limiting discontinuous waveforms.
#[inline]
pub fn poly_blep(t: f32, dt: f32) -> f32 {
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

/// One white-noise sample from a raw xorshift64 state (which it advances).
/// Handy for percussion voices that store a bare `u64` seed.
#[inline]
pub fn white(state: &mut u64) -> f32 {
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

/// A single phase-accumulating oscillator. Saw and square are PolyBLEP
/// band-limited; sine and triangle are computed directly.
#[derive(Clone)]
pub struct Osc {
    sr: f32,
    phase: f32, // 0..1
    freq: f32,
    pub waveform: Waveform,
    pub pulse_width: f32, // square duty, 0..1
}

impl Osc {
    pub fn new(sample_rate: u32, waveform: Waveform) -> Self {
        Self {
            sr: sample_rate as f32,
            phase: 0.0,
            freq: 440.0,
            waveform,
            pulse_width: 0.5,
        }
    }

    pub fn set_freq(&mut self, hz: f32) {
        self.freq = hz;
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let dt = self.freq / self.sr;
        let p = self.phase;
        let s = match self.waveform {
            Waveform::Sine => (core::f32::consts::TAU * p).sin(),
            Waveform::Saw => (2.0 * p - 1.0) - poly_blep(p, dt),
            Waveform::Square => {
                let pw = self.pulse_width;
                let mut s = if p < pw { 1.0 } else { -1.0 };
                s += poly_blep(p, dt);
                s -= poly_blep((p + 1.0 - pw).fract(), dt);
                s
            }
            Waveform::Triangle => {
                if p < 0.5 {
                    4.0 * p - 1.0
                } else {
                    3.0 - 4.0 * p
                }
            }
        };
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        s
    }

    pub fn render_block(&mut self, num_frames: u32) -> Vec<f32> {
        (0..num_frames).map(|_| self.next()).collect()
    }
}

/// `N` detuned saws, free-running (phases never reset on note-on — that's what
/// keeps a supersaw from sounding sterile), summed and amplitude-compensated.
#[derive(Clone)]
pub struct SuperSaw<const N: usize> {
    oscs: [Osc; N],
    detune_cents: f32,
    norm: f32,
}

impl<const N: usize> SuperSaw<N> {
    pub fn new(sample_rate: u32, detune_cents: f32) -> Self {
        // Spread initial phases so the stack doesn't start in unison.
        let oscs = core::array::from_fn(|i| {
            let mut osc = Osc::new(sample_rate, Waveform::Saw);
            osc.phase = i as f32 / N as f32;
            osc
        });
        Self {
            oscs,
            detune_cents,
            norm: 1.0 / (N as f32).sqrt(),
        }
    }

    pub fn set_freq(&mut self, hz: f32) {
        for (i, osc) in self.oscs.iter_mut().enumerate() {
            let spread = if N > 1 {
                (i as f32 / (N as f32 - 1.0)) * 2.0 - 1.0
            } else {
                0.0
            };
            osc.set_freq(hz * 2f32.powf(spread * self.detune_cents / 1200.0));
        }
    }

    /// Free-running: deliberately does NOT reset phase. Use [`Self::hard_reset`]
    /// for a full state reset (cartridge reload / loop).
    pub fn hard_reset(&mut self) {
        for osc in &mut self.oscs {
            osc.reset();
        }
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut sum = 0.0;
        for osc in &mut self.oscs {
            sum += osc.next();
        }
        sum * self.norm
    }

    pub fn render_block(&mut self, num_frames: u32) -> Vec<f32> {
        (0..num_frames).map(|_| self.next()).collect()
    }
}

/// White-noise generator wrapping the [`white`] xorshift.
#[derive(Clone)]
pub struct Noise {
    state: u64,
}

impl Noise {
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed | 1,
        }
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        white(&mut self.state)
    }

    pub fn render_block(&mut self, num_frames: u32) -> Vec<f32> {
        (0..num_frames).map(|_| self.next()).collect()
    }
}

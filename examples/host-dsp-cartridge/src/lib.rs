//! "Eutectic (Hosted)" — the same drop-A gated chug as the breakdown demo, but
//! the heavy DSP is done by the host: the oscillator is `dsp::osc_square`, the
//! crunch is the host's oversampled anti-aliased `dsp::Waveshaper`, and the tail
//! is a `dsp::Reverb` send. The cartridge itself carries almost no DSP code, so
//! its wasm is small — that's the point of host imports.

use jukebox_cartridge_sdk::dsp::{self, ShapeKind, Reverb, Waveshaper};
use jukebox_cartridge_sdk::prelude::*;

const SR: u32 = 48_000;

const RIFF: TrackerSong = song! {
    tempo: 150;
    rows_per_beat: 4;

    pattern "drop" {
        chug_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:      "X-x- x-x- X-xx -x-x  X-x- x-x- X-x- xxxx";
    }

    sequence: [drop, drop];
};

struct Hosted {
    phase: f32,
    freq: f32,
    gate: Gate,
    shaper: Waveshaper,
    reverb: Reverb,
    song: CompiledSong,
}

impl Player for Hosted {
    fn init(sample_rate: u32) -> Result<Self, String> {
        if sample_rate != SR {
            return Err(format!("host-dsp is authored at {SR} Hz, host offered {sample_rate} Hz"));
        }

        // Oversampled (×4) asymmetric tanh: the metalcore chug crunch.
        let shaper = Waveshaper::new(SR, 4);
        shaper.set_shape(ShapeKind::AsymTanh, 4.5, 0.15);
        shaper.set_tone(100.0, 8000.0);

        let reverb = Reverb::new(SR);
        reverb.set_params(0.6, 0.5, 0.22);

        log::log("host-dsp cartridge loaded: ×4 waveshaper + reverb send");

        Ok(Self {
            phase: 0.0,
            freq: 55.0,
            gate: Gate::new(SR, 2.0, 6.0),
            shaper,
            reverb,
            song: RIFF.compile(SR)?,
        })
    }

    fn render(&mut self, start_frame: u64, num_frames: u32) -> Vec<f32> {
        let n = num_frames as usize;

        for ev in self.song.events_in_range(start_frame, num_frames as u64) {
            match (ev.lane, ev.cell) {
                ("chug_note", Cell::Note(note)) => self.freq = note.hz(),
                ("gate", Cell::Hit { accent }) => self.gate.set(true, if accent { 1.4 } else { 1.0 }),
                ("gate", Cell::Ghost) => self.gate.set(true, 0.3),
                ("gate", Cell::Off) => self.gate.set(false, 0.0),
                _ => {}
            }
        }

        // Host oscillator (stateless; we keep the phase).
        let (raw, new_phase) = dsp::osc_square(self.freq, self.phase, 0.5, num_frames, SR);
        self.phase = new_phase;

        // De-click gate (inline; it's tiny).
        let mut gated = vec![0.0f32; n];
        for i in 0..n {
            gated[i] = raw[i] * self.gate.next();
        }

        // Host oversampled crunch, then a host reverb send.
        let crunch = self.shaper.process(&gated);
        let (wet_l, wet_r) = self.reverb.process(&crunch, &crunch);

        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            out.push(soft_clip(crunch[i] + wet_l[i]));
            out.push(soft_clip(crunch[i] + wet_r[i]));
        }
        out
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.gate.reset();
        self.shaper.reset();
    }

    fn is_finished(&self) -> bool {
        false
    }

    fn metadata(&self) -> Metadata {
        Metadata {
            title: "Eutectic (Hosted)".to_string(),
            artist: "blackcap".to_string(),
            duration_frames: 0,
            sample_rate: SR,
            loop_point: Some(LoopPoint {
                start_frame: 0,
                end_frame: self.song.duration_frames(),
            }),
            cover_art: None,
            tags: vec!["metalcore".to_string(), "host-dsp".to_string()],
        }
    }
}

export_player!(Hosted);

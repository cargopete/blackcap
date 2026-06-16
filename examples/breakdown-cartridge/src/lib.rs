//! "Eutectic (Breakdown)" — a drop-A metalcore breakdown built from SDK
//! primitives: a held low chug gated into a palm-mute rhythm, run through a
//! tighten-HP → soft-clip → tone-LP chain, plus synthesised kick/snare/crash
//! with a sample-accurate kick→chug sidechain duck.
//!
//! The oversampled anti-aliased `waveshaper` and master limiter from the RFC
//! are host imports that arrive at M3; for now the distortion is an inline
//! `tanh` with tightening filters, which is honest M2 territory.

use jukebox_cartridge_sdk::dsp::{OnePoleHp, OnePoleLp};
use jukebox_cartridge_sdk::prelude::*;

const SR: u32 = 48_000;
const DRIVE: f32 = 4.5;

const BREAKDOWN: TrackerSong = song! {
    tempo: 150;
    rows_per_beat: 4;

    pattern "drop" {
        // Held drop-A drone: one note-on, then it rings (gated below).
        chug_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        // Palm-mute rhythm: X accent, x normal, . ghost, - muted.
        gate:      "X-x- x-x- X-xx -x-x  X-x- x-x- X--- ----";
        kick:      "x--- x--- x-x- ---x  x--- x--- x--- ----";
        snare:     "---- x--- ---- x---  ---- x--- ---- x---";
        crash:     "x--- ---- ---- ----  ---- ---- ---- ----";
    }

    sequence: [drop, drop, drop, drop];
};

struct Breakdown {
    chug: SquareVoice,
    gate: Gate,
    pre_hp: OnePoleHp,
    post_lp: OnePoleLp,
    kick: KickVoice,
    snare: SnareVoice,
    crash: CymbalVoice,
    song: CompiledSong,
}

impl Player for Breakdown {
    fn init(sample_rate: u32) -> Result<Self, String> {
        if sample_rate != SR {
            return Err(format!("breakdown is authored at {SR} Hz, host offered {sample_rate} Hz"));
        }
        Ok(Self {
            chug: SquareVoice::new(SR, 0.5),
            gate: Gate::new(SR, 2.0, 6.0), // de-click: 2 ms attack, 6 ms choke
            pre_hp: OnePoleHp::new(SR, 95.0),
            post_lp: OnePoleLp::new(SR, 8000.0),
            kick: KickVoice::metal(SR),
            snare: SnareVoice::metalcore(SR),
            crash: CymbalVoice::crash(SR),
            song: BREAKDOWN.compile(SR)?,
        })
    }

    fn render(&mut self, start_frame: u64, num_frames: u32) -> Vec<f32> {
        let n = num_frames as usize;

        for ev in self.song.events_in_range(start_frame, num_frames as u64) {
            match (ev.lane, ev.cell) {
                ("chug_note", Cell::Note(note)) => self.chug.note_on(note.hz()),
                ("gate", cell) => match cell {
                    Cell::Hit { accent } => self.gate.set(true, if accent { 1.4 } else { 1.0 }),
                    Cell::Ghost => self.gate.set(true, 0.3),
                    Cell::Off => self.gate.set(false, 0.0),
                    Cell::Note(_) => {}
                },
                ("kick", Cell::Hit { .. }) => self.kick.trigger(),
                ("snare", Cell::Hit { .. }) => self.snare.trigger(),
                ("crash", Cell::Hit { .. }) => self.crash.trigger(),
                _ => {}
            }
        }

        let chug_raw = self.chug.render_block(num_frames);
        let kick = self.kick.render_block(num_frames);
        let snare = self.snare.render_block(num_frames);
        let crash = self.crash.render_block(num_frames);

        let mut out_l = vec![0.0f32; n];
        let mut out_r = vec![0.0f32; n];

        for i in 0..n {
            // chug: gate → tighten HP → drive/clip → tone LP
            let gated = chug_raw[i] * self.gate.next();
            let pre = self.pre_hp.process(gated);
            let dist = soft_clip(pre * DRIVE);
            let chug = self.post_lp.process(dist);

            // sample-accurate kick→chug duck (the breakdown "pump")
            let duck = 1.0 - 0.5 * kick[i].abs().min(1.0);

            let mix = 0.55 * chug * duck + 0.90 * kick[i] + 0.70 * snare[i] + 0.30 * crash[i];
            let mix = soft_clip(mix);
            out_l[i] = mix;
            out_r[i] = mix;
        }

        interleave(&out_l, &out_r)
    }

    fn reset(&mut self) {
        self.chug.reset();
        self.gate.reset();
        self.pre_hp.reset();
        self.post_lp.reset();
        self.kick.reset();
        self.snare.reset();
        self.crash.reset();
    }

    fn is_finished(&self) -> bool {
        false
    }

    fn metadata(&self) -> Metadata {
        Metadata {
            title: "Eutectic (Breakdown)".to_string(),
            artist: "blackcap".to_string(),
            duration_frames: 0,
            sample_rate: SR,
            loop_point: Some(LoopPoint {
                start_frame: 0,
                end_frame: self.song.duration_frames(),
            }),
            cover_art: None,
            tags: vec![
                "metalcore".to_string(),
                "breakdown".to_string(),
                "synthcore".to_string(),
            ],
        }
    }
}

export_player!(Breakdown);

//! "Eutectic (Breakdown)" — an actual drop-A breakdown in the metalcore /
//! deathcore sense: a slow **half-time lurch** (snare on beat 3 only, not a
//! 2-and-4 backbeat), syncopated low chugs with big gaps, the kick locked to
//! the chug, and a sub-octave sine under the low note for floor-weight. Two
//! feels: the main syncopated groove and a sparse two-step "mosh" variant.
//!
//! All inline DSP (no host imports): gate → tighten-HP → soft-clip drive →
//! tone-LP, plus a separate (un-highpassed) sub. The point of a breakdown is
//! weight and groove, not notes — so this is deliberately simple and spacious.

use jukebox_cartridge_sdk::fx::{OnePoleHp, OnePoleLp};
use jukebox_cartridge_sdk::osc::{Osc, Waveform};
use jukebox_cartridge_sdk::prelude::*;

const SR: u32 = 48_000;
const DRIVE: f32 = 5.5;

const BREAKDOWN: TrackerSong = song! {
    tempo: 130; // slow — a breakdown should lurch, not drive
    rows_per_beat: 4;

    // The main groove: accented chug on 1 (rings), syncopated stabs, big space
    // around beat 3 where the half-time snare lands. Bar 2 ends in a 16th burst.
    pattern "main" {
        chug_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:      "Xx-- --x- --x- xx--  Xx-- --x- -x-x xxxx";
        kick:      "x--- --x- --x- xx--  x--- --x- -x-x xxxx";
        snare:     "---- ---- x--- ----  ---- ---- x--- ----";
        crash:     "x--- ---- ---- ----  x--- ---- ---- ----";
    }

    // The two-step: hugely spaced — one big chug on 1, one on the "and" of 3.
    // This is the mosh part; the silence between hits is the instrument.
    pattern "stomp" {
        chug_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:      "X--- ---- --x- ----  X--- ---- --x- ----";
        kick:      "x--- ---- --x- ----  x--- ---- --x- ----";
        snare:     "---- ---- x--- ----  ---- ---- x--- ----";
        crash:     "x--- ---- ---- ----  x--- ---- ---- ----";
    }

    sequence: [main, main, stomp, main, main, stomp, stomp, main];
};

struct Breakdown {
    chug: SquareVoice,
    sub: Osc,
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
            sub: Osc::new(SR, Waveform::Sine),
            gate: Gate::new(SR, 2.0, 7.0), // de-click: 2 ms attack, 7 ms choke
            pre_hp: OnePoleHp::new(SR, 45.0), // keep the 55 Hz fundamental (was 95 — too thin)
            post_lp: OnePoleLp::new(SR, 7000.0),
            kick: KickVoice::metal(SR),
            snare: SnareVoice::metalcore(SR),
            crash: CymbalVoice::china(SR),
            song: BREAKDOWN.compile(SR)?,
        })
    }

    fn render(&mut self, start_frame: u64, num_frames: u32) -> Vec<f32> {
        let n = num_frames as usize;

        for ev in self.song.events_in_range(start_frame, num_frames as u64) {
            match (ev.lane, ev.cell) {
                ("chug_note", Cell::Note(note)) => {
                    self.chug.note_on(note.hz());
                    self.sub.set_freq(note.hz() * 0.5); // octave-down floor-weight
                }
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
        let sub_raw = self.sub.render_block(num_frames);
        let kick = self.kick.render_block(num_frames);
        let snare = self.snare.render_block(num_frames);
        let crash = self.crash.render_block(num_frames);

        let mut out_l = vec![0.0f32; n];
        let mut out_r = vec![0.0f32; n];

        for i in 0..n {
            let g = self.gate.next();

            // chug: gate → tighten HP → drive/clip → tone LP
            let pre = self.pre_hp.process(chug_raw[i] * g);
            let chug = self.post_lp.process(soft_clip(pre * DRIVE));

            // sub: same gate, NOT high-passed — this is the low-end weight.
            let sub = soft_clip(sub_raw[i] * g * 1.2);

            // sample-accurate kick→chug duck (the breakdown "pump")
            let duck = 1.0 - 0.5 * kick[i].abs().min(1.0);

            let mix = soft_clip(
                0.50 * chug * duck
                    + 0.32 * sub * duck
                    + 0.95 * kick[i]
                    + 0.70 * snare[i]
                    + 0.30 * crash[i],
            );
            out_l[i] = mix;
            out_r[i] = mix;
        }

        interleave(&out_l, &out_r)
    }

    fn reset(&mut self) {
        self.chug.reset();
        self.sub.reset();
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
            duration_frames: self.song.duration_frames(),
            sample_rate: SR,
            loop_point: Some(LoopPoint {
                start_frame: 0,
                end_frame: self.song.duration_frames(),
            }),
            cover_art: None,
            tags: vec![
                "metalcore".to_string(),
                "breakdown".to_string(),
                "deathcore".to_string(),
            ],
        }
    }
}

export_player!(Breakdown);

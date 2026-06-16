//! "Blackstar" — electronicore in The Browning / "Skybreaker" mould: A phrygian,
//! drop-A, 176 bpm. Dry and brutal, not pretty. Machine-gun double-bass under
//! chromatic chugs, a screaming hard-driven supersaw lead, and — the signature —
//! a half-time **dubstep wobble bass**: a saw through a resonant SVF whose cutoff
//! is swept by a tempo-synced LFO, then driven hard. Minimal reverb; the master
//! limiter does the gluing.
//!
//! Where Severance is synthwave-leaning (lush, euphoric, mid-tempo), this is the
//! heavy end of the genre: drops crush instead of soar.

use jukebox_cartridge_sdk::dsp::{Reverb, ShapeKind, Waveshaper};
use jukebox_cartridge_sdk::osc::{white, Osc, Waveform};
use jukebox_cartridge_sdk::prelude::*;

const SR: u32 = 48_000;
const TEMPO: f32 = 176.0;
/// Wobble LFO at an eighth note: bpm/60 * 2 cycles per second.
const WOBBLE_HZ: f32 = TEMPO / 60.0 * 2.0;

const SONG: TrackerSong = song! {
    tempo: 176;
    rows_per_beat: 4; // 16th grid; 2 bars (32 cells) per pattern

    pattern "intro" {
        lead:      "a4 -  -  -   -  -  -  -   -  -  -  -   -  -  -  -    a4 -  -  -   bb4 - c5 -   d5 -  e5 -   f5 -  e5 -";
        chug_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    .  .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:      "X--- ---- ---- ----  ---- ---- x-x- xxxx";
        kick:      "x--- ---- ---- ----  ---- ---- x-x- xxxx";
        crash:     "x--- ---- ---- ----  ---- ---- ---- ----";
    }

    pattern "drop" {
        // The wobble is the bass; LFO sweeps the filter. Re-hit each bar so its
        // long-decay envelope stays up across the drop.
        wobble_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        chug_note:   "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:        "X--- --x- X--- ----  X--- --x- X-x- ----";
        lead:        "a4 -  -  -   -  -  -  -   g4 -  -  -   -  -  -  -    f4 -  -  -   -  -  -  -   e4 -  -  -   -  -  -  -";
        kick:        "x--- ---- x--- ----  x--- ---- x--- ----";
        snare:       "---- ---- x--- ----  ---- ---- x--- ----";
        crash:       "x--- ---- ---- ----  x--- ---- ---- ----";
    }

    pattern "breakdown" {
        chug_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    .  .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:      "X--- x-xx X--- --xx  X-x- x-xx X--- ----";
        kick:      "x--- x-xx x--- --xx  x-x- x-xx x--- ----";
        snare:     "---- ---- ---- ----  ---- ---- x--- ----";
        crash:     "x--- ---- ---- ----  ---- ---- ---- x---";
    }

    pattern "outro" {
        chug_note:   "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    .  .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        wobble_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    .  .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:        "X--- ---- ---- ----  ---- ---- ---- ----";
        crash:       "x--- ---- ---- ----  ---- ---- ---- ----";
        kick:        "x--- ---- ---- ----  ---- ---- ---- ----";
    }

    // Instrumental — no verses. `intro` doubles as the riser/build that lifts
    // into each drop; the track lives in the drop ↔ breakdown contrast.
    sequence: [
        intro, intro,
        drop, drop, drop, drop,
        breakdown, breakdown, breakdown, breakdown,
        drop, drop,
        intro,
        drop, drop, drop, drop,
        breakdown, breakdown, breakdown,
        drop, drop, drop,
        outro,
    ];
};

struct Blackstar {
    lead: SawSuperVoice<7>,
    lead_env: Adsr,
    lead_shaper: Waveshaper,
    chug: SquareVoice,
    chug_gate: Gate,
    chug_shaper: Waveshaper,
    sub: Osc,
    // Wobble bass: saw → resonant SVF (LFO-swept) → drive.
    wobble: Osc,
    wobble_svf: Svf,
    wobble_env: Adsr,
    lfo_phase: f32,
    kick: KickVoice,
    snare: SnareVoice,
    crash: CymbalVoice,
    hat_state: u64,
    hat_bp: Svf,
    reverb: Reverb,
    song: CompiledSong,
}

impl Player for Blackstar {
    fn init(sample_rate: u32) -> Result<Self, String> {
        if sample_rate != SR {
            return Err(format!("Blackstar is authored at {SR} Hz, host offered {sample_rate} Hz"));
        }

        let lead_shaper = Waveshaper::new(SR, 4);
        lead_shaper.set_shape(ShapeKind::AsymTanh, 5.0, 0.12);
        lead_shaper.set_tone(180.0, 8500.0);

        let chug_shaper = Waveshaper::new(SR, 4);
        chug_shaper.set_shape(ShapeKind::AsymTanh, 5.5, 0.15);
        chug_shaper.set_tone(90.0, 7000.0);

        // Dry record — just a touch of space on the lead.
        let reverb = Reverb::new(SR);
        reverb.set_params(0.45, 0.6, 0.18);

        let mut hat_bp = Svf::new(SR);
        hat_bp.set_params(9000.0, 4.0);

        log::log("Blackstar loaded — A phrygian, drop-A, 176 bpm. Wobble incoming.");

        Ok(Self {
            lead: SawSuperVoice::new(SR, 18.0),
            lead_env: Adsr::new(SR, 0.003, 0.12, 0.30, 0.08),
            lead_shaper,
            chug: SquareVoice::new(SR, 0.5),
            chug_gate: Gate::new(SR, 1.5, 6.0),
            chug_shaper,
            sub: Osc::new(SR, Waveform::Sine),
            wobble: Osc::new(SR, Waveform::Saw),
            wobble_svf: Svf::new(SR),
            // Long decay so the re-hit-per-bar wobble sustains through a drop and
            // fades out when the section ends (no note-off needed).
            wobble_env: Adsr::new(SR, 0.01, 3.0, 0.0, 0.4),
            lfo_phase: 0.0,
            kick: KickVoice::metal(SR),
            snare: SnareVoice::metalcore(SR),
            crash: CymbalVoice::china(SR),
            hat_state: 0x8B1A_CC57_A12E_D004,
            hat_bp,
            reverb,
            song: SONG.compile(SR)?,
        })
    }

    fn render(&mut self, start_frame: u64, num_frames: u32) -> Vec<f32> {
        let n = num_frames as usize;

        for ev in self.song.events_in_range(start_frame, num_frames as u64) {
            match (ev.lane, ev.cell) {
                ("lead", Cell::Note(note)) => {
                    self.lead.note_on(note.hz());
                    self.lead_env.trigger();
                }
                ("chug_note", Cell::Note(note)) => {
                    self.chug.note_on(note.hz());
                    self.sub.set_freq(note.hz() * 0.5);
                }
                ("gate", Cell::Hit { accent }) => self.chug_gate.set(true, if accent { 1.4 } else { 1.0 }),
                ("gate", Cell::Ghost) => self.chug_gate.set(true, 0.3),
                ("gate", Cell::Off) => self.chug_gate.set(false, 0.0),
                ("wobble_note", Cell::Note(note)) => {
                    self.wobble.set_freq(note.hz());
                    self.wobble_env.trigger();
                }
                ("kick", Cell::Hit { .. }) => self.kick.trigger(),
                ("snare", Cell::Hit { .. }) => self.snare.trigger(),
                ("hat", Cell::Hit { .. }) => { /* no hat lane used here */ }
                ("crash", Cell::Hit { .. }) => self.crash.trigger(),
                _ => {}
            }
        }

        // Lead → hard asym crunch.
        let lead_raw = self.lead.render_block(num_frames);
        let lead_env = self.lead_env.render_block(num_frames);
        let lead_sig: Vec<f32> = (0..n).map(|i| lead_raw[i] * lead_env[i]).collect();
        let lead_crunch = self.lead_shaper.process(&lead_sig);

        // Chug → gate (captured per-sample so the sub rides it too) → ×4 crunch.
        let chug_raw = self.chug.render_block(num_frames);
        let chug_g: Vec<f32> = (0..n).map(|_| self.chug_gate.next()).collect();
        let chug_gated: Vec<f32> = (0..n).map(|i| chug_raw[i] * chug_g[i]).collect();
        let chug_crunch = self.chug_shaper.process(&chug_gated);

        let sub_raw = self.sub.render_block(num_frames);
        let wobble_env = self.wobble_env.render_block(num_frames);
        let kick = self.kick.render_block(num_frames);
        let snare = self.snare.render_block(num_frames);
        let crash = self.crash.render_block(num_frames);

        // Reverb send: just the lead, lightly.
        let send: Vec<f32> = (0..n).map(|i| 0.14 * lead_crunch[i]).collect();
        let (wet_l, wet_r) = self.reverb.process(&send, &send);

        let lfo_inc = WOBBLE_HZ / SR as f32;

        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let duck = 1.0 - 0.5 * kick[i].abs().min(1.0);

            // Dubstep wobble: LFO sweeps the resonant lowpass cutoff, then drive.
            let lfo = 0.5 * (1.0 + (core::f32::consts::TAU * self.lfo_phase).sin());
            self.lfo_phase = (self.lfo_phase + lfo_inc).fract();
            let cutoff = 130.0 + 3200.0 * lfo * lfo;
            self.wobble_svf.set_params(cutoff, 4.0);
            let (wlp, _, _) = self.wobble_svf.process_one(self.wobble.next());
            let wob = soft_clip(wlp * 3.5) * wobble_env[i];

            let noise = white(&mut self.hat_state);
            let (_, _, hbp) = self.hat_bp.process_one(noise);
            let hat = hbp * 0.04; // faint top-end air

            let dry = 0.26 * lead_crunch[i]
                + 0.50 * chug_crunch[i] * duck
                + 0.28 * sub_raw[i] * chug_g[i] * duck
                + 0.42 * wob
                + 0.95 * kick[i]
                + 0.60 * snare[i]
                + hat
                + 0.26 * crash[i];

            out.push(soft_clip(dry + wet_l[i]));
            out.push(soft_clip(dry + wet_r[i]));
        }
        out
    }

    fn reset(&mut self) {
        self.lead.reset();
        self.lead_env.reset();
        self.lead_shaper.reset();
        self.chug.reset();
        self.chug_gate.reset();
        self.chug_shaper.reset();
        self.sub.reset();
        self.wobble.reset();
        self.wobble_svf.reset();
        self.wobble_env.reset();
        self.lfo_phase = 0.0;
        self.kick.reset();
        self.snare.reset();
        self.crash.reset();
    }

    fn is_finished(&self) -> bool {
        false
    }

    fn metadata(&self) -> Metadata {
        Metadata {
            title: "Blackstar".to_string(),
            artist: "blackcap".to_string(),
            duration_frames: self.song.duration_frames(),
            sample_rate: SR,
            loop_point: Some(LoopPoint {
                start_frame: 0,
                end_frame: self.song.duration_frames(),
            }),
            cover_art: None,
            tags: vec![
                "electronicore".to_string(),
                "synth-metal".to_string(),
                "drop-a".to_string(),
            ],
        }
    }
}

export_player!(Blackstar);

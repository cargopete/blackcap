//! "Severance" — synth-metalcore in the Erra / early-Asking-Alexandria mould:
//! D minor, drop-D, 150 bpm. Ambient supersaw intro → palm-mute gallop verse →
//! soaring melodic chorus over the i–VI–III–VII loop (Dm–Bb–F–C) → half-time
//! crab-core breakdown → the euphoric trance-synth drop (fast supersaw arp over
//! heavy chug — the genre's signature) → reprise → final breakdown → outro.
//!
//! Lead + pad are detuned supersaws (lead lightly driven, pad reverb-drenched);
//! the chug is a gated drop-D square through the host's ×4 anti-aliased
//! waveshaper; bass has a sub-sine octave; the kit is synthesised. Master
//! compressor + limiter are host-side. Pure synthesis → it lands at synthcore,
//! by design.

use jukebox_cartridge_sdk::dsp::{Reverb, ShapeKind, Waveshaper};
use jukebox_cartridge_sdk::osc::{white, Osc, Waveform};
use jukebox_cartridge_sdk::prelude::*;

const SR: u32 = 48_000;

const SONG: TrackerSong = song! {
    tempo: 150;
    rows_per_beat: 4; // 16th grid; each pattern is 2 bars (32 cells)

    pattern "intro" {
        pad:   "d5 -  -  -   -  -  -  -   -  -  -  -   -  -  -  -    a4 -  -  -   -  -  -  -   -  -  -  -   -  -  -  -";
        kick:  "x  -  -  -   -  -  -  -   x  -  -  -   -  -  -  -    x  -  -  -   -  -  -  -   x  -  -  -   -  -  -  -";
        crash: "x  -  -  -   -  -  -  -   -  -  -  -   -  -  -  -    -  -  -  -   -  -  -  -   -  -  -  -   -  -  -  -";
    }

    pattern "build" {
        lead:  "d4 f4 a4 d5  f4 a4 d5 f5  d4 f4 a4 d5  f4 a4 d5 a5   d4 f4 a4 d5  a4 d5 f5 a5  bb4 d5 f5 bb5  a5 f5 d5 -";
        pad:   "d5 -  -  -   -  -  -  -   -  -  -  -   -  -  -  -    a4 -  -  -   -  -  -  -   -  -  -  -   -  -  -  -";
        hat:   "x  -  x  -   x  -  x  -   x  x  x  x   x  x  x  x    x  x  x  x   x  x  x  x   x  x  x  x   x  x  x  x";
        kick:  "x  -  -  -   x  -  -  -   x  -  -  -   x  -  -  -    x  -  -  x   x  -  -  x   x  -  x  -   x  x  x  x";
    }

    pattern "verse" {
        chug_note: "d2 -  -  -   -  -  -  -   -  -  -  -   eb2 - d2 -   d2 -  -  -   -  -  -  -   f2 -  eb2 -   d2 -  -  -";
        gate:      "x-xx -x-x x-xx -x-x  x-xx -x-x X-xx xxxx";
        kick:      "x-xx x-xx x-xx x-xx  x-xx x-xx x-xx x-xx";
        snare:     "---- x--- ---- x---  ---- x--- ---- x---";
        hat:       "x-x- x-x- x-x- x-x-  x-x- x-x- x-x- x-x-";
    }

    pattern "chorus" {
        lead:      "d5 -  c5 -   a4 -  d5 -   bb4 - a4 -   f4 -  a4 -    d5 -  f5 -   e5 -  d5 -   c5 -  d5 -   a4 -  -  -";
        pad:       "d4 -  -  -   -  -  -  -   bb3 -  -  -   -  -  -  -    f3 -  -  -   -  -  -  -   c4 -  -  -   -  -  -  -";
        bass:      "d2 -  -  -   -  -  -  -   bb1 -  -  -   -  -  -  -    f1 -  -  -   -  -  -  -   c2 -  -  -   -  -  -  -";
        chug_note: "d2 -  -  -   -  -  -  -   bb1 -  -  -   -  -  -  -    f1 -  -  -   -  -  -  -   c2 -  -  -   -  -  -  -";
        gate:      "X-x- x-x- X-x- x-x-  X-x- x-x- X-x- x-x-";
        kick:      "x--x x--x x--x x--x  x--x x--x x--x x--x";
        snare:     "---- x--- ---- x---  ---- x--- ---- x---";
        crash:     "x--- ---- x--- ----  x--- ---- x--- ----";
        hat:       "x-x- x-x- x-x- x-x-  x-x- x-x- x-x- x-x-";
    }

    pattern "breakdown" {
        chug_note: "d1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    .  .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:      "X--- --x- X--- --x-  X--- x-x- X--- ----";
        kick:      "x--- ---- --x- ----  x--- ---- x--- ----";
        snare:     "---- ---- x--- ----  ---- ---- x--- ----";
        crash:     "x--- ---- ---- ----  x--- ---- ---- ----";
    }

    pattern "trance" {
        lead:      "d5 a4 f5 a4  d5 a4 f5 d5  bb4 f4 d5 f4  bb4 d5 f5 bb5   f4 c5 a5 c5  f5 a4 c5 f5  c5 g4 e5 g4  c5 e5 g5 c6";
        pad:       "d4 -  -  -   -  -  -  -   bb3 -  -  -   -  -  -  -    f3 -  -  -   -  -  -  -   c4 -  -  -   -  -  -  -";
        chug_note: "d2 -  -  -   -  -  -  -   bb1 -  -  -   -  -  -  -    f1 -  -  -   -  -  -  -   c2 -  -  -   -  -  -  -";
        gate:      "X--- x--- X--- x---  X--- x--- X--- x---";
        kick:      "x--- x--- x--- x---  x--- x--- x--- x---";
        snare:     "---- x--- ---- x---  ---- x--- ---- x---";
        crash:     "x--- ---- ---- ----  ---- ---- ---- ----";
        hat:       "--x- --x- --x- --x-  --x- --x- --x- --x-";
    }

    pattern "outro" {
        pad:       "d4 -  -  -   -  -  -  -   -  -  -  -   -  -  -  -    -  -  -  -   -  -  -  -   -  -  -  -   -  -  -  -";
        chug_note: "d1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    .  .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:      "X--- ---- ---- ----  ---- ---- ---- ----";
        crash:     "x--- ---- ---- ----  ---- ---- ---- ----";
        kick:      "x--- ---- ---- ----  ---- ---- ---- ----";
    }

    sequence: [
        intro, intro, build,
        verse, verse, verse, verse,
        chorus, chorus, chorus, chorus,
        breakdown, breakdown, breakdown, breakdown,
        trance, trance, trance, trance,
        chorus, chorus,
        breakdown, breakdown, breakdown,
        outro,
    ];
};

struct Severance {
    lead: SawSuperVoice<7>,
    lead_env: Adsr,
    pad: SawSuperVoice<5>,
    pad_env: Adsr,
    bass: SquareVoice,
    bass_env: Adsr,
    sub: Osc,
    chug: SquareVoice,
    gate: Gate,
    kick: KickVoice,
    snare: SnareVoice,
    crash: CymbalVoice,
    hat_state: u64,
    hat_bp: Svf,
    hat_env: Adsr,
    lead_shaper: Waveshaper,
    chug_shaper: Waveshaper,
    reverb: Reverb,
    song: CompiledSong,
}

impl Player for Severance {
    fn init(sample_rate: u32) -> Result<Self, String> {
        if sample_rate != SR {
            return Err(format!("Severance is authored at {SR} Hz, host offered {sample_rate} Hz"));
        }

        let lead_shaper = Waveshaper::new(SR, 2);
        lead_shaper.set_shape(ShapeKind::SoftTanh, 1.8, 0.0);
        lead_shaper.set_tone(150.0, 9000.0);

        let chug_shaper = Waveshaper::new(SR, 4);
        chug_shaper.set_shape(ShapeKind::AsymTanh, 4.5, 0.15);
        chug_shaper.set_tone(95.0, 7500.0);

        let reverb = Reverb::new(SR);
        reverb.set_params(0.72, 0.5, 0.32); // lush — this is a synthcore record

        let mut hat_bp = Svf::new(SR);
        hat_bp.set_params(8500.0, 4.0);

        log::log("Severance loaded — D minor, drop-D, ~83s. Mind the breakdown.");

        Ok(Self {
            lead: SawSuperVoice::new(SR, 16.0),
            lead_env: Adsr::new(SR, 0.004, 0.14, 0.35, 0.10),
            pad: SawSuperVoice::new(SR, 22.0),
            pad_env: Adsr::new(SR, 0.08, 0.30, 0.70, 0.50),
            bass: SquareVoice::new(SR, 0.30),
            bass_env: Adsr::new(SR, 0.003, 0.10, 0.70, 0.06),
            sub: Osc::new(SR, Waveform::Sine),
            chug: SquareVoice::new(SR, 0.50),
            gate: Gate::new(SR, 2.0, 8.0),
            kick: KickVoice::metal(SR),
            snare: SnareVoice::metalcore(SR),
            crash: CymbalVoice::crash(SR),
            hat_state: 0x5E7E_8A0C_1234_9F00,
            hat_bp,
            hat_env: Adsr::new(SR, 0.0, 0.04, 0.0, 0.0),
            lead_shaper,
            chug_shaper,
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
                ("pad", Cell::Note(note)) => {
                    self.pad.note_on(note.hz());
                    self.pad_env.trigger();
                }
                ("bass", Cell::Note(note)) => {
                    self.bass.note_on(note.hz());
                    self.sub.set_freq(note.hz() * 0.5);
                    self.bass_env.trigger();
                }
                ("chug_note", Cell::Note(note)) => self.chug.note_on(note.hz()),
                ("gate", Cell::Hit { accent }) => self.gate.set(true, if accent { 1.4 } else { 1.0 }),
                ("gate", Cell::Ghost) => self.gate.set(true, 0.3),
                ("gate", Cell::Off) => self.gate.set(false, 0.0),
                ("kick", Cell::Hit { .. }) => self.kick.trigger(),
                ("snare", Cell::Hit { .. }) => self.snare.trigger(),
                ("hat", Cell::Hit { .. }) => self.hat_env.trigger(),
                ("crash", Cell::Hit { .. }) => self.crash.trigger(),
                _ => {}
            }
        }

        let lead_raw = self.lead.render_block(num_frames);
        let lead_env = self.lead_env.render_block(num_frames);
        let lead_sig: Vec<f32> = (0..n).map(|i| lead_raw[i] * lead_env[i]).collect();
        let lead_crunch = self.lead_shaper.process(&lead_sig);

        let pad_raw = self.pad.render_block(num_frames);
        let pad_env = self.pad_env.render_block(num_frames);
        let pad_sig: Vec<f32> = (0..n).map(|i| pad_raw[i] * pad_env[i]).collect();

        let bass_raw = self.bass.render_block(num_frames);
        let bass_env = self.bass_env.render_block(num_frames);
        let sub_raw = self.sub.render_block(num_frames);

        let chug_raw = self.chug.render_block(num_frames);
        let chug_gated: Vec<f32> = (0..n).map(|i| chug_raw[i] * self.gate.next()).collect();
        let chug_crunch = self.chug_shaper.process(&chug_gated);

        let kick = self.kick.render_block(num_frames);
        let snare = self.snare.render_block(num_frames);
        let crash = self.crash.render_block(num_frames);
        let hat_env = self.hat_env.render_block(num_frames);

        // Reverb send: lead + a very wet pad (the ambient bed).
        let send: Vec<f32> = (0..n).map(|i| 0.22 * lead_crunch[i] + 0.45 * pad_sig[i]).collect();
        let (wet_l, wet_r) = self.reverb.process(&send, &send);

        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let duck = 1.0 - 0.45 * kick[i].abs().min(1.0);

            let noise = white(&mut self.hat_state);
            let (_, _, hbp) = self.hat_bp.process_one(noise);
            let hat = hbp * hat_env[i] * 0.4;

            let dry = 0.28 * lead_crunch[i]
                + 0.16 * pad_sig[i]
                + 0.30 * bass_raw[i] * bass_env[i] * duck
                + 0.22 * sub_raw[i] * bass_env[i] * duck
                + 0.50 * chug_crunch[i] * duck
                + 0.90 * kick[i]
                + 0.65 * snare[i]
                + hat
                + 0.28 * crash[i];

            out.push(soft_clip(dry + wet_l[i]));
            out.push(soft_clip(dry + wet_r[i]));
        }
        out
    }

    fn reset(&mut self) {
        self.lead.reset();
        self.lead_env.reset();
        self.pad.reset();
        self.pad_env.reset();
        self.bass.reset();
        self.bass_env.reset();
        self.sub.reset();
        self.chug.reset();
        self.gate.reset();
        self.kick.reset();
        self.snare.reset();
        self.crash.reset();
        self.hat_env.reset();
        self.hat_bp.reset();
        self.lead_shaper.reset();
        self.chug_shaper.reset();
    }

    fn is_finished(&self) -> bool {
        false
    }

    fn metadata(&self) -> Metadata {
        Metadata {
            title: "Severance".to_string(),
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
                "synthcore".to_string(),
                "drop-d".to_string(),
            ],
        }
    }
}

export_player!(Severance);

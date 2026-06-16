//! "Eutectic Point" — a ~70-second synth-metal track in A phrygian, drop-A.
//!
//! Sections: intro (pad + sparse kick) → verse riff (supersaw lead, square bass
//! + sub sine, full kit) → breakdown (gated drop-A chug, half-time kit) → lead
//! chorus → outro ring. The lead and chug run through host `Waveshaper`s (soft
//! drive on the lead, ×4 asym crunch on the chug) with a shared `Reverb` send;
//! the bass/sub duck off the kick. Master compressor + limiter are host-side.
//!
//! Honest genre note: pure synthesis, no samples — this lands at Master Boot
//! Record / synthcore, not djent. That's the medium, not a bug.

use jukebox_cartridge_sdk::dsp::{Reverb, ShapeKind, Waveshaper};
use jukebox_cartridge_sdk::osc::{white, Osc, Waveform};
use jukebox_cartridge_sdk::prelude::*;

const SR: u32 = 48_000;

const TRACK: TrackerSong = song! {
    tempo: 150;
    rows_per_beat: 4; // 16th grid; each pattern is 2 bars (32 cells)

    pattern "intro" {
        lead:  "a4 -  -  -   -  -  -  -   -  -  -  -   -  -  -  -    c5 -  -  -   -  -  -  -   -  -  -  -   -  -  -  -";
        kick:  "x  -  -  -   -  -  -  -   x  -  -  -   -  -  -  -    x  -  -  -   -  -  -  -   x  -  -  -   -  -  -  -";
        crash: "x  -  -  -   -  -  -  -   -  -  -  -   -  -  -  -    -  -  -  -   -  -  -  -   -  -  -  -   -  -  -  -";
    }

    pattern "verse" {
        lead:  "a4 -  g4 a4  bb4 - a4 -   e4 -  f4 e4  d4 -  c4 -    a4 -  g4 a4  bb4 - c5 -   a4 -  e4 -   a4 g4 e4 -";
        bass:  "a2 -  -  -   a2 -  -  -   a2 -  -  -   a2 -  -  -    a2 -  -  -   a2 -  -  -   g2 -  -  -   e2 -  -  -";
        kick:  "x  -  -  x   x  -  -  x   x  -  -  x   x  -  -  x    x  -  -  x   x  -  -  x   x  -  -  x   x  -  x  x";
        snare: "-  -  -  -   x  -  -  -   -  -  -  -   x  -  -  -    -  -  -  -   x  -  -  -   -  -  -  -   x  -  -  -";
        hat:   "x  -  x  -   x  -  x  -   x  -  x  -   x  -  x  -    x  -  x  -   x  -  x  -   x  -  x  -   x  -  x  x";
    }

    pattern "break" {
        chug_note: "a1 . . .   . . . .   . . . .   . . . .    . . . .   . . . .   . . . .   . . . .";
        gate:      "X-x- x-x- X-xx -x-x  X-x- x-x- X-x- X---";
        kick:      "x--- x--- x-x- ---x  x--- x--- x-x- ----";
        snare:     "---- x--- ---- x---  ---- x--- ---- x---";
        crash:     "x--- ---- ---- ----  x--- ---- ---- ----";
    }

    pattern "lead" {
        lead:  "a5 -  g5 e5  f5 -  e5 -   d5 -  c5 d5  e5 -  -  -    a5 -  c6 b5  a5 -  g5 -   e5 -  f5 e5  a5 -  -  -";
        bass:  "a2 -  -  -   a2 -  -  -   f2 -  -  -   g2 -  -  -    a2 -  -  -   a2 -  -  -   e2 -  -  -   g2 -  -  -";
        kick:  "x  -  -  x   x  -  -  x   x  -  -  x   x  -  -  x    x  -  -  x   x  -  -  x   x  -  -  x   x  -  x  x";
        snare: "-  -  -  -   x  -  -  -   -  -  -  -   x  -  -  -    -  -  -  -   x  -  -  -   -  -  -  -   x  -  x  -";
        hat:   "x  x  x  x   x  x  x  x   x  x  x  x   x  x  x  x    x  x  x  x   x  x  x  x   x  x  x  x   x  x  x  x";
        crash: "x  -  -  -   -  -  -  -   -  -  -  -   -  -  -  -    x  -  -  -   -  -  -  -   -  -  -  -   -  -  -  -";
    }

    pattern "outro" {
        chug_note: "a1 . . .   . . . .   . . . .   . . . .    . . . .   . . . .   . . . .   . . . .";
        gate:      "X--- ---- ---- ----  ---- ---- ---- ----";
        crash:     "x--- ---- ---- ----  ---- ---- ---- ----";
        kick:      "x--- ---- ---- ----  ---- ---- ---- ----";
    }

    sequence: [
        intro, intro,
        verse, verse, verse, verse,
        lead, lead,
        break, break,
        verse, verse,
        lead, lead,
        break, break, break, break,
        lead, lead,
        verse,
        outro,
    ];
};

struct Track {
    lead: SawSuperVoice<7>,
    lead_env: Adsr,
    bass: SquareVoice,
    bass_env: Adsr,
    sub: Osc,
    kick: KickVoice,
    snare: SnareVoice,
    crash: CymbalVoice,
    hat_state: u64,
    hat_bp: Svf,
    hat_env: Adsr,
    chug: SquareVoice,
    gate: Gate,
    lead_shaper: Waveshaper,
    chug_shaper: Waveshaper,
    reverb: Reverb,
    song: CompiledSong,
}

impl Player for Track {
    fn init(sample_rate: u32) -> Result<Self, String> {
        if sample_rate != SR {
            return Err(format!("Eutectic Point is authored at {SR} Hz, host offered {sample_rate} Hz"));
        }

        let lead_shaper = Waveshaper::new(SR, 2);
        lead_shaper.set_shape(ShapeKind::SoftTanh, 2.4, 0.0);
        lead_shaper.set_tone(120.0, 9000.0);

        let chug_shaper = Waveshaper::new(SR, 4);
        chug_shaper.set_shape(ShapeKind::AsymTanh, 4.5, 0.15);
        chug_shaper.set_tone(100.0, 8000.0);

        let reverb = Reverb::new(SR);
        reverb.set_params(0.62, 0.5, 0.28);

        let mut hat_bp = Svf::new(SR);
        hat_bp.set_params(8500.0, 4.0);

        log::log("Eutectic Point loaded — ~70s, A phrygian, drop-A");

        Ok(Self {
            lead: SawSuperVoice::new(SR, 14.0),
            lead_env: Adsr::new(SR, 0.004, 0.16, 0.55, 0.10),
            bass: SquareVoice::new(SR, 0.32),
            bass_env: Adsr::new(SR, 0.003, 0.10, 0.7, 0.05),
            sub: Osc::new(SR, Waveform::Sine),
            kick: KickVoice::metal(SR),
            snare: SnareVoice::metalcore(SR),
            crash: CymbalVoice::crash(SR),
            hat_state: 0xBADC_0FFE_E0DD_F00D,
            hat_bp,
            hat_env: Adsr::new(SR, 0.0, 0.045, 0.0, 0.0),
            chug: SquareVoice::new(SR, 0.5),
            gate: Gate::new(SR, 2.0, 6.0),
            lead_shaper,
            chug_shaper,
            reverb,
            song: TRACK.compile(SR)?,
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
                ("bass", Cell::Note(note)) => {
                    self.bass.note_on(note.hz());
                    self.sub.set_freq(note.hz() * 0.5);
                    self.bass_env.trigger();
                }
                ("kick", Cell::Hit { .. }) => self.kick.trigger(),
                ("snare", Cell::Hit { .. }) => self.snare.trigger(),
                ("hat", Cell::Hit { .. }) => self.hat_env.trigger(),
                ("crash", Cell::Hit { .. }) => self.crash.trigger(),
                ("chug_note", Cell::Note(note)) => self.chug.note_on(note.hz()),
                ("gate", Cell::Hit { accent }) => self.gate.set(true, if accent { 1.4 } else { 1.0 }),
                ("gate", Cell::Ghost) => self.gate.set(true, 0.3),
                ("gate", Cell::Off) => self.gate.set(false, 0.0),
                _ => {}
            }
        }

        // Lead: supersaw × envelope → host soft-drive shaper.
        let lead_raw = self.lead.render_block(num_frames);
        let lead_env = self.lead_env.render_block(num_frames);
        let lead_sig: Vec<f32> = (0..n).map(|i| lead_raw[i] * lead_env[i]).collect();
        let lead_crunch = self.lead_shaper.process(&lead_sig);

        // Bass + sub (sine an octave down), sharing the bass envelope.
        let bass_raw = self.bass.render_block(num_frames);
        let bass_env = self.bass_env.render_block(num_frames);
        let sub_raw = self.sub.render_block(num_frames);

        // Kit.
        let kick = self.kick.render_block(num_frames);
        let snare = self.snare.render_block(num_frames);
        let crash = self.crash.render_block(num_frames);
        let hat_env = self.hat_env.render_block(num_frames);

        // Chug: held drone → gate → host ×4 asym crunch.
        let chug_raw = self.chug.render_block(num_frames);
        let chug_gated: Vec<f32> = (0..n).map(|i| chug_raw[i] * self.gate.next()).collect();
        let chug_crunch = self.chug_shaper.process(&chug_gated);

        // Reverb send: lead + a touch of chug.
        let send: Vec<f32> = (0..n).map(|i| 0.22 * lead_crunch[i] + 0.10 * chug_crunch[i]).collect();
        let (wet_l, wet_r) = self.reverb.process(&send, &send);

        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            // Bass/sub/chug duck under the kick (the "pump").
            let duck = 1.0 - 0.5 * kick[i].abs().min(1.0);

            let noise = white(&mut self.hat_state);
            let (_, _, hbp) = self.hat_bp.process_one(noise);
            let hat = hbp * hat_env[i] * 0.4;

            let dry = 0.30 * lead_crunch[i]
                + 0.32 * bass_raw[i] * bass_env[i] * duck
                + 0.24 * sub_raw[i] * bass_env[i] * duck
                + 0.50 * chug_crunch[i] * duck
                + 0.90 * kick[i]
                + 0.70 * snare[i]
                + hat
                + 0.30 * crash[i];

            out.push(soft_clip(dry + wet_l[i]));
            out.push(soft_clip(dry + wet_r[i]));
        }
        out
    }

    fn reset(&mut self) {
        self.lead.reset();
        self.lead_env.reset();
        self.bass.reset();
        self.bass_env.reset();
        self.sub.reset();
        self.kick.reset();
        self.snare.reset();
        self.crash.reset();
        self.hat_env.reset();
        self.hat_bp.reset();
        self.chug.reset();
        self.gate.reset();
        self.lead_shaper.reset();
        self.chug_shaper.reset();
    }

    fn is_finished(&self) -> bool {
        false // the host loops it
    }

    fn metadata(&self) -> Metadata {
        Metadata {
            title: "Eutectic Point".to_string(),
            artist: "blackcap".to_string(),
            duration_frames: self.song.duration_frames(),
            sample_rate: SR,
            loop_point: Some(LoopPoint {
                start_frame: 0,
                end_frame: self.song.duration_frames(),
            }),
            cover_art: None,
            tags: vec![
                "synth-metal".to_string(),
                "synthcore".to_string(),
                "drop-a".to_string(),
            ],
        }
    }
}

export_player!(Track);

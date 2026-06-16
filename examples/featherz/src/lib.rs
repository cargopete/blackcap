//! "featherz" — the aggressive one. Fuses the two favourites: the **sampled
//! Karplus-Strong guitar** of *Plucked* (host sampler, pitch-shifted, crunched)
//! as the riff/lead, and the **wobble drops + gated chug + breakdowns** of
//! *Blackstar* — then pushes it harder: A phrygian / drop-A at 184 bpm, a
//! hard-clipped buzzsaw chug, and a proper blast-beat section. Instrumental, no
//! verses; it lives in the drop ↔ blast ↔ breakdown churn.

use jukebox_cartridge_sdk::dsp::{Reverb, ShapeKind, Waveshaper};
use jukebox_cartridge_sdk::osc::{white, Osc, Waveform};
use jukebox_cartridge_sdk::prelude::*;
use jukebox_cartridge_sdk::sampler::{Sample, SampleVoice};

const SR: u32 = 48_000;
const TEMPO: f32 = 184.0;
const WOBBLE_HZ: f32 = TEMPO / 60.0 * 2.0; // 1/8-note wobble
const PLUCK_ROOT: f32 = 440.0; // a4 — the Karplus root; riff notes pitch-shift from here
const VOICES: usize = 6;

const SONG: TrackerSong = song! {
    tempo: 184;
    rows_per_beat: 4; // 16th grid; 2 bars (32 cells) per pattern

    pattern "intro" {
        lead:      "a3 c4 e4 a4  c4 e4 a4 c5  e4 a4 c5 e5  a4 c5 e5 a5   a3 e4 a4 c5  e4 a4 c5 e5  a4 c5 e5 a5  c5 e5 a5 c6";
        chug_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    .  .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:      "X--- ---- ---- ----  x-x- ---- x-x- xxxx";
        kick:      "x--- ---- x--- ----  x-x- ---- x-x- xxxx";
        crash:     "x--- ---- ---- ----  ---- ---- ---- ----";
    }

    pattern "drop" {
        wobble_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        chug_note:   "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:        "X--- --x- X--- ----  X--- --x- X-x- ----";
        lead:        "a4 -  -  e4  -  -  a4 -   bb4 - -  a4  -  g4 -  -    a4 -  -  e4  -  -  a4 -   c5 -  bb4 -   a4 -  -  -";
        kick:        "x--- ---- x--- ----  x--- ---- x--- ----";
        snare:       "---- ---- x--- ----  ---- ---- x--- ----";
        crash:       "x--- ---- ---- ----  x--- ---- ---- ----";
    }

    pattern "blast" {
        lead:      "a4 a4 a4 a4  bb4 bb4 a4 a4  g4 g4 a4 a4  e4 e4 a4 a4   a4 a4 a4 a4  bb4 bb4 c5 c5  a4 a4 g4 g4  e4 e4 -  -";
        chug_note: "a1 .  .  .   bb1 .  .  .   a1 .  .  .   g1 .  .  .    a1 .  .  .   bb1 .  .  .   a1 .  f1 .   e1 .  .  .";
        gate:      "x-xx x-xx x-xx xxxx  x-xx x-xx x-xx xxxx";
        kick:      "xxxx xxxx xxxx xxxx  xxxx xxxx xxxx xxxx";
        snare:     "--x- --x- --x- --x-  --x- --x- --x- --x-";
        crash:     "x--- ---- ---- ----  x--- ---- ---- ----";
    }

    // The breakdown drops to 120 bpm — a real half-time tempo change, the way a
    // proper metalcore/deathcore breakdown lurches slower than the rest.
    pattern "breakdown" @120 {
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

    sequence: [
        intro, intro,
        drop, drop, blast, blast,
        breakdown, breakdown,
        drop, blast, blast,
        breakdown, breakdown, breakdown,
        drop, drop, blast, blast,
        breakdown, breakdown, breakdown,
        drop,
        outro,
    ];
};

fn karplus_strong(freq: f32, seconds: f32, decay: f32, seed: u64) -> Vec<f32> {
    let n = (SR as f32 * seconds) as usize;
    let p = (SR as f32 / freq).round().max(2.0) as usize;
    let mut state = seed;
    let mut ring: Vec<f32> = (0..p).map(|_| white(&mut state)).collect();
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    for _ in 0..n {
        let cur = ring[i];
        let next = ring[(i + 1) % p];
        out.push(cur);
        ring[i] = 0.5 * (cur + next) * decay;
        i = (i + 1) % p;
    }
    out
}

struct Featherz {
    pluck: Sample,
    voices: Vec<SampleVoice>,
    next: usize,
    pluck_shaper: Waveshaper,
    chug: SquareVoice,
    chug_gate: Gate,
    chug_shaper: Waveshaper,
    sub: Osc,
    wobble: Osc,
    wobble_svf: Svf,
    wobble_env: Adsr,
    lfo_phase: f32,
    kick: KickVoice,
    snare: SnareVoice,
    crash: CymbalVoice,
    reverb: Reverb,
    song: CompiledSong,
}

impl Player for Featherz {
    fn init(sample_rate: u32) -> Result<Self, String> {
        if sample_rate != SR {
            return Err(format!("featherz is authored at {SR} Hz, host offered {sample_rate} Hz"));
        }

        // The lead/riff: a Karplus-Strong pluck, or guitar.wav if you've got one.
        let pluck = Sample::from_library("guitar")
            .unwrap_or_else(|| Sample::from_pcm(&karplus_strong(PLUCK_ROOT, 0.5, 0.993, 0xFEA37E_42)));

        // Aggressive lead crunch.
        let pluck_shaper = Waveshaper::new(SR, 4);
        pluck_shaper.set_shape(ShapeKind::AsymTanh, 4.5, 0.1);
        pluck_shaper.set_tone(150.0, 9000.0);

        // Brutal buzzsaw chug — hard-clip, heavily oversampled so it doesn't fizz.
        let chug_shaper = Waveshaper::new(SR, 4);
        chug_shaper.set_shape(ShapeKind::HardClip, 5.0, 0.0);
        chug_shaper.set_tone(85.0, 7000.0);

        let reverb = Reverb::new(SR);
        reverb.set_params(0.42, 0.6, 0.14); // dry — aggression over wash

        log::log("featherz loaded — A phrygian, drop-A, 184 bpm. No survivors.");

        Ok(Self {
            pluck,
            voices: (0..VOICES).map(|_| SampleVoice::new()).collect(),
            next: 0,
            pluck_shaper,
            chug: SquareVoice::new(SR, 0.5),
            chug_gate: Gate::new(SR, 1.5, 6.0),
            chug_shaper,
            sub: Osc::new(SR, Waveform::Sine),
            wobble: Osc::new(SR, Waveform::Saw),
            wobble_svf: Svf::new(SR),
            wobble_env: Adsr::new(SR, 0.01, 3.0, 0.0, 0.4),
            lfo_phase: 0.0,
            kick: KickVoice::metal(SR),
            snare: SnareVoice::metalcore(SR),
            crash: CymbalVoice::china(SR),
            reverb,
            song: SONG.compile(SR)?,
        })
    }

    fn render(&mut self, start_frame: u64, num_frames: u32) -> Vec<f32> {
        let n = num_frames as usize;

        for ev in self.song.events_in_range(start_frame, num_frames as u64) {
            match (ev.lane, ev.cell) {
                ("lead", Cell::Note(note)) => {
                    self.voices[self.next].trigger(&self.pluck, note.hz() / PLUCK_ROOT, 0.85);
                    self.next = (self.next + 1) % VOICES;
                }
                ("chug_note", Cell::Note(note)) => {
                    self.chug.note_on(note.hz());
                    self.sub.set_freq(note.hz() * 0.5);
                }
                ("gate", Cell::Hit { accent }) => self.chug_gate.set(true, if accent { 1.5 } else { 1.0 }),
                ("gate", Cell::Ghost) => self.chug_gate.set(true, 0.3),
                ("gate", Cell::Off) => self.chug_gate.set(false, 0.0),
                ("wobble_note", Cell::Note(note)) => {
                    self.wobble.set_freq(note.hz());
                    self.wobble_env.trigger();
                }
                ("kick", Cell::Hit { .. }) => self.kick.trigger(),
                ("snare", Cell::Hit { .. }) => self.snare.trigger(),
                ("crash", Cell::Hit { .. }) => self.crash.trigger(),
                _ => {}
            }
        }

        // Lead: sum the sampled-pluck voices → aggressive crunch.
        let mut pluck_bus = vec![0.0f32; n];
        for voice in &self.voices {
            let block = voice.render(num_frames);
            for (p, s) in pluck_bus.iter_mut().zip(block.iter()) {
                *p += *s;
            }
        }
        let pluck_crunch = self.pluck_shaper.process(&pluck_bus);

        // Chug: gate (captured for the sub too) → hard-clip buzzsaw.
        let chug_raw = self.chug.render_block(num_frames);
        let chug_g: Vec<f32> = (0..n).map(|_| self.chug_gate.next()).collect();
        let chug_gated: Vec<f32> = (0..n).map(|i| chug_raw[i] * chug_g[i]).collect();
        let chug_crunch = self.chug_shaper.process(&chug_gated);

        let sub_raw = self.sub.render_block(num_frames);
        let wobble_env = self.wobble_env.render_block(num_frames);
        let kick = self.kick.render_block(num_frames);
        let snare = self.snare.render_block(num_frames);
        let crash = self.crash.render_block(num_frames);

        let send: Vec<f32> = (0..n).map(|i| 0.16 * pluck_crunch[i]).collect();
        let (wet_l, wet_r) = self.reverb.process(&send, &send);

        let lfo_inc = WOBBLE_HZ / SR as f32;
        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let duck = 1.0 - 0.5 * kick[i].abs().min(1.0);

            // Dubstep wobble (drops only — silent elsewhere via its envelope).
            let lfo = 0.5 * (1.0 + (core::f32::consts::TAU * self.lfo_phase).sin());
            self.lfo_phase = (self.lfo_phase + lfo_inc).fract();
            self.wobble_svf.set_params(130.0 + 3200.0 * lfo * lfo, 4.0);
            let (wlp, _, _) = self.wobble_svf.process_one(self.wobble.next());
            let wob = soft_clip(wlp * 3.5) * wobble_env[i];

            let dry = 0.30 * pluck_crunch[i]
                + 0.50 * chug_crunch[i] * duck
                + 0.42 * wob
                + 0.28 * sub_raw[i] * chug_g[i] * duck
                + 0.95 * kick[i]
                + 0.62 * snare[i]
                + 0.26 * crash[i];

            out.push(soft_clip(dry + wet_l[i]));
            out.push(soft_clip(dry + wet_r[i]));
        }
        out
    }

    fn reset(&mut self) {
        for v in &self.voices {
            v.stop();
        }
        self.pluck_shaper.reset();
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
            title: "featherz".to_string(),
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
                "aggressive".to_string(),
                "drop-a".to_string(),
            ],
        }
    }
}

export_player!(Featherz);

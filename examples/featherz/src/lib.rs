//! "featherz" — aggressive and haunting. Keeps the sampled Karplus guitar of
//! *Plucked* (that creepy, organ-ish, haunted character) but pushes it dark and
//! heavy rather than bright/Eastern: A phrygian with the tritone (eb) for dread,
//! drop-A, a soft reverb-drenched detuned-saw **choir pad** underneath, a low
//! haunted pluck melody, and a hard-clipped buzzsaw chug. No dance-wobble.
//!
//! Structure: a haunting swell intro → heavy aggressive sections over the choir
//! → slow crushing breakdowns that get *slower* toward the end. Instrumental.

use jukebox_cartridge_sdk::dsp::{Reverb, ShapeKind, Waveshaper};
use jukebox_cartridge_sdk::osc::Osc;
use jukebox_cartridge_sdk::osc::Waveform;
use jukebox_cartridge_sdk::prelude::*;
use jukebox_cartridge_sdk::sampler::{Sample, SampleVoice};

const SR: u32 = 48_000;
const PLUCK_ROOT: f32 = 220.0; // a3 — darker than before; riff notes shift from here
const VOICES: usize = 5;

const SONG: TrackerSong = song! {
    tempo: 144;
    rows_per_beat: 4; // 16th grid; 2 bars (32 cells) per pattern

    // Haunting swell: choir pad + sparse low pluck, dread building. No bright arp.
    pattern "intro" {
        pad:  "a3 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    f3 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        lead: "a4 -  -  -   -  -  -  -   bb4 - -  -   -  -  -  -    -  -  -  -   -  -  -  -   eb4 - -  -   -  -  -  -";
        crash:"x--- ---- ---- ----  ---- ---- ---- ----";
        kick: "x--- ---- ---- ----  x--- ---- ---- ----";
    }

    // Heavy + haunting: buzzsaw chug + choir pad + a low phrygian pluck melody.
    pattern "heavy" {
        pad:       "a3 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    f3 .  .  .   .  .  .  .   g3 .  .  .   .  .  .  .";
        lead:      "a4 -  -  -   eb5 - -  -   c5 -  -  -   bb4 - a4 -    a4 -  -  -   f4 -  -  -   g4 -  e4 -   a4 -  -  -";
        chug_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    a1 .  .  .   .  .  .  .   g1 .  .  .   f1 .  .  .";
        gate:      "X-x- -x-x X-x- -x--  X-x- -x-x X-x- xx--";
        kick:      "x--x --x- x--x --x-  x--x --x- x--x --xx";
        snare:     "---- x--- ---- x---  ---- x--- ---- x---";
        crash:     "x--- ---- ---- ----  x--- ---- ---- ----";
    }

    // Slow crushing breakdown — one tight stomping bar (the weak slow first half
    // is gone). Accented hits with no dead air: slow and HEAVY, not slow + quiet.
    // 120 bpm.
    pattern "breakdown" @120 {
        pad:       "a3 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        lead:      "a4 .  .  .   .  .  .  .   eb5 - -  -   -  -  -  -"; // ghost over the stomp
        chug_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:      "X--- --X- X--- X-x-";
        kick:      "x--- --x- x--- x-x-";
        snare:     "---- ---- X--- ----";
        crash:     "x--- ---- ---- ----";
    }

    // Pure haunting breather: choir + ghost pluck, no drums or chug. The intro's
    // vibe, dropped in mid-song.
    pattern "interlude" {
        pad:  "a3 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    f3 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        lead: "a4 -  -  -   bb4 - -  -   c5 -  -  -   eb5 - -  -    d5 -  -  -   c5 -  -  -   bb4 - -  -   a4 -  -  -";
        crash:"x--- ---- ---- ----  ---- ---- ---- ----";
    }

    // The great slow ending: even slower, enormous gaps, final dread. 84 bpm.
    pattern "ending" @84 {
        pad:       "a3 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    .  .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        lead:      "a4 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    eb4 - -  -   -  -  -  -   c4 -  -  -   bb3 - -  -";
        chug_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    .  .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:      "X--- ---- ---- ----  --x- ---- X--- ----";
        kick:      "x--- ---- ---- ----  --x- ---- x--- ----";
        snare:     "---- ---- ---- ----  ---- ---- x--- ----";
        crash:     "x--- ---- ---- ----  ---- ---- ---- ----";
    }

    pattern "outro" @84 {
        pad:       "a3 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    .  .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        chug_note: "a1 .  .  .   .  .  .  .   .  .  .  .   .  .  .  .    .  .  .  .   .  .  .  .   .  .  .  .   .  .  .  .";
        gate:      "X--- ---- ---- ----  ---- ---- ---- ----";
        crash:     "x--- ---- ---- ----  ---- ---- ---- ----";
    }

    sequence: [
        intro, intro,
        heavy, heavy, heavy,
        breakdown, breakdown, breakdown, breakdown,
        heavy, heavy,
        breakdown, breakdown, breakdown, breakdown,
        interlude,
        ending, ending,
        outro,
    ];
};

fn karplus_strong(freq: f32, seconds: f32, decay: f32, seed: u64) -> Vec<f32> {
    let n = (SR as f32 * seconds) as usize;
    let p = (SR as f32 / freq).round().max(2.0) as usize;
    let mut state = seed;
    let mut ring: Vec<f32> = (0..p).map(|_| jukebox_cartridge_sdk::osc::white(&mut state)).collect();
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
    pad: SawSuperVoice<5>,
    pad_env: Adsr,
    pad_lp: Svf,
    chug: SquareVoice,
    chug_gate: Gate,
    chug_shaper: Waveshaper,
    sub: Osc,
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

        let pluck = Sample::from_library("guitar")
            .unwrap_or_else(|| Sample::from_pcm(&karplus_strong(PLUCK_ROOT, 0.7, 0.995, 0xFEA37E_42)));

        // Haunted lead — moderate drive, darker tone (less bright/Eastern).
        let pluck_shaper = Waveshaper::new(SR, 4);
        pluck_shaper.set_shape(ShapeKind::AsymTanh, 3.0, 0.08);
        pluck_shaper.set_tone(120.0, 6500.0);

        // Aggressive buzzsaw chug.
        let chug_shaper = Waveshaper::new(SR, 4);
        chug_shaper.set_shape(ShapeKind::HardClip, 4.5, 0.0);
        chug_shaper.set_tone(85.0, 6500.0);

        // Soft choir pad: detuned saws rolled off and drowned in reverb.
        let mut pad_lp = Svf::new(SR);
        pad_lp.set_params(1400.0, 0.7);

        let reverb = Reverb::new(SR);
        reverb.set_params(0.78, 0.4, 0.32); // lush + haunting, but it's a send

        log::log("featherz loaded — aggressive + haunting, A phrygian, drop-A.");

        Ok(Self {
            pluck,
            voices: (0..VOICES).map(|_| SampleVoice::new()).collect(),
            next: 0,
            pluck_shaper,
            pad: SawSuperVoice::new(SR, 24.0),
            pad_env: Adsr::new(SR, 0.18, 0.40, 0.70, 0.9),
            pad_lp,
            chug: SquareVoice::new(SR, 0.5),
            chug_gate: Gate::new(SR, 1.5, 7.0),
            chug_shaper,
            sub: Osc::new(SR, Waveform::Sine),
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
                ("pad", Cell::Note(note)) => {
                    self.pad.note_on(note.hz());
                    self.pad_env.trigger();
                }
                ("lead", Cell::Note(note)) => {
                    self.voices[self.next].trigger(&self.pluck, note.hz() / PLUCK_ROOT, 0.8);
                    self.next = (self.next + 1) % VOICES;
                }
                ("chug_note", Cell::Note(note)) => {
                    self.chug.note_on(note.hz());
                    self.sub.set_freq(note.hz() * 0.5);
                }
                ("gate", Cell::Hit { accent }) => self.chug_gate.set(true, if accent { 1.5 } else { 1.0 }),
                ("gate", Cell::Ghost) => self.chug_gate.set(true, 0.3),
                ("gate", Cell::Off) => self.chug_gate.set(false, 0.0),
                ("kick", Cell::Hit { .. }) => self.kick.trigger(),
                ("snare", Cell::Hit { .. }) => self.snare.trigger(),
                ("crash", Cell::Hit { .. }) => self.crash.trigger(),
                _ => {}
            }
        }

        // Choir pad: detuned saws → lowpass → envelope.
        let pad_raw = self.pad.render_block(num_frames);
        let pad_env = self.pad_env.render_block(num_frames);
        let pad_sig: Vec<f32> = (0..n)
            .map(|i| {
                let (lp, _, _) = self.pad_lp.process_one(pad_raw[i]);
                lp * pad_env[i]
            })
            .collect();

        // Haunted lead: sampled pluck voices → moderate crunch.
        let mut pluck_bus = vec![0.0f32; n];
        for voice in &self.voices {
            let block = voice.render(num_frames);
            for (p, s) in pluck_bus.iter_mut().zip(block.iter()) {
                *p += *s;
            }
        }
        let pluck_crunch = self.pluck_shaper.process(&pluck_bus);

        // Chug: gate (kept for the sub) → hard-clip buzzsaw.
        let chug_raw = self.chug.render_block(num_frames);
        let chug_g: Vec<f32> = (0..n).map(|_| self.chug_gate.next()).collect();
        let chug_gated: Vec<f32> = (0..n).map(|i| chug_raw[i] * chug_g[i]).collect();
        let chug_crunch = self.chug_shaper.process(&chug_gated);

        let sub_raw = self.sub.render_block(num_frames);
        let kick = self.kick.render_block(num_frames);
        let snare = self.snare.render_block(num_frames);
        let crash = self.crash.render_block(num_frames);

        // Reverb send: the pad (mostly) and the pluck — the haunting space.
        let send: Vec<f32> = (0..n).map(|i| 0.55 * pad_sig[i] + 0.26 * pluck_crunch[i]).collect();
        let (wet_l, wet_r) = self.reverb.process(&send, &send);

        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let duck = 1.0 - 0.5 * kick[i].abs().min(1.0);
            let dry = 0.19 * pad_sig[i]
                + 0.26 * pluck_crunch[i]
                + 0.50 * chug_crunch[i] * duck
                + 0.28 * sub_raw[i] * chug_g[i] * duck
                + 0.92 * kick[i]
                + 0.60 * snare[i]
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
        self.pad.reset();
        self.pad_env.reset();
        self.pad_lp.reset();
        self.chug.reset();
        self.chug_gate.reset();
        self.chug_shaper.reset();
        self.sub.reset();
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
                "aggressive".to_string(),
                "haunting".to_string(),
                "drop-a".to_string(),
            ],
        }
    }
}

export_player!(Featherz);

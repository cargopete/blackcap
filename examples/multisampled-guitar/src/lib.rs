//! "Fretboard" — the multisampling demo. Builds a 3-zone instrument from
//! Karplus-Strong plucks rooted at A1/A2/A3 (55/110/220 Hz); each note picks the
//! nearest-rooted zone and shifts only the small remainder, so a wide riff stays
//! consistent instead of getting rubbery toward the edges (the single-sample
//! failure mode `sampled-guitar` shows).
//!
//! Drop `guitar_a1.wav` / `guitar_a2.wav` / `guitar_a3.wav` into
//! ~/.jukebox/samples to use real DI guitar zones instead.

use jukebox_cartridge_sdk::dsp::{ShapeKind, Waveshaper};
use jukebox_cartridge_sdk::osc::white;
use jukebox_cartridge_sdk::prelude::*;
use jukebox_cartridge_sdk::sampler::{Multisample, Sample, SampleVoice};

const SR: u32 = 48_000;
const VOICES: usize = 6;
const ZONES: [(f32, &str); 3] = [(55.0, "guitar_a1"), (110.0, "guitar_a2"), (220.0, "guitar_a3")];

const RIFF: TrackerSong = song! {
    tempo: 140;
    rows_per_beat: 4;

    pattern "a" {
        // Wide range so different notes land in different zones.
        gtr:  "a2 -  e3 a3  c4 -  e4 -   a4 -  g4 e4  c4 -  a3 -    a2 -  e3 a3  c4 -  e4 g4  a4 -  e5 -   a4 g4 e4 -";
        kick: "x  -  -  x   x  -  -  x   x  -  -  x   x  -  -  x    x  -  -  x   x  -  -  x   x  -  -  x   x  -  x  x";
    }

    sequence: [a, a];
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

struct Fretboard {
    instrument: Multisample,
    voices: Vec<SampleVoice>,
    next: usize,
    shaper: Waveshaper,
    kick: KickVoice,
    song: CompiledSong,
}

impl Player for Fretboard {
    fn init(sample_rate: u32) -> Result<Self, String> {
        if sample_rate != SR {
            return Err(format!("multisampled-guitar is authored at {SR} Hz, host offered {sample_rate} Hz"));
        }

        let instrument = Multisample::new();
        for (root, name) in ZONES {
            // Prefer a real DI zone; fall back to a synthesised pluck.
            let sample = Sample::from_library(name)
                .unwrap_or_else(|| Sample::from_pcm(&karplus_strong(root, 0.8, 0.996, 0x5EED ^ root as u64)));
            instrument.add(&sample, root);
            // `sample` drops here; the host keeps its own Arc in the zone.
        }
        log::log(&format!("multisampled-guitar: {} zones loaded", instrument.zone_count()));

        let shaper = Waveshaper::new(SR, 4);
        shaper.set_shape(ShapeKind::AsymTanh, 3.5, 0.1);
        shaper.set_tone(90.0, 7500.0);

        Ok(Self {
            instrument,
            voices: (0..VOICES).map(|_| SampleVoice::new()).collect(),
            next: 0,
            shaper,
            kick: KickVoice::metal(SR),
            song: RIFF.compile(SR)?,
        })
    }

    fn render(&mut self, start_frame: u64, num_frames: u32) -> Vec<f32> {
        let n = num_frames as usize;

        for ev in self.song.events_in_range(start_frame, num_frames as u64) {
            match (ev.lane, ev.cell) {
                ("gtr", Cell::Note(note)) => {
                    // Host picks the nearest zone and shifts only the remainder.
                    self.voices[self.next].trigger_pitched(&self.instrument, note.hz(), 0.8);
                    self.next = (self.next + 1) % VOICES;
                }
                ("kick", Cell::Hit { .. }) => self.kick.trigger(),
                _ => {}
            }
        }

        let mut gtr = vec![0.0f32; n];
        for voice in &self.voices {
            let block = voice.render(num_frames);
            for (g, s) in gtr.iter_mut().zip(block.iter()) {
                *g += *s;
            }
        }

        let crunch = self.shaper.process(&gtr);
        let kick = self.kick.render_block(num_frames);

        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let duck = 1.0 - 0.5 * kick[i].abs().min(1.0);
            let mix = soft_clip(0.7 * crunch[i] * duck + 0.9 * kick[i]);
            out.push(mix);
            out.push(mix);
        }
        out
    }

    fn reset(&mut self) {
        for v in &self.voices {
            v.stop();
        }
        self.shaper.reset();
        self.kick.reset();
    }

    fn is_finished(&self) -> bool {
        false
    }

    fn metadata(&self) -> Metadata {
        Metadata {
            title: "Fretboard".to_string(),
            artist: "blackcap".to_string(),
            duration_frames: self.song.duration_frames(),
            sample_rate: SR,
            loop_point: Some(LoopPoint {
                start_frame: 0,
                end_frame: self.song.duration_frames(),
            }),
            cover_art: None,
            tags: vec!["sampled".to_string(), "multisample".to_string()],
        }
    }
}

export_player!(Fretboard);

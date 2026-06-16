//! "Plucked" — the v2 sampler demo. Proves the sample-playback path end-to-end
//! WITHOUT any external files: at init it synthesises a Karplus-Strong plucked
//! string into a PCM buffer, registers it as a host sample, then plays a riff by
//! triggering pitch-shifted voices, all crunched through the host waveshaper.
//!
//! To hear a REAL guitar instead, drop `guitar.wav` into ~/.jukebox/samples and
//! it'll be used in place of the synthesised pluck (see `init`).

use jukebox_cartridge_sdk::dsp::{ShapeKind, Waveshaper};
use jukebox_cartridge_sdk::osc::white;
use jukebox_cartridge_sdk::prelude::*;
use jukebox_cartridge_sdk::sampler::{Sample, SampleVoice};

const SR: u32 = 48_000;
const ROOT_HZ: f32 = 110.0; // A2 — synth root; pitch-shift up from here
const VOICES: usize = 6;

const RIFF: TrackerSong = song! {
    tempo: 140;
    rows_per_beat: 4;

    pattern "a" {
        gtr:  "a2 -  e3 -   a2 -  g3 -   a2 -  e3 -   f3 -  e3 -    a2 -  e3 -   a2 -  c4 -   a3 -  g3 -   e3 -  -  -";
        kick: "x  -  -  x   x  -  -  x   x  -  -  x   x  -  -  x    x  -  -  x   x  -  -  x   x  -  -  x   x  -  x  x";
    }

    sequence: [a, a];
};

/// A Karplus-Strong plucked string: a noise burst recirculated through a short
/// delay with an averaging lowpass — the classic cheap "string" synth.
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

struct Plucked {
    sample: Sample,
    voices: Vec<SampleVoice>,
    next: usize,
    shaper: Waveshaper,
    kick: KickVoice,
    song: CompiledSong,
}

impl Player for Plucked {
    fn init(sample_rate: u32) -> Result<Self, String> {
        if sample_rate != SR {
            return Err(format!("sampled-guitar is authored at {SR} Hz, host offered {sample_rate} Hz"));
        }

        // Prefer a real guitar from the library; fall back to a synth pluck.
        let sample = match Sample::from_library("guitar") {
            Some(s) => {
                log::log("sampled-guitar: using ~/.jukebox/samples/guitar.wav");
                s
            }
            None => {
                log::log("sampled-guitar: no guitar.wav — synthesising a Karplus-Strong pluck");
                Sample::from_pcm(&karplus_strong(ROOT_HZ, 0.8, 0.996, 0x51A77ED_5EED))
            }
        };

        let shaper = Waveshaper::new(SR, 4);
        shaper.set_shape(ShapeKind::AsymTanh, 3.5, 0.1);
        shaper.set_tone(90.0, 7500.0);

        Ok(Self {
            sample,
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
                    // Pitch-shift the sample by playback speed (sampler-style).
                    let speed = note.hz() / ROOT_HZ;
                    self.voices[self.next].trigger(&self.sample, speed, 0.8);
                    self.next = (self.next + 1) % VOICES;
                }
                ("kick", Cell::Hit { .. }) => self.kick.trigger(),
                _ => {}
            }
        }

        // Sum the polyphonic sample voices.
        let mut gtr = vec![0.0f32; n];
        for voice in &self.voices {
            let block = voice.render(num_frames);
            for (g, s) in gtr.iter_mut().zip(block.iter()) {
                *g += *s;
            }
        }

        // Crunch the guitar bus; add the kick under it.
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
            title: "Plucked".to_string(),
            artist: "blackcap".to_string(),
            duration_frames: self.song.duration_frames(),
            sample_rate: SR,
            loop_point: Some(LoopPoint {
                start_frame: 0,
                end_frame: self.song.duration_frames(),
            }),
            cover_art: None,
            tags: vec!["sampled".to_string(), "karplus-strong".to_string()],
        }
    }
}

export_player!(Plucked);

//! "Arpeggio Aurora" — the M2 hello-world: three channels (supersaw lead,
//! square bass, noise hat) driven by the `song!{}` tracker DSL, all through the
//! cartridge SDK. No host DSP imports yet (that's M3); everything is inline.

use jukebox_cartridge_sdk::dsp::Svf;
use jukebox_cartridge_sdk::osc::white;
use jukebox_cartridge_sdk::prelude::*;

const SR: u32 = 48_000;

// A minor arpeggio over a slow root-note bass and a straight-eighths hat.
const SONG: TrackerSong = song! {
    tempo: 140;
    rows_per_beat: 4; // 16th-note grid

    pattern "a" {
        lead: "a4 c5 e5 a5  e5 c5 a4 c5  a4 c5 e5 a5  b5 a5 e5 c5";
        bass: "a2 -  -  -   a2 -  -  -   f2 -  -  -   e2 -  -  -";
        hat:  "x  -  x  -   x  -  x  -   x  -  x  -   x  -  x  x";
    }

    sequence: [a, a, a, a];
};

struct Arpeggio {
    lead: SawSuperVoice<5>,
    lead_env: Adsr,
    bass: SquareVoice,
    bass_env: Adsr,
    hat_noise_state: u64,
    hat_bp: Svf,
    hat_env: Adsr,
    song: CompiledSong,
}

impl Player for Arpeggio {
    fn init(sample_rate: u32) -> Result<Self, String> {
        if sample_rate != SR {
            return Err(format!("arpeggio is authored at {SR} Hz, host offered {sample_rate} Hz"));
        }
        let mut hat_bp = Svf::new(SR);
        hat_bp.set_params(8000.0, 4.0);
        Ok(Self {
            lead: SawSuperVoice::new(SR, 14.0),
            lead_env: Adsr::new(SR, 0.003, 0.18, 0.0, 0.05),
            bass: SquareVoice::new(SR, 0.35),
            bass_env: Adsr::new(SR, 0.004, 0.20, 0.6, 0.06),
            hat_noise_state: 0xC0FF_EE00_1234_5678,
            hat_bp,
            hat_env: Adsr::new(SR, 0.0, 0.045, 0.0, 0.0),
            song: SONG.compile(SR)?,
        })
    }

    fn render(&mut self, start_frame: u64, num_frames: u32) -> Vec<f32> {
        let n = num_frames as usize;

        // Dispatch this block's note/trigger events.
        for ev in self.song.events_in_range(start_frame, num_frames as u64) {
            match (ev.lane, ev.cell) {
                ("lead", Cell::Note(note)) => {
                    self.lead.note_on(note.hz());
                    self.lead_env.trigger();
                }
                ("bass", Cell::Note(note)) => {
                    self.bass.note_on(note.hz());
                    self.bass_env.trigger();
                }
                ("hat", Cell::Hit { .. }) => self.hat_env.trigger(),
                _ => {}
            }
        }

        // Render each channel's raw source + envelope.
        let lead_raw = self.lead.render_block(num_frames);
        let lead_env = self.lead_env.render_block(num_frames);
        let bass_raw = self.bass.render_block(num_frames);
        let bass_env = self.bass_env.render_block(num_frames);
        let hat_env = self.hat_env.render_block(num_frames);

        let mut out_l = vec![0.0f32; n];
        let mut out_r = vec![0.0f32; n];

        for i in 0..n {
            // Hat: band-passed noise, sharp AR, panned slightly right.
            let noise = white(&mut self.hat_noise_state);
            let (_, _, hat_bp) = self.hat_bp.process_one(noise);
            let hat = hat_bp * hat_env[i] * 0.5;

            let lead = 0.35 * lead_raw[i] * lead_env[i];
            let bass = 0.40 * bass_raw[i] * bass_env[i];

            out_l[i] = soft_clip(lead + bass + hat * 0.7);
            out_r[i] = soft_clip(lead + bass + hat);
        }

        interleave(&out_l, &out_r)
    }

    fn reset(&mut self) {
        self.lead.reset();
        self.bass.reset();
        self.lead_env.reset();
        self.bass_env.reset();
        self.hat_env.reset();
        self.hat_bp.reset();
    }

    fn is_finished(&self) -> bool {
        false
    }

    fn metadata(&self) -> Metadata {
        Metadata {
            title: "Arpeggio Aurora".to_string(),
            artist: "blackcap".to_string(),
            duration_frames: 0, // looping
            sample_rate: SR,
            loop_point: Some(LoopPoint {
                start_frame: 0,
                end_frame: self.song.duration_frames(),
            }),
            cover_art: None,
            tags: vec!["demo".to_string(), "arpeggio".to_string()],
        }
    }
}

export_player!(Arpeggio);

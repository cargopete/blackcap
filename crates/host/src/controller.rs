//! Orchestration shared by the headless loop and the TUI: owns the active and
//! retiring workers, issues crossfades, and keeps the shared now-playing state
//! current.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_channel::Sender;
use wasmtime::Engine;

use crate::mixer::Switch;
use crate::shared::{NowPlaying, Shared};
use crate::worker::{self, Worker};

pub struct Controller {
    engine: Engine,
    cmd_tx: Sender<Switch>,
    pub shared: Arc<Shared>,
    sr: u32,
    block_frames: u32,
    fade_frames: usize,
    fade_dur: Duration,
    active: Option<Worker>,
    retiring: Vec<(Worker, Instant)>,
}

impl Controller {
    pub fn new(
        engine: Engine,
        cmd_tx: Sender<Switch>,
        shared: Arc<Shared>,
        sr: u32,
        block_frames: u32,
        fade_frames: usize,
        fade_dur: Duration,
    ) -> Self {
        Self {
            engine,
            cmd_tx,
            shared,
            sr,
            block_frames,
            fade_frames,
            fade_dur,
            active: None,
            retiring: Vec::new(),
        }
    }

    pub fn active_title(&self) -> Option<(&str, &str)> {
        self.active.as_ref().map(|w| (w.title.as_str(), w.artist.as_str()))
    }

    fn announce(&self, w: &Worker) {
        self.shared.set_now(NowPlaying {
            title: w.title.clone(),
            artist: w.artist.clone(),
            sample_rate: w.sample_rate,
            duration_frames: w.duration_frames,
            origin_frame: self.shared.frames_played(),
            tags: w.tags.clone(),
        });
    }

    fn install(&mut self, mut w: Worker) {
        // No fade for the first source; equal-power crossfade thereafter.
        let fade = if self.active.is_some() { self.fade_frames } else { 0 };
        self.cmd_tx
            .send(Switch { consumer: w.take_consumer(), fade_frames: fade })
            .ok();
        self.announce(&w);
        if let Some(old) = self.active.take() {
            self.retiring.push((old, Instant::now() + self.fade_dur));
        }
        self.active = Some(w);
    }

    pub fn play_sine(&mut self, freq: f32, amplitude: f32) {
        let w = worker::spawn_sine(self.sr, freq, amplitude, self.block_frames);
        self.install(w);
    }

    /// Spawn a cartridge worker and crossfade to it.
    pub fn crossfade_to(&mut self, path: &Path) -> Result<()> {
        let w = worker::spawn_cartridge(&self.engine, path, self.sr, self.block_frames)?;
        self.install(w);
        Ok(())
    }

    /// Tear down workers whose fade-out has completed.
    pub fn reap(&mut self) {
        let now = Instant::now();
        self.retiring.retain_mut(|(w, deadline)| {
            if now >= *deadline {
                w.stop();
                false
            } else {
                true
            }
        });
    }

    pub fn shutdown(&mut self) {
        if let Some(mut w) = self.active.take() {
            w.stop();
        }
        for (mut w, _) in self.retiring.drain(..) {
            w.stop();
        }
    }
}

//! State shared between the audio/mixer threads and the TUI: a VU level, the
//! running frame count (for the timeline), the underrun counter, and the
//! now-playing metadata.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Clone, Default)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub sample_rate: u32,
    /// 0 = generative / no fixed length.
    pub duration_frames: u64,
    /// `frames_played` at the moment this became the active source.
    pub origin_frame: u64,
    pub tags: Vec<String>,
}

pub struct Shared {
    vu: AtomicU32, // f32 bits, smoothed output peak
    frames_played: AtomicU64,
    now: Mutex<NowPlaying>,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            vu: AtomicU32::new(0),
            frames_played: AtomicU64::new(0),
            now: Mutex::new(NowPlaying::default()),
        }
    }

    #[inline]
    pub fn set_vu(&self, level: f32) {
        self.vu.store(level.to_bits(), Ordering::Relaxed);
    }

    #[inline]
    pub fn vu(&self) -> f32 {
        f32::from_bits(self.vu.load(Ordering::Relaxed))
    }

    #[inline]
    pub fn add_frames(&self, n: u64) {
        self.frames_played.fetch_add(n, Ordering::Relaxed);
    }

    #[inline]
    pub fn frames_played(&self) -> u64 {
        self.frames_played.load(Ordering::Relaxed)
    }

    pub fn set_now(&self, now: NowPlaying) {
        *self.now.lock().unwrap() = now;
    }

    pub fn now(&self) -> NowPlaying {
        self.now.lock().unwrap().clone()
    }
}

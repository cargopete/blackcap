//! The "hello world" cartridge: a 220 Hz sine. Hand-rolled against the raw
//! wit-bindgen output (no SDK yet — that's M2).
//!
//! The only state it needs is the sample rate, learned at `init()` and used in
//! `render()`. Everything else is derived from the absolute frame index, so
//! `render()` is pure: the same `start_frame` always yields the same block.

use std::sync::atomic::{AtomicU32, Ordering};

wit_bindgen::generate!({
    path: "../../wit",
    world: "cartridge",
});

use exports::jukebox::cartridge::player::{Guest, Metadata};

const AUTHORED_SR: u32 = 48_000;
const FREQ: f64 = 220.0; // A3
const AMP: f32 = 0.2; // ~-14 dBFS

/// Set once at init; wasm is single-threaded so contention is moot.
static SAMPLE_RATE: AtomicU32 = AtomicU32::new(AUTHORED_SR);

struct Cart;

impl Guest for Cart {
    fn init(sample_rate: u32) -> Result<(), String> {
        if sample_rate != AUTHORED_SR {
            return Err(format!(
                "sine-cartridge is authored at {AUTHORED_SR} Hz, host offered {sample_rate} Hz"
            ));
        }
        SAMPLE_RATE.store(sample_rate, Ordering::Relaxed);
        Ok(())
    }

    fn render(start_frame: u64, num_frames: u32) -> Vec<f32> {
        let sr = SAMPLE_RATE.load(Ordering::Relaxed) as f64;
        let mut out = Vec::with_capacity(num_frames as usize * 2);
        for i in 0..num_frames as u64 {
            // f64 phase so the tone doesn't drift after millions of frames.
            let t = (start_frame + i) as f64 / sr;
            let s = (AMP as f64 * (core::f64::consts::TAU * FREQ * t).sin()) as f32;
            out.push(s); // L
            out.push(s); // R
        }
        out
    }

    fn seek(_frame: u64) {}

    fn reset() {}

    fn is_finished() -> bool {
        false // generative: plays forever
    }

    fn get_metadata() -> Metadata {
        Metadata {
            title: "Sine Qua Non".to_string(),
            artist: "blackcap".to_string(),
            duration_frames: 0, // 0 = infinite/generative
            sample_rate: AUTHORED_SR,
            loop_point: None,
            cover_art: None,
            tags: vec!["test".to_string(), "sine".to_string()],
        }
    }
}

export!(Cart);

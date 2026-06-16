//! A block source produces interleaved-stereo audio on demand. Both the M0
//! built-in sine and an M1 wasm cartridge expose the same shape, so the ring
//! producer worker doesn't care which it's driving.

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rtrb::Producer;

/// Fill `num_frames` of interleaved stereo starting at absolute `start_frame`.
/// Returns `2 * num_frames` samples. May fail (e.g. a cartridge trap).
pub type BlockSource = Box<dyn FnMut(u64, u32) -> Result<Vec<f32>> + Send>;

/// Stateless sine, phase derived from the absolute frame index so it matches
/// the cartridge contract (no hidden between-call state). A3 = 220 Hz, -14 dBFS.
pub fn sine_source(sample_rate: u32, freq_hz: f32, amplitude: f32) -> BlockSource {
    let sr = sample_rate as f32;
    Box::new(move |start_frame: u64, num_frames: u32| {
        let mut block = Vec::with_capacity(num_frames as usize * 2);
        for i in 0..num_frames as u64 {
            let t = (start_frame + i) as f32 / sr;
            let s = amplitude * (std::f32::consts::TAU * freq_hz * t).sin();
            block.push(s); // L
            block.push(s); // R
        }
        Ok(block)
    })
}

/// Drive a [`BlockSource`] into the ring until `running` clears or the source
/// errors. Sleeps briefly when the ring has no headroom (back-pressure).
pub fn run_producer(
    mut producer: Producer<f32>,
    mut source: BlockSource,
    block_frames: u32,
    running: Arc<AtomicBool>,
) {
    let need = block_frames as usize * 2;
    let mut start_frame: u64 = 0;

    while running.load(Ordering::Relaxed) {
        if producer.slots() < need {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        match source(start_frame, block_frames) {
            Ok(block) => {
                for s in block {
                    if producer.push(s).is_err() {
                        break; // ring filled out from under us; loop will re-check
                    }
                }
                start_frame += block_frames as u64;
            }
            Err(e) => {
                eprintln!("blackcap: render failed: {e:#} — fading to silence");
                running.store(false, Ordering::Relaxed);
                break;
            }
        }
    }
}

/// Simple peak / RMS for headless verification.
pub fn block_stats(block: &[f32]) -> (f32, f32) {
    let mut peak = 0.0f32;
    let mut sumsq = 0.0f64;
    for &s in block {
        peak = peak.max(s.abs());
        sumsq += (s as f64) * (s as f64);
    }
    let rms = if block.is_empty() {
        0.0
    } else {
        (sumsq / block.len() as f64).sqrt() as f32
    };
    (peak, rms)
}

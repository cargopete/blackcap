//! cpal output: pick a config (preferring 48 kHz stereo f32), then drain the
//! SPSC ring in the callback. The callback allocates nothing and never logs —
//! on an empty ring it writes silence and bumps an atomic underrun counter.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use rtrb::Consumer;

/// The cartridge contract is 48 kHz by convention; ask the device for it.
pub const PREFERRED_SAMPLE_RATE: u32 = 48_000;

pub struct AudioOut {
    /// Held to keep the stream alive; dropping it stops playback.
    pub stream: cpal::Stream,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Open the default output device and start draining `consumer`.
///
/// The ring always carries interleaved stereo pairs (L, R, L, R, …) regardless
/// of the device channel count; the callback maps a pair onto the device's
/// channels (downmix to mono, pass-through stereo, or fill extra channels with
/// silence).
pub fn open(consumer: Consumer<f32>, underruns: Arc<AtomicU64>) -> Result<AudioOut> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("no default output device"))?;

    let supported = pick_config(&device)?;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.config();
    let channels = config.channels;
    let sample_rate = config.sample_rate; // cpal 0.18: SampleRate is a u32 alias

    let stream = match sample_format {
        SampleFormat::F32 => build::<f32>(&device, config, channels, consumer, underruns)?,
        SampleFormat::I16 => build::<i16>(&device, config, channels, consumer, underruns)?,
        SampleFormat::U16 => build::<u16>(&device, config, channels, consumer, underruns)?,
        other => bail!("unsupported sample format: {other:?}"),
    };

    Ok(AudioOut {
        stream,
        sample_rate,
        channels,
    })
}

/// Prefer f32 + 2ch + 48 kHz; fall back to the device default if nothing fits.
fn pick_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig> {
    let target = PREFERRED_SAMPLE_RATE; // cpal 0.18: SampleRate is a u32 alias

    let mut best: Option<(i32, cpal::SupportedStreamConfig)> = None;
    if let Ok(ranges) = device.supported_output_configs() {
        for range in ranges {
            if range.min_sample_rate() > target || range.max_sample_rate() < target {
                continue;
            }
            let cfg = range.with_sample_rate(target);
            // Higher score = better fit.
            let mut score = 0;
            if cfg.sample_format() == SampleFormat::F32 {
                score += 2;
            }
            if cfg.channels() == 2 {
                score += 1;
            }
            if best.as_ref().map_or(true, |(b, _)| score > *b) {
                best = Some((score, cfg));
            }
        }
    }

    match best {
        Some((_, cfg)) => Ok(cfg),
        None => device
            .default_output_config()
            .map_err(|e| anyhow!("no usable output config: {e}")),
    }
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: u16,
    mut consumer: Consumer<f32>,
    underruns: Arc<AtomicU64>,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let ch = channels as usize;
    let stream = device.build_output_stream(
        config,
        move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
            for frame in out.chunks_mut(ch) {
                let l = consumer.pop().unwrap_or_else(|_| {
                    underruns.fetch_add(1, Ordering::Relaxed);
                    0.0
                });
                let r = consumer.pop().unwrap_or_else(|_| {
                    underruns.fetch_add(1, Ordering::Relaxed);
                    0.0
                });
                match ch {
                    1 => frame[0] = T::from_sample((l + r) * 0.5),
                    _ => {
                        frame[0] = T::from_sample(l);
                        frame[1] = T::from_sample(r);
                        for extra in frame.iter_mut().skip(2) {
                            *extra = T::from_sample(0.0f32);
                        }
                    }
                }
            }
        },
        |err| eprintln!("cpal stream error: {err}"),
        None,
    )?;
    Ok(stream)
}

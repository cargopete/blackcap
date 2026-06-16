//! blackcap — a WebAssembly jukebox.
//!
//! Loads `.wasm` component cartridges (or a built-in sine, for M0) and plays
//! them through cpal via a buffered SPSC ring. See `wit/jukebox.wit` for the
//! cartridge contract.

mod audio;
mod engine;
mod player;
mod source;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use player::Cartridge;
use source::{block_stats, run_producer, sine_source, BlockSource};

/// ~0.5 s of interleaved stereo at 48 kHz. Paranoid but cheap; absorbs cold
/// starts and the occasional slow render.
const RING_CAPACITY: usize = 48_000;
const DEFAULT_BLOCK_FRAMES: u32 = 1024;
const PREFILL: Duration = Duration::from_millis(120);

const SINE_FREQ: f32 = 220.0; // A3
const SINE_AMP: f32 = 0.2; // ~-14 dBFS

struct Args {
    cartridge: Option<PathBuf>,
    sine: bool,
    dry_run: bool,
    seconds: Option<f64>,
    blocks: usize,
    block_frames: u32,
    sample_rate: u32,
    freq: f32,
}

fn print_help() {
    println!(
        "blackcap — a WebAssembly jukebox\n\n\
         USAGE:\n    blackcap [CARTRIDGE.wasm] [OPTIONS]\n\n\
         With no cartridge (or --sine) the built-in sine plays (M0).\n\n\
         OPTIONS:\n\
         \x20   --sine               Play the built-in sine instead of a cartridge\n\
         \x20   --dry-run            No audio device: render blocks and print peak/RMS\n\
         \x20   --seconds <N>        Auto-stop after N seconds (audio mode)\n\
         \x20   --blocks <N>         Blocks to render in --dry-run (default 4)\n\
         \x20   --block-frames <N>   Frames per render block (default 1024)\n\
         \x20   --sample-rate <N>    Sample rate for --dry-run (default 48000)\n\
         \x20   --freq <HZ>          Built-in sine frequency (default 220)\n\
         \x20   -h, --help           Print this help"
    );
}

fn parse_args() -> Result<Option<Args>> {
    let mut args = Args {
        cartridge: None,
        sine: false,
        dry_run: false,
        seconds: None,
        blocks: 4,
        block_frames: DEFAULT_BLOCK_FRAMES,
        sample_rate: audio::PREFERRED_SAMPLE_RATE,
        freq: SINE_FREQ,
    };

    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut next_val = |name: &str| -> Result<String> {
            it.next()
                .ok_or_else(|| anyhow::anyhow!("{name} requires a value"))
        };
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            "--sine" => args.sine = true,
            "--dry-run" => args.dry_run = true,
            "--seconds" => args.seconds = Some(next_val("--seconds")?.parse()?),
            "--blocks" => args.blocks = next_val("--blocks")?.parse()?,
            "--block-frames" => args.block_frames = next_val("--block-frames")?.parse()?,
            "--sample-rate" => args.sample_rate = next_val("--sample-rate")?.parse()?,
            "--freq" => args.freq = next_val("--freq")?.parse()?,
            other if other.starts_with("--") => bail!("unknown option: {other}"),
            other => args.cartridge = Some(PathBuf::from(other)),
        }
    }
    Ok(Some(args))
}

fn main() -> Result<()> {
    let args = match parse_args()? {
        Some(a) => a,
        None => return Ok(()),
    };

    let use_sine = args.sine || args.cartridge.is_none();

    if args.dry_run {
        return dry_run(&args, use_sine);
    }
    play(&args, use_sine)
}

/// Headless: render a few blocks and print stats. No audio device touched —
/// this is how we verify the wasm path without hardware.
fn dry_run(args: &Args, use_sine: bool) -> Result<()> {
    let sr = args.sample_rate;
    let (mut src, desc) = build_source(args, use_sine, sr)?;
    println!("blackcap dry-run: {desc}");
    println!("  sample-rate={sr} Hz  block-frames={}  blocks={}", args.block_frames, args.blocks);

    let mut peak_all = 0.0f32;
    for b in 0..args.blocks {
        let start = b as u64 * args.block_frames as u64;
        let block = src(start, args.block_frames)?;
        let (peak, rms) = block_stats(&block);
        peak_all = peak_all.max(peak);
        println!(
            "  block {b:>3}: start_frame={start:>8}  len={:>5}  peak={peak:.4}  rms={rms:.4}",
            block.len()
        );
    }
    println!("  overall peak={peak_all:.4}");
    if peak_all <= 0.0 {
        bail!("source produced pure silence — something is wrong");
    }
    Ok(())
}

/// Open the audio device and play until Ctrl+C or --seconds.
fn play(args: &Args, use_sine: bool) -> Result<()> {
    let underruns = Arc::new(AtomicU64::new(0));
    let running = Arc::new(AtomicBool::new(true));

    let (producer, consumer) = rtrb::RingBuffer::<f32>::new(RING_CAPACITY);
    let out = audio::open(consumer, Arc::clone(&underruns))?;
    println!(
        "blackcap: device {} Hz, {} channel(s)",
        out.sample_rate, out.channels
    );

    let (source, desc) = build_source(args, use_sine, out.sample_rate)?;
    println!("blackcap: now playing — {desc}");

    // Worker fills the ring; give it a head start before we open the tap.
    let worker = {
        let running = Arc::clone(&running);
        let block_frames = args.block_frames;
        std::thread::spawn(move || run_producer(producer, source, block_frames, running))
    };
    std::thread::sleep(PREFILL);

    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || running.store(false, Ordering::Relaxed))
            .expect("failed to set Ctrl+C handler");
    }

    use cpal::traits::StreamTrait;
    out.stream.play()?;

    let started = Instant::now();
    while running.load(Ordering::Relaxed) {
        if let Some(limit) = args.seconds {
            if started.elapsed() >= Duration::from_secs_f64(limit) {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    running.store(false, Ordering::Relaxed);
    drop(out.stream); // stop the device
    let _ = worker.join();

    let total = underruns.load(Ordering::Relaxed);
    if total > 0 {
        eprintln!("blackcap: {total} underrun sample(s) — ring starved at some point");
    }
    println!("blackcap: stopped cleanly.");
    Ok(())
}

/// Build the right [`BlockSource`] (sine or cartridge) for `sample_rate`.
fn build_source(args: &Args, use_sine: bool, sample_rate: u32) -> Result<(BlockSource, String)> {
    if use_sine {
        let desc = format!("built-in sine {:.1} Hz", args.freq);
        return Ok((sine_source(sample_rate, args.freq, SINE_AMP), desc));
    }

    let path = args.cartridge.clone().expect("cartridge path present");
    let engine = engine::make_engine()?;
    engine::spawn_epoch_ticker(&engine);
    let mut cart = Cartridge::load(&engine, &path, sample_rate)?;
    let length = if cart.duration_frames == 0 {
        "generative".to_string()
    } else {
        format!("{} frames", cart.duration_frames)
    };
    let desc = format!(
        "cartridge \"{}\" by {} [{} Hz, {}] ({})",
        cart.title,
        cart.artist,
        cart.sample_rate,
        length,
        path.display()
    );
    // Move the cartridge (and a strong engine ref to keep it alive) into the
    // render closure so the worker thread owns the whole wasm world.
    let source: BlockSource = Box::new(move |start, num| {
        let _keep_engine_alive = &engine;
        cart.render(start, num)
    });
    Ok((source, desc))
}

//! blackcap — a WebAssembly jukebox.
//!
//! Loads `.wasm` component cartridges (or a built-in sine) and plays them
//! through cpal. Render workers feed their own rings; a mixer thread crossfades
//! between them and applies the master chain before the output ring. `--tui`
//! puts a ratatui front-end on top.

mod audio;
mod controller;
mod engine;
mod host;
mod master;
mod mixer;
mod player;
mod sampler;
mod shared;
mod source;
mod tui;
mod wit;
mod worker;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{bail, Result};

use controller::Controller;
use master::MasterChain;
use mixer::{Mixer, Switch};
use shared::Shared;
use source::{block_stats, sine_source, BlockSource};

/// ~0.5 s of interleaved stereo at 48 kHz per ring.
pub const RING_CAPACITY: usize = 48_000;
const DEFAULT_BLOCK_FRAMES: u32 = 1024;
const PREFILL: Duration = Duration::from_millis(120);

const SINE_FREQ: f32 = 220.0; // A3
const SINE_AMP: f32 = 0.2; // ~-14 dBFS

/// Extra time after a fade before tearing down the faded-out worker.
const RETIRE_MARGIN: Duration = Duration::from_millis(300);

struct Args {
    cartridges: Vec<PathBuf>,
    sine: bool,
    dry_run: bool,
    tui: bool,
    watch: bool,
    no_master: bool,
    seconds: Option<f64>,
    fade_ms: u64,
    fade_after: f64,
    blocks: usize,
    block_frames: u32,
    sample_rate: u32,
    freq: f32,
}

fn print_help() {
    println!(
        "blackcap — a WebAssembly jukebox\n\n\
         USAGE:\n    blackcap [CARTRIDGE.wasm ...] [OPTIONS]\n\n\
         No cartridge (or --sine) plays the built-in sine. Two cartridges\n\
         crossfade. --watch and --tui hot-reload from ~/.jukebox/cartridges.\n\n\
         OPTIONS:\n\
         \x20   --tui                Interactive ratatui front-end\n\
         \x20   --watch              Watch ~/.jukebox/cartridges; crossfade to new drops\n\
         \x20   --sine               Play the built-in sine\n\
         \x20   --no-master          Bypass the master compressor + limiter\n\
         \x20   --fade-ms <N>        Crossfade length in ms (default 600)\n\
         \x20   --fade-after <S>     Seconds before the two-cartridge crossfade (default 4)\n\
         \x20   --dry-run            No audio device: render blocks and print peak/RMS\n\
         \x20   --seconds <N>        Auto-stop after N seconds\n\
         \x20   --blocks <N>         Blocks to render in --dry-run (default 4)\n\
         \x20   --block-frames <N>   Frames per render block (default 1024)\n\
         \x20   --sample-rate <N>    Sample rate for --dry-run (default 48000)\n\
         \x20   --freq <HZ>          Built-in sine frequency (default 220)\n\
         \x20   -h, --help           Print this help"
    );
}

fn parse_args() -> Result<Option<Args>> {
    let mut args = Args {
        cartridges: Vec::new(),
        sine: false,
        dry_run: false,
        tui: false,
        watch: false,
        no_master: false,
        seconds: None,
        fade_ms: 600,
        fade_after: 4.0,
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
            "--tui" => args.tui = true,
            "--sine" => args.sine = true,
            "--watch" => args.watch = true,
            "--no-master" => args.no_master = true,
            "--dry-run" => args.dry_run = true,
            "--fade-ms" => args.fade_ms = next_val("--fade-ms")?.parse()?,
            "--fade-after" => args.fade_after = next_val("--fade-after")?.parse()?,
            "--seconds" => args.seconds = Some(next_val("--seconds")?.parse()?),
            "--blocks" => args.blocks = next_val("--blocks")?.parse()?,
            "--block-frames" => args.block_frames = next_val("--block-frames")?.parse()?,
            "--sample-rate" => args.sample_rate = next_val("--sample-rate")?.parse()?,
            "--freq" => args.freq = next_val("--freq")?.parse()?,
            other if other.starts_with("--") => bail!("unknown option: {other}"),
            other => args.cartridges.push(PathBuf::from(other)),
        }
    }
    Ok(Some(args))
}

fn main() -> Result<()> {
    let args = match parse_args()? {
        Some(a) => a,
        None => return Ok(()),
    };

    let use_sine = args.sine || (args.cartridges.is_empty() && !args.watch && !args.tui);

    if args.dry_run {
        return dry_run(&args, use_sine);
    }
    if args.tui && !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        bail!("--tui needs an interactive terminal");
    }
    play(&args, use_sine)
}

/// Headless: render a few blocks of the first source and print stats.
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

fn build_source(args: &Args, use_sine: bool, sample_rate: u32) -> Result<(BlockSource, String)> {
    if use_sine {
        let desc = format!("built-in sine {:.1} Hz", args.freq);
        return Ok((sine_source(sample_rate, args.freq, SINE_AMP), desc));
    }
    let path = args.cartridges.first().expect("cartridge path present").clone();
    let engine = engine::make_engine()?;
    engine::spawn_epoch_ticker(&engine);
    let mut cart = player::Cartridge::load(&engine, &path, sample_rate)?;
    let desc = format!("cartridge \"{}\" by {} ({})", cart.title, cart.artist, path.display());
    let source: BlockSource = Box::new(move |start, num| {
        let _keep_engine_alive = &engine;
        cart.render(start, num)
    });
    Ok((source, desc))
}

/// Open the audio device, spin up the mixer, and run either the TUI or the
/// headless controller loop.
fn play(args: &Args, use_sine: bool) -> Result<()> {
    let underruns = Arc::new(AtomicU64::new(0));
    let running = Arc::new(AtomicBool::new(true));

    let (out_producer, out_consumer) = rtrb::RingBuffer::<f32>::new(RING_CAPACITY);
    let out = audio::open(out_consumer, Arc::clone(&underruns))?;
    let sr = out.sample_rate;
    let block_frames = args.block_frames;
    if !args.tui {
        println!("blackcap: device {} Hz, {} channel(s)", sr, out.channels);
        if args.no_master {
            println!("blackcap: master chain bypassed");
        }
    }

    let engine = engine::make_engine()?;
    engine::spawn_epoch_ticker(&engine);

    let shared = Arc::new(Shared::new());
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<Switch>();
    let master = MasterChain::new(sr, !args.no_master);
    let mixer = Mixer::new(out_producer, cmd_rx, Arc::clone(&running), block_frames as usize, master, Arc::clone(&shared));
    let mixer_handle = std::thread::spawn(move || mixer.run());

    let fade_frames = (args.fade_ms as f32 * 0.001 * sr as f32) as usize;
    let fade_dur = Duration::from_millis(args.fade_ms) + RETIRE_MARGIN;
    let mut ctl = Controller::new(engine, cmd_tx, Arc::clone(&shared), sr, block_frames, fade_frames, fade_dur);

    // Initial source.
    if use_sine {
        ctl.play_sine(args.freq, SINE_AMP);
    } else if !args.cartridges.is_empty() {
        ctl.crossfade_to(&args.cartridges[0])?;
    }
    if let Some((title, artist)) = ctl.active_title() {
        if !args.tui {
            println!("blackcap: now playing — {title} by {artist}");
        }
    }

    std::thread::sleep(PREFILL);
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || running.store(false, Ordering::Relaxed))
            .expect("failed to set Ctrl+C handler");
    }
    use cpal::traits::StreamTrait;
    out.stream.play()?;

    if args.tui {
        let dir = cartridges_dir();
        let _ = std::fs::create_dir_all(&dir);
        tui::run(&mut ctl, Arc::clone(&running), Arc::clone(&underruns), dir)?;
    } else {
        headless_loop(args, sr, &mut ctl, &running);
    }

    running.store(false, Ordering::Relaxed);
    drop(out.stream);
    let _ = mixer_handle.join();
    ctl.shutdown();

    let total = underruns.load(Ordering::Relaxed);
    if total > 0 {
        eprintln!("blackcap: {total} underrun sample(s)");
    }
    if !args.tui {
        println!("blackcap: stopped cleanly.");
    }
    Ok(())
}

/// The non-TUI controller loop: crossfade demo and/or hot-reload watching.
fn headless_loop(args: &Args, _sr: u32, ctl: &mut Controller, running: &Arc<AtomicBool>) {
    let watch_dir = cartridges_dir();
    let mut seen: HashMap<PathBuf, SystemTime> = HashMap::new();
    if args.watch {
        let _ = std::fs::create_dir_all(&watch_dir);
        scan_dir(&watch_dir, &mut seen);
        println!("blackcap: watching {} for cartridges", watch_dir.display());
    }

    let mut demo_done = false;
    let started = Instant::now();

    while running.load(Ordering::Relaxed) {
        if let Some(limit) = args.seconds {
            if started.elapsed() >= Duration::from_secs_f64(limit) {
                break;
            }
        }
        ctl.reap();

        if !demo_done
            && !args.watch
            && args.cartridges.len() >= 2
            && started.elapsed() >= Duration::from_secs_f64(args.fade_after)
        {
            if let Err(e) = ctl.crossfade_to(&args.cartridges[1]) {
                eprintln!("blackcap: {e:#}");
            } else if let Some((t, a)) = ctl.active_title() {
                println!("blackcap: crossfading to {t} by {a}");
            }
            demo_done = true;
        }

        if args.watch {
            if let Some(path) = poll_new_cartridge(&watch_dir, &mut seen) {
                match ctl.crossfade_to(&path) {
                    Ok(()) => println!("blackcap: crossfading to {}", path.display()),
                    Err(e) => eprintln!("blackcap: skipping {} — {e:#}", path.display()),
                }
            }
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}

fn cartridges_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".jukebox").join("cartridges")
}

fn scan_dir(dir: &Path, seen: &mut HashMap<PathBuf, SystemTime>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
                if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
                    seen.insert(path, mtime);
                }
            }
        }
    }
}

fn poll_new_cartridge(dir: &Path, seen: &mut HashMap<PathBuf, SystemTime>) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
            continue;
        }
        let mtime = match entry.metadata().and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if seen.get(&path).map_or(true, |&prev| mtime > prev) {
            seen.insert(path.clone(), mtime);
            return Some(path);
        }
    }
    None
}

//! A render worker: one source (cartridge or sine) running on its own thread,
//! filling its own ring. The mixer reads the consumer end. Stoppable so a
//! faded-out cartridge can be torn down cleanly (its Store + linear memory go
//! with it).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use anyhow::Result;
use rtrb::{Consumer, RingBuffer};
use wasmtime::Engine;

use crate::player::Cartridge;
use crate::source::{run_producer, sine_source, BlockSource};
use crate::RING_CAPACITY;

pub struct Worker {
    consumer: Option<Consumer<f32>>,
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    pub title: String,
    pub artist: String,
}

impl Worker {
    /// Hand the consumer end to the mixer. Callable once.
    pub fn take_consumer(&mut self) -> Consumer<f32> {
        self.consumer.take().expect("worker consumer already taken")
    }

    /// Signal the render thread to stop and join it.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn spawn(source: BlockSource, block_frames: u32, title: String, artist: String) -> Worker {
    let (producer, consumer) = RingBuffer::<f32>::new(RING_CAPACITY);
    let running = Arc::new(AtomicBool::new(true));
    let handle = {
        let running = Arc::clone(&running);
        std::thread::spawn(move || run_producer(producer, source, block_frames, running))
    };
    Worker {
        consumer: Some(consumer),
        running,
        handle: Some(handle),
        title,
        artist,
    }
}

/// A worker rendering the built-in sine.
pub fn spawn_sine(sample_rate: u32, freq: f32, amplitude: f32, block_frames: u32) -> Worker {
    spawn(
        sine_source(sample_rate, freq, amplitude),
        block_frames,
        format!("Sine {freq:.0} Hz"),
        "blackcap".to_string(),
    )
}

/// A worker rendering a `.wasm` cartridge. Loads + `init()`s on the calling
/// thread (so errors surface synchronously), then moves the cartridge into the
/// render thread.
pub fn spawn_cartridge(
    engine: &Engine,
    path: &std::path::Path,
    sample_rate: u32,
    block_frames: u32,
) -> Result<Worker> {
    let mut cart = Cartridge::load(engine, path, sample_rate)?;
    let title = cart.title.clone();
    let artist = cart.artist.clone();
    let source: BlockSource = Box::new(move |start, num| cart.render(start, num));
    Ok(spawn(source, block_frames, title, artist))
}

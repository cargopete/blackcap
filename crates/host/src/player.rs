//! Loading and driving a `.wasm` component cartridge.
//!
//! A cartridge is instantiated exactly once; the same instance is reused across
//! every `render()` so its linear memory (oscillator phases, reverb tails, …)
//! survives between blocks. We re-instantiate only on cartridge change.

use std::path::Path;

use anyhow::{anyhow, bail, Result};
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Engine, Store};

use crate::engine::EPOCH_BUDGET;
use crate::host::HostState;
use crate::wit;

pub struct Cartridge {
    store: Store<HostState>,
    world: wit::Cartridge,
    pub title: String,
    pub artist: String,
    pub sample_rate: u32,
    pub duration_frames: u64,
    pub tags: Vec<String>,
}

impl Cartridge {
    /// Compile, instantiate, `init(device_sr)`, and read metadata. Fails loudly
    /// on a sample-rate mismatch (v1 does no resampling).
    pub fn load(engine: &Engine, path: &Path, device_sr: u32) -> Result<Self> {
        let component = Component::from_file(engine, path)
            .map_err(|e| anyhow!("failed to load component {}: {e}", path.display()))?;

        let mut linker = Linker::<HostState>::new(engine);
        // std cartridges import WASI (stdio, env, …); the jukebox world imports
        // host dsp + log. Provide all of them.
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        wit::Cartridge::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |s| s)?;

        let mut store = Store::new(engine, HostState::new());
        store.set_epoch_deadline(EPOCH_BUDGET);

        let world = wit::Cartridge::instantiate(&mut store, &component, &linker)?;
        let player = world.jukebox_cartridge_player();

        player
            .call_init(&mut store, device_sr)?
            .map_err(|e| anyhow!("cartridge init() rejected sample rate: {e}"))?;

        let meta = player.call_get_metadata(&mut store)?;
        if meta.sample_rate != device_sr {
            bail!(
                "sample-rate mismatch: cartridge authored at {} Hz, device at {} Hz \
                 (v1 host does no resampling)",
                meta.sample_rate,
                device_sr
            );
        }

        Ok(Self {
            store,
            world,
            sample_rate: meta.sample_rate,
            title: meta.title,
            artist: meta.artist,
            duration_frames: meta.duration_frames,
            tags: meta.tags,
        })
    }

    /// Render one interleaved-stereo block. Arms the epoch deadline first so a
    /// hung cartridge traps instead of starving the ring forever.
    pub fn render(&mut self, start_frame: u64, num_frames: u32) -> Result<Vec<f32>> {
        self.store.set_epoch_deadline(EPOCH_BUDGET);
        let player = self.world.jukebox_cartridge_player();
        let block = player.call_render(&mut self.store, start_frame, num_frames)?;

        let expected = num_frames as usize * 2;
        if block.len() != expected {
            bail!(
                "cartridge returned {} samples for {} frames (expected {} interleaved L/R)",
                block.len(),
                num_frames,
                expected
            );
        }
        Ok(block)
    }

    /// Wired into the producer worker at M4 (loop / advance-to-next).
    #[allow(dead_code)]
    pub fn is_finished(&mut self) -> Result<bool> {
        let player = self.world.jukebox_cartridge_player();
        Ok(player.call_is_finished(&mut self.store)?)
    }
}

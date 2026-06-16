//! Shared wasmtime engine plumbing: component-model config + the epoch ticker
//! that is our runaway-cartridge protection.

use anyhow::Result;
use wasmtime::{Config, Engine, OptLevel};

/// Epoch tick interval. A background thread bumps the engine epoch this often;
/// a `render()` that doesn't return within `EPOCH_BUDGET` ticks traps.
pub const EPOCH_TICK: std::time::Duration = std::time::Duration::from_millis(50);

/// Generous per-render deadline in epoch ticks. 20 * 50ms = ~1s — far longer
/// than any honest render of a 1024-frame block, short enough to kill a hang.
pub const EPOCH_BUDGET: u64 = 20;

pub fn make_engine() -> Result<Engine> {
    let mut cfg = Config::new();
    cfg.wasm_component_model(true);
    cfg.epoch_interruption(true);
    cfg.cranelift_opt_level(OptLevel::Speed);
    // wasmtime 45 has its own error type; coerce into anyhow.
    Ok(Engine::new(&cfg)?)
}

/// Spawn a detached thread that increments the engine epoch every [`EPOCH_TICK`].
///
/// Holds only a weak handle so the ticker never keeps the engine alive — when
/// the host drops its `Engine`, `upgrade()` returns `None` and the thread ends.
pub fn spawn_epoch_ticker(engine: &Engine) {
    let weak = engine.weak();
    std::thread::spawn(move || loop {
        std::thread::sleep(EPOCH_TICK);
        match weak.upgrade() {
            Some(engine) => engine.increment_epoch(),
            None => break,
        }
    });
}

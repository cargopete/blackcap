# blackcap

A WebAssembly jukebox. Cartridges are `.wasm` **component** files that export a
tiny audio interface; the host loads them, calls `render()` on a worker thread,
and plays the result through [cpal] over a wait-free SPSC ring buffer.

> Status: **M1**. The host plays the built-in sine (M0) *and* loads a real wasm
> cartridge that exports `render()` (M1). The cartridge SDK, host DSP imports,
> hot-reload/crossfade, and the TUI are still ahead (M2–M6).

## The shape of it

```
cartridge.wasm ──load──▶ wasmtime (component model)
                              │  render(start, n) -> list<f32>  (worker thread)
                              ▼
                         rtrb SPSC ring ──drain──▶ cpal callback ──▶ your speakers
```

- **Buffered, never in the callback.** A wasmtime call goes through JIT'd code,
  the canonical ABI, and an allocator — none of it bounded by what the audio
  thread needs. So we render ahead into a ring and the callback only drains.
- **One instance, reused.** The cartridge is instantiated once; its linear
  memory (phases, filter state) survives between blocks. Re-instantiation only
  happens on cartridge change.
- **Epoch interruption.** A background thread bumps the engine epoch every
  50 ms; a `render()` that hangs traps after a generous deadline instead of
  starving the ring forever.

The cartridge contract lives in [`wit/jukebox.wit`](wit/jukebox.wit).

## Build & run

Requires a Rust toolchain with the `wasm32-wasip2` target:

```sh
rustup target add wasm32-wasip2
```

With [`just`](https://github.com/casey/just):

```sh
just sine        # M0: built-in sine through your speakers
just play        # M1: build the sine cartridge and play it
just dry-run     # headless: render blocks, print peak/RMS (no audio device)
just inspect     # show the WIT the built cartridge exports
```

Or by hand:

```sh
# Host
cargo build -p blackcap --release

# Cartridge → wasm component
cd examples/sine-cartridge
cargo build --target wasm32-wasip2 --release

# Play it
cargo run -p blackcap --release -- \
    examples/sine-cartridge/target/wasm32-wasip2/release/sine_cartridge.wasm
```

`Ctrl+C` stops cleanly. `--seconds N` auto-stops. `--dry-run` skips the audio
device entirely and is the way to verify a cartridge on a headless box.

## Layout

```
wit/jukebox.wit            cartridge contract (types + player + world)
crates/host/               the jukebox host (cpal + wasmtime + rtrb)
examples/sine-cartridge/   M1 hand-rolled cartridge (no SDK)
```

## Licence

MIT OR Apache-2.0.

[cpal]: https://github.com/RustAudio/cpal

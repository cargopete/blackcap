# blackcap

A WebAssembly jukebox. Cartridges are `.wasm` **component** files that export a
tiny audio interface; the host loads them, calls `render()` on a worker thread,
and plays the result through [cpal] over a wait-free SPSC ring buffer.

> Status: **M2**. The host plays the built-in sine (M0) and loads real wasm
> cartridges (M1). The cartridge SDK (M2) is up: oscillators, envelopes,
> percussion, a de-clicking gate, and a `song!{}` tracker DSL — enough to write
> a three-channel arpeggio or a drop-A breakdown. Host DSP imports
> (oversampled waveshaper, reverb, master limiter), hot-reload/crossfade, and
> the TUI are still ahead (M3–M6).

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

With [`just`](https://github.com/casey/just) (recipes take a cartridge dir name):

```sh
just sine                       # M0: built-in sine through your speakers
just play arpeggio-cartridge    # build + play the three-channel arpeggio
just play breakdown-cartridge   # build + play the drop-A breakdown
just dry-run breakdown-cartridge  # headless: render blocks, print peak/RMS
just inspect arpeggio-cartridge   # show the WIT the cartridge exports
just test                       # SDK unit tests
```

Or by hand:

```sh
cargo build -p blackcap --release            # host

cd examples/arpeggio-cartridge               # a cartridge → wasm component
cargo build --target wasm32-wasip2 --release

cargo run -p blackcap --release -- \
    examples/arpeggio-cartridge/target/wasm32-wasip2/release/arpeggio_cartridge.wasm
```

`Ctrl+C` stops cleanly. `--seconds N` auto-stops. `--dry-run` skips the audio
device entirely and is the way to verify a cartridge on a headless box.

## Writing a cartridge

Implement [`Player`] and hand it to `export_player!`; the SDK owns the
wit-bindgen glue. The `song!{}` macro is a tracker DSL — note lanes (`c5 eb5`),
trigger lanes (`x - x -`) and gate lanes (`X-x- ----`) share one syntax, parsed
once at `init()`:

```rust
use jukebox_cartridge_sdk::prelude::*;

const SONG: TrackerSong = song! {
    tempo: 140; rows_per_beat: 4;
    pattern "a" {
        lead: "a4 c5 e5 a5  e5 c5 a4 c5";
        hat:  "x  -  x  -   x  -  x  -";
    }
    sequence: [a, a];
};
```

See `examples/arpeggio-cartridge` (clean) and `examples/breakdown-cartridge`
(percussion + de-click gate + sidechain). [`Player`]: crates/sdk/src/lib.rs

## Layout

```
wit/jukebox.wit              cartridge contract (types + player + world)
crates/host/                 the jukebox host (cpal + wasmtime + rtrb)
crates/sdk/                  jukebox-cartridge-sdk: osc, env, perc, song! DSL
examples/sine-cartridge/     M1 hand-rolled cartridge (no SDK)
examples/arpeggio-cartridge/ M2 three-channel arpeggio (SDK + song!)
examples/breakdown-cartridge/ M2 drop-A metalcore breakdown (inline DSP)
```

## Licence

MIT OR Apache-2.0.

[cpal]: https://github.com/RustAudio/cpal

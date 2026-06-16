# blackcap

A WebAssembly jukebox. Cartridges are `.wasm` **component** files that export a
tiny audio interface; the host loads them, calls `render()` on a worker thread,
and plays the result through [cpal] over a wait-free SPSC ring buffer.

> Status: **M4**. Host plays the built-in sine (M0), loads wasm cartridges (M1),
> ships a cartridge SDK with a `song!{}` tracker DSL (M2), exposes host DSP
> imports (M3, WIT `@0.2.0`), and now **hot-reloads with crossfade**: render
> workers feed their own rings, a mixer thread equal-power crossfades between
> them and applies a master compressor + brickwall limiter before the output
> ring. Drop a `.wasm` into `~/.jukebox/cartridges` and it fades in. A worked
> ~70 s synth-metal track ships in `examples/synthcore-track` (M5); the ratatui
> TUI (M6) is the last piece.

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

## Hot-reload & crossfade

```sh
# Crossfade demo: play the first, fade to the second after 4s.
blackcap arpeggio-cartridge.wasm breakdown-cartridge.wasm

# Watch ~/.jukebox/cartridges and crossfade to anything dropped in.
blackcap --watch
#   …then in another shell:  cp my_song.wasm ~/.jukebox/cartridges/
```

Each cartridge renders on its own worker thread into its own ring; a mixer
thread equal-power crossfades between them (`--fade-ms`, default 600) and runs a
master compressor + brickwall limiter (`--no-master` to bypass) before the
output ring. A buggy cartridge can be torn down mid-fade without the audio
thread noticing.

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

DSP comes two ways. **Inline** helpers live in `sdk::fx` (oscillators,
envelopes, percussion, filters) — self-contained, no imports. **Host imports**
live in `sdk::dsp`: effect *state* lives in the host (native, upgradeable), the
cartridge holds an opaque handle.

```rust
use jukebox_cartridge_sdk::dsp::{Waveshaper, ShapeKind};

let shaper = Waveshaper::new(48_000, 4);          // ×4 oversampled
shaper.set_shape(ShapeKind::AsymTanh, 4.5, 0.15);
shaper.set_tone(100.0, 8000.0);
let crunch = shaper.process(&chug);               // anti-aliased, no fizz
```

See `examples/arpeggio-cartridge` (clean), `examples/breakdown-cartridge`
(percussion + de-click gate + sidechain, all inline), and
`examples/host-dsp-cartridge` (the same chug crunched + reverbed by host DSP —
~30% smaller wasm). [`Player`]: crates/sdk/src/lib.rs

## Layout

```
wit/jukebox.wit              cartridge contract @0.2.0 (types + dsp + log + player)
crates/host/                 the jukebox host (cpal + wasmtime + rtrb)
  src/host.rs                host DSP resources (svf, freeverb, delay, waveshaper)
  src/{worker,mixer}.rs      render workers + crossfade mixer thread
  src/master.rs              master bus: compressor + brickwall limiter
crates/sdk/                  jukebox-cartridge-sdk
  src/fx.rs                  inline DSP helpers
  src/{osc,env,perc,song}.rs oscillators, envelopes, percussion, song! DSL
examples/sine-cartridge/     M1 hand-rolled cartridge (no SDK)
examples/arpeggio-cartridge/ M2 three-channel arpeggio (SDK + song!)
examples/breakdown-cartridge/ M2 drop-A metalcore breakdown (inline DSP)
examples/host-dsp-cartridge/ M3 chug crunched + reverbed by host DSP imports
examples/synthcore-track/    M5 "Eutectic Point" — a ~70s synth-metal track
```

Hear the worked track: `just play synthcore-track` (drop-A, A phrygian; intro →
verse riff → breakdown → lead chorus → outro, ~70 s). Pure synthesis, so it
lands at Master Boot Record / synthcore rather than djent — that's the medium.

## Licence

MIT OR Apache-2.0.

[cpal]: https://github.com/RustAudio/cpal

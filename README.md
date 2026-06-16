# blackcap

A WebAssembly jukebox. Cartridges are `.wasm` **component** files that export a
tiny audio interface; the host loads them, calls `render()` on a worker thread,
and plays the result through [cpal] over a wait-free SPSC ring buffer.

> Status: **feature-complete** (WIT `@0.3.0`). What's here:
>
> - **Host** — cpal output over a wait-free ring; cartridges run on render
>   worker threads; epoch-interrupt protection against runaway cartridges.
> - **SDK + `song!{}` tracker DSL** — oscillators, envelopes, synthesised
>   percussion, a de-clicking gate; write a cartridge in a few lines.
> - **Host DSP imports** — SVF, freeverb, delay, an oversampled anti-aliased
>   waveshaper; effect state lives host-side, cartridges hold handles.
> - **Hot-reload + crossfade** — a mixer thread equal-power crossfades between
>   workers and runs a master compressor + brickwall limiter. Drop a `.wasm`
>   into `~/.jukebox/cartridges` and it fades in.
> - **Sample playback** — drop `.wav` files into `~/.jukebox/samples`;
>   cartridges pitch-shift them, with multisampling for believable instruments.
>   The path to real recorded (guitar/drum) timbre.
> - **`--tui`** — a ratatui front-end: cartridge list, now-playing, VU, timeline.
>
> Worked tracks ship in `examples/` — see [Tracks](#tracks).

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
just play severance               # the worked synth-metalcore track
just test                       # unit tests (host + SDK)
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

```sh
# Interactive front-end: cartridge list, now-playing, VU, timeline.
blackcap --tui          # ↑/↓ select · ⏎ play/crossfade · r rescan · q quit
```

The TUI watches `~/.jukebox/cartridges`; ⏎ on a list entry crossfades to it.

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
wit/jukebox.wit               cartridge contract @0.3.0 (types/dsp/sampler/log/player)
crates/host/                  the jukebox host (cpal + wasmtime + rtrb)
  src/host.rs                 host DSP resources (svf, freeverb, delay, waveshaper)
  src/sampler.rs              WAV library + interpolating voices + multisample
  src/{worker,mixer}.rs       render workers + crossfade mixer thread
  src/master.rs               master bus: compressor + brickwall limiter
  src/{controller,tui}.rs     orchestration + ratatui front-end
crates/sdk/                   jukebox-cartridge-sdk
  src/fx.rs                   inline DSP helpers
  src/{osc,env,perc,song}.rs  oscillators, envelopes, percussion, song! DSL
examples/sine-cartridge/      hand-rolled cartridge, no SDK (the raw contract)
examples/arpeggio-cartridge/  three-channel arpeggio (SDK + song!)
examples/breakdown-cartridge/ drop-A breakdown, all inline DSP
examples/host-dsp-cartridge/  the same chug via host DSP imports (~30% smaller)
examples/synthcore-track/     "Eutectic Point" — ~70s synth-metal track
examples/severance/           "Severance" — ~83s synth-metalcore track
examples/blackstar/           "Blackstar" — ~71s electronicore (wobble drops)
examples/sampled-guitar/      Karplus-Strong pluck via the host sampler
examples/multisampled-guitar/ multisampled instrument (nearest-zone)
```

## Sample playback (v2)

The host owns sample PCM and the playback voices; cartridges trigger them
pitch-shifted. Samples come from `~/.jukebox/samples/<name>.wav` (decoded and
resampled to the device rate) or from PCM the cartridge hands over:

```rust
use jukebox_cartridge_sdk::sampler::{Sample, SampleVoice};

let sample = Sample::from_library("guitar")          // real DI .wav, or…
    .unwrap_or_else(|| Sample::from_pcm(&my_pcm));    // …cartridge-provided
let voice = SampleVoice::new();
voice.trigger(&sample, target_hz / root_hz, 0.8);     // pitch-shift by speed
let block = voice.render(num_frames);
```

`examples/sampled-guitar` synthesises a plucked string so it works with no
external files; drop a `guitar.wav` into the samples dir and it uses that
instead. This is the only route to literal recorded timbre — pure synthesis
caps out at synthcore.

For a believable instrument across a wide range, use a **multisample** — several
root samples, host picks the nearest zone per note and shifts only the
remainder (a single sample stretched across an octave goes rubbery):

```rust
use jukebox_cartridge_sdk::sampler::{Multisample, Sample, SampleVoice};

let inst = Multisample::new();
inst.add(&Sample::from_library("guitar_a1").unwrap(), 55.0);   // A1
inst.add(&Sample::from_library("guitar_a2").unwrap(), 110.0);  // A2
voice.trigger_pitched(&inst, note_hz, 0.8);                    // nearest zone
```

See `examples/multisampled-guitar` (3 synthesised zones; drop
`guitar_a1/a2/a3.wav` into the samples dir for real ones).

## Tracks

Worked songs, written entirely in the `song!{}` DSL:

```sh
just play blackstar        # electronicore, A phrygian / drop-A, ~71 s
just play severance        # synth-metalcore, D minor / drop-D, ~83 s
just play synthcore-track  # "Eutectic Point" — synth-metal, A phrygian, ~70 s
```

- **Blackstar** — The Browning / "Skybreaker" flavour, the heavy end:
  machine-gun double-bass, chromatic drop-A chugs, hard-driven supersaw lead,
  and half-time **dubstep-wobble** drops (an LFO-swept resonant filter on a
  driven saw). Dry and brutal.
- **Severance** — Erra / early-Asking-Alexandria flavour: ambient supersaw
  intro → palm-mute gallop verse → soaring chorus over Dm–Bb–F–C → half-time
  breakdown → euphoric trance-synth drop → reprise → outro.
- **Eutectic Point** — drop-A synth-metal: intro → verse riff → breakdown →
  lead chorus → outro.

Pure synthesis lands these at Master Boot Record / synthcore, not djent — that's
the medium, not a bug. Drop real DI `.wav` guitars into `~/.jukebox/samples` and
point a cartridge at the sampler for literal guitar timbre.

## Web UI

A browser front-end for *seeing* a track's structure while it plays — a
composition aid. `webui/build.sh` renders every cartridge to a WAV (through the
real host + master chain) and extracts its `song!{}` structure to JSON; the
static page then shows:

- **Jukebox** — the cartridge shelf; click to load.
- **Bones** — the arrangement: the `sequence` as colour-coded sections
  (intro/verse/chorus/drop/breakdown/…), so the song's skeleton is one glance.
  Click to seek.
- **Breakdown** — the tracker grid of the current pattern: every lane × cell,
  notes and hits, with the active column tracking the playhead.
- **Live view** — a playhead sweeps the arrangement and an analyser drives a
  live spectrum while the rendered audio plays.

```sh
./webui/build.sh            # render WAVs + extract structure
python3 webui/serve.py 8080 # Range-capable server (seeking works); open http://localhost:8080
```

Keys: **space** play/pause · click the Bones strip to seek. (The bundled
`serve.py` supports HTTP Range — stock `python -m http.server` doesn't, which
breaks audio seeking.)

Cartridges are wasm *components* importing the whole host, so they don't run
in-browser directly — instead the host renders them to audio (`blackcap --render
out.wav`) and the page plays that, so what you hear matches the jukebox exactly.
Generated WAVs and `tracks.json` are gitignored; re-run `build.sh` to refresh.

## Licence

MIT OR Apache-2.0.

[cpal]: https://github.com/RustAudio/cpal

# blackcap — task runner
# `just` is optional; every recipe is a plain cargo invocation underneath.

host_run := "cargo run -p blackcap --release --"

default:
    @just --list

# Build the host binary.
build-host:
    cargo build -p blackcap --release

# Run the SDK unit tests (native target).
test:
    cd crates/sdk && cargo test

# Build a cartridge in examples/ to a wasm component. e.g. `just build arpeggio-cartridge`
build CART:
    cd examples/{{CART}} && cargo build --target wasm32-wasip2 --release

# The path to a built cartridge's wasm.
_wasm CART:
    @echo "examples/{{CART}}/target/wasm32-wasip2/release/{{replace(CART, "-", "_")}}.wasm"

# Play a cartridge through the speakers. e.g. `just play arpeggio-cartridge`
play CART: (build CART)
    {{host_run}} `just _wasm {{CART}}`

# Headless smoke test: render a cartridge with no audio device.
dry-run CART: (build CART)
    {{host_run}} `just _wasm {{CART}}` --dry-run --blocks 8

# Inspect the WIT a built cartridge exports.
inspect CART: (build CART)
    wasm-tools component wit `just _wasm {{CART}}`

# Play the built-in sine (M0), no cartridge.
sine:
    {{host_run}} --sine

# Crossfade demo: play A, fade to B. e.g. `just crossfade arpeggio-cartridge breakdown-cartridge`
crossfade A B: (build A) (build B)
    {{host_run}} `just _wasm {{A}}` `just _wasm {{B}}`

# Watch ~/.jukebox/cartridges and crossfade to new drops.
watch:
    {{host_run}} --watch

# Drop a built cartridge into the jukebox library dir.
install CART: (build CART)
    mkdir -p ~/.jukebox/cartridges
    cp `just _wasm {{CART}}` ~/.jukebox/cartridges/

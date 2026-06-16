# blackcap — task runner
# `just` is optional; every recipe is a plain cargo invocation underneath.

cartridge_dir := "examples/sine-cartridge"
cartridge_wasm := "examples/sine-cartridge/target/wasm32-wasip2/release/sine_cartridge.wasm"

default:
    @just --list

# Build the host binary.
build-host:
    cargo build -p blackcap --release

# Build the example sine cartridge to a wasm component.
build-cartridge:
    cd {{cartridge_dir}} && cargo build --target wasm32-wasip2 --release

# Build everything.
build: build-host build-cartridge

# Inspect the WIT the built cartridge actually exports.
inspect: build-cartridge
    wasm-tools component wit {{cartridge_wasm}}

# Play the built-in sine (M0).
sine:
    cargo run -p blackcap --release -- --sine

# Play the sine cartridge through the speakers (M1).
play: build-cartridge
    cargo run -p blackcap --release -- {{cartridge_wasm}}

# Headless smoke test: render the cartridge with no audio device.
dry-run: build-cartridge
    cargo run -p blackcap --release -- {{cartridge_wasm}} --dry-run

# Drop the cartridge into the jukebox library dir.
install-cartridge: build-cartridge
    mkdir -p ~/.jukebox/cartridges
    cp {{cartridge_wasm}} ~/.jukebox/cartridges/

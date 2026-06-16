#!/usr/bin/env bash
# Render every cartridge that has a song to a WAV and extract its structure,
# so the web UI has audio + bones to show. Generated files (tracks/*.wav,
# tracks.json) are gitignored; this regenerates them.
set -euo pipefail
cd "$(dirname "$0")/.."

# Cartridges shown in the web jukebox (must match INCLUDE in extract.py).
TRACKS="blackstar sampled-guitar featherz"

echo "==> building host"
cargo build -p blackcap --release

rm -rf webui/tracks && mkdir -p webui/tracks
HOST=./target/release/blackcap

for name in $TRACKS; do
  dir="examples/$name/"
  wasm="${dir}target/wasm32-wasip2/release/$(echo "$name" | tr '-' '_').wasm"
  echo "==> $name"
  ( cd "$dir" && cargo build --target wasm32-wasip2 --release -q )
  "$HOST" --render "webui/tracks/$name.wav" "$wasm"
done

python3 webui/extract.py

cat <<'EOF'

done. serve the UI (Range-capable, so seeking works):
  python3 webui/serve.py 8080
then open http://localhost:8080
EOF

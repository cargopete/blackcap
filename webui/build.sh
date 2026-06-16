#!/usr/bin/env bash
# Render every cartridge that has a song to a WAV and extract its structure,
# so the web UI has audio + bones to show. Generated files (tracks/*.wav,
# tracks.json) are gitignored; this regenerates them.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> building host"
cargo build -p blackcap --release

mkdir -p webui/tracks
HOST=./target/release/blackcap

for dir in examples/*/; do
  name=$(basename "$dir")
  [ -f "${dir}src/lib.rs" ] || continue
  grep -q "TrackerSong = song" "${dir}src/lib.rs" || continue
  wasm="${dir}target/wasm32-wasip2/release/$(echo "$name" | tr '-' '_').wasm"
  echo "==> $name"
  ( cd "$dir" && cargo build --target wasm32-wasip2 --release -q )
  "$HOST" --render "webui/tracks/$name.wav" "$wasm"
done

python3 webui/extract.py

cat <<'EOF'

done. serve the UI:
  (cd webui && python3 -m http.server 8080)
then open http://localhost:8080
EOF

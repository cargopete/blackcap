#!/usr/bin/env python3
"""Extract song!{} structure from cartridge sources into webui/tracks.json.

The song! DSL is regular enough to parse without a full Rust parser: we find the
`song! { ... }` block, then pull tempo / rows_per_beat / patterns / sequence and
the metadata title. The web UI does cell tokenisation + frame maths itself.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXAMPLES = ROOT / "examples"
OUT = Path(__file__).resolve().parent / "tracks.json"


def matching_block(src: str, open_idx: int) -> str:
    """Given the index of a '{', return the text up to its matching '}'."""
    depth = 0
    for i in range(open_idx, len(src)):
        c = src[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return src[open_idx + 1 : i]
    return src[open_idx + 1 :]


def parse_song(src: str):
    # Pick the song! block that actually holds a tempo (skip doc-comment
    # mentions like `song!{}`).
    block = None
    for m in re.finditer(r"song!\s*\{", src):
        candidate = matching_block(src, m.end() - 1)
        if "tempo:" in candidate:
            block = candidate
            break
    if block is None:
        return None

    tempo = int(re.search(r"tempo:\s*(\d+)", block).group(1))
    rpb = int(re.search(r"rows_per_beat:\s*(\d+)", block).group(1))

    patterns = []
    for pm in re.finditer(r'pattern\s+"([^"]+)"\s*\{', block):
        name = pm.group(1)
        body = matching_block(block, pm.end() - 1)
        lanes = [
            {"name": ln, "cells": cells}
            for ln, cells in re.findall(r'(\w+)\s*:\s*"([^"]*)"', body)
        ]
        patterns.append({"name": name, "lanes": lanes})

    seq_m = re.search(r"sequence:\s*\[(.*?)\]", block, re.DOTALL)
    sequence = [s.strip() for s in seq_m.group(1).split(",") if s.strip()]

    return {"tempo": tempo, "rows_per_beat": rpb, "patterns": patterns, "sequence": sequence}


def main():
    tracks = []
    for ex in sorted(EXAMPLES.iterdir()):
        lib = ex / "src" / "lib.rs"
        if not lib.is_file():
            continue
        src = lib.read_text()
        song = parse_song(src)
        if not song:
            continue

        tm = re.search(r'title:\s*"([^"]*)"', src)
        title = tm.group(1) if tm else ex.name
        tags = re.findall(r'"([^"]+)"\.to_string\(\)', re.search(r"tags:\s*vec!\[(.*?)\]", src, re.DOTALL).group(1)) if re.search(r"tags:\s*vec!\[", src) else []

        wav = f"tracks/{ex.name}.wav"
        has_wav = (Path(__file__).resolve().parent / wav).is_file()

        tracks.append({
            "id": ex.name,
            "title": title,
            "tags": tags,
            "wav": wav,
            "hasWav": has_wav,
            **song,
        })
        print(f"  {ex.name}: \"{title}\" — {len(song['patterns'])} patterns, {len(song['sequence'])} in sequence{' [wav]' if has_wav else ''}")

    OUT.write_text(json.dumps({"sampleRate": 48000, "tracks": tracks}, indent=2))
    print(f"wrote {OUT.relative_to(ROOT)} ({len(tracks)} tracks)")


if __name__ == "__main__":
    sys.exit(main())

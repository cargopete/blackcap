#!/usr/bin/env bash
# Build a YouTube-ready MP4 (1080p, H.264 + AAC) from a cover image + a rendered
# track. With no image it makes a dark title card instead.
#
#   ./make-video.sh                 # title-card video of featherz
#   ./make-video.sh crow.png        # featherz with your cover art
#   AUDIO=webui/tracks/blackstar.wav OUT=media/blackstar.mp4 TITLE=blackstar ./make-video.sh cover.png
set -euo pipefail
cd "$(dirname "$0")"

AUDIO="${AUDIO:-webui/tracks/featherz.wav}"
OUT="${OUT:-media/featherz.mp4}"
TITLE="${TITLE:-featherz}"
COVER="${1:-}"
FONT="/System/Library/Fonts/Supplemental/DIN Condensed Bold.ttf"

mkdir -p "$(dirname "$OUT")"
[ -f "$AUDIO" ] || { echo "no audio at $AUDIO — run webui/build.sh first"; exit 1; }

# Exact track length, so the video doesn't run on past the audio.
DUR="$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$AUDIO")"

if [ -n "$COVER" ]; then
  # Sharp cover centred over a blurred, darkened zoom of itself (fills 16:9).
  ffmpeg -y -loop 1 -framerate 12 -i "$COVER" -i "$AUDIO" -filter_complex "\
    [0:v]scale=1920:1080:force_original_aspect_ratio=increase,crop=1920:1080,gblur=sigma=42,eq=brightness=-0.4[bg];\
    [0:v]scale=-2:1040[fg];[bg][fg]overlay=(W-w)/2:(H-h)/2,format=yuv420p[v]" \
    -map "[v]" -map 1:a -t "$DUR" -c:v libx264 -preset medium -tune stillimage -pix_fmt yuv420p -r 12 \
    -c:a aac -b:a 320k -movflags +faststart "$OUT"
else
  # Dark title card (no cover supplied).
  ffmpeg -y -f lavfi -i "color=c=0x07030f:s=1920x1080:r=12:d=900" -i "$AUDIO" -filter_complex "\
    [0:v]drawtext=fontfile='$FONT':text='$TITLE':fontcolor=white:fontsize=250:x=(w-text_w)/2:y=(h-text_h)/2-40:bordercolor=0x8c0020:borderw=3:shadowcolor=0xff2244@0.5:shadowx=0:shadowy=0,\
    drawtext=fontfile='$FONT':text='aggressive / haunting':fontcolor=0x9aa0c0:fontsize=54:x=(w-text_w)/2:y=h/2+160,\
    drawtext=fontfile='$FONT':text='blackcap':fontcolor=0x6b6494:fontsize=36:x=(w-text_w)/2:y=h-90,\
    vignette=PI/4,format=yuv420p[v]" \
    -map "[v]" -map 1:a -t "$DUR" -c:v libx264 -preset medium -tune stillimage -pix_fmt yuv420p -r 12 \
    -c:a aac -b:a 320k -movflags +faststart "$OUT"
fi

echo "wrote $OUT"

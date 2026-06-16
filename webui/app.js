"use strict";

let SR = 48000;
let track = null;       // selected track
let timeline = [];      // [{name, pattern, startSec, durSec, rows, secPerRow}]
let durationSec = 0;    // total, from song maths
let trackerPattern = null;
let trackerCols = [];   // column index -> [cell elements]
let activeCol = -1;

const audio = new Audio();
audio.preload = "auto";
let actx = null, analyser = null, freqData = null;

const $ = (id) => document.getElementById(id);

// --- song! cell tokeniser (mirrors the SDK's parse_lane) -------------------
function tokenize(cells) {
  const out = [];
  for (const tok of cells.trim().split(/\s+/).filter(Boolean)) {
    if (/[0-9]/.test(tok)) {
      out.push({ kind: "note", text: tok });
    } else {
      for (const ch of tok) {
        if (ch === "x") out.push({ kind: "hit", text: "▆" });
        else if (ch === "X") out.push({ kind: "accent", text: "█" });
        else if (ch === ".") out.push({ kind: "ghost", text: "·" });
        else if (ch === "-") out.push({ kind: "off", text: "·" });
      }
    }
  }
  return out;
}

function patternRows(p) {
  return Math.max(1, ...p.lanes.map((l) => tokenize(l.cells).length));
}

// --- section colour by pattern name ----------------------------------------
function patternColor(name) {
  const n = name.toLowerCase();
  if (n.includes("intro") || n.includes("build")) return "#1fe0ff"; // cyan
  if (n.includes("drop") || n.includes("trance")) return "#ff2d95"; // magenta
  if (n.includes("break")) return "#ff3b6b"; // hot red
  if (n.includes("chorus") || n.includes("lead")) return "#a857ff"; // purple
  if (n.includes("verse")) return "#ff9d3d"; // orange
  if (n.includes("outro") || n.includes("stop")) return "#6b6494"; // dim
  return "#28c0a8"; // default teal (riffs, a/b, …)
}

const mmss = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

// --- load ------------------------------------------------------------------
async function boot() {
  const data = await (await fetch("tracks.json")).json();
  SR = data.sampleRate || 48000;
  const list = $("cartridge-list");
  data.tracks.forEach((t, i) => {
    const li = document.createElement("li");
    li.style.setProperty("--i", i);
    li.innerHTML = `<div class="t">${t.title}</div>
      <div class="meta">${t.id} · ${t.tags.join(" · ") || "·"}</div>
      ${t.hasWav ? "" : '<div class="nowav">structure only · run build.sh for audio</div>'}`;
    li.onclick = () => selectTrack(t, li);
    list.appendChild(li);
  });
  // auto-select the first track with audio, else the first
  const first = data.tracks.find((t) => t.hasWav) || data.tracks[0];
  if (first) {
    const li = [...list.children][data.tracks.indexOf(first)];
    selectTrack(first, li);
  }
}

function selectTrack(t, li) {
  track = t;
  [...$("cartridge-list").children].forEach((el) => el.classList.remove("active"));
  if (li) li.classList.add("active");

  // Build a time-based timeline: each pattern may run at its own tempo, so a
  // slower breakdown takes more seconds (and shows wider in the bones).
  const byName = Object.fromEntries(t.patterns.map((p) => [p.name, p]));
  timeline = [];
  let sec = 0;
  for (const name of t.sequence) {
    const p = byName[name];
    if (!p) continue;
    const rows = patternRows(p);
    const tempo = p.tempo || t.tempo;
    const secPerRow = 60 / (tempo * t.rows_per_beat);
    const durSec = rows * secPerRow;
    timeline.push({ name, pattern: p, startSec: sec, durSec, rows, secPerRow });
    sec += durSec;
  }
  durationSec = sec;

  $("np-title").textContent = t.title;
  $("np-tags").textContent = t.tags.join("  ·  ");
  $("clock").textContent = `0:00 / ${mmss(durationSec)}`;

  renderArrangement();
  renderLegend();
  trackerPattern = null;
  renderFromFrac(0);

  if (t.hasWav) {
    audio.src = t.wav;
    $("playpause").disabled = false;
  } else {
    audio.removeAttribute("src");
    $("playpause").disabled = true;
  }
  $("playpause").textContent = "▶";
}

function renderArrangement() {
  const arr = $("arrangement");
  arr.innerHTML = "";
  for (const seg of timeline) {
    const div = document.createElement("div");
    div.className = "seg";
    div.style.flex = `${seg.durSec} 0 0`;
    div.style.setProperty("--c", patternColor(seg.name));
    const label = document.createElement("span");
    label.textContent = seg.name;
    div.appendChild(label);
    arr.appendChild(div);
  }
  const ph = document.createElement("div");
  ph.id = "playhead";
  arr.appendChild(ph);
  arr.onclick = (e) => {
    const frac = (e.clientX - arr.getBoundingClientRect().left) / arr.clientWidth;
    seekToFrac(Math.max(0, Math.min(0.999, frac)));
  };
}

function renderLegend() {
  const seen = new Map();
  for (const seg of timeline) if (!seen.has(seg.name)) seen.set(seg.name, patternColor(seg.name));
  $("legend").innerHTML = [...seen].map(([n, c]) => `<span style="--c:${c}">${n}</span>`).join("");
}

function renderTracker(pattern, col) {
  $("pattern-name").textContent = pattern.name;
  const root = $("tracker");
  root.innerHTML = "";
  trackerCols = [];
  for (const lane of pattern.lanes) {
    const cells = tokenize(lane.cells);
    const laneEl = document.createElement("div");
    laneEl.className = "lane";
    const name = document.createElement("div");
    name.className = "name";
    name.textContent = lane.name;
    laneEl.appendChild(name);
    const cellsEl = document.createElement("div");
    cellsEl.className = "cells";
    cells.forEach((c, i) => {
      const el = document.createElement("div");
      el.className = `cell ${c.kind}` + (i % track.rows_per_beat === 0 ? " beat" : "");
      el.textContent = c.text;
      cellsEl.appendChild(el);
      (trackerCols[i] ||= []).push(el);
    });
    laneEl.appendChild(cellsEl);
    root.appendChild(laneEl);
  }
  activeCol = -1;
  highlightCol(col);
}

function highlightCol(col) {
  if (col === activeCol) return;
  if (trackerCols[activeCol]) trackerCols[activeCol].forEach((e) => e.classList.remove("active"));
  if (trackerCols[col]) trackerCols[col].forEach((e) => e.classList.add("active"));
  activeCol = col;
}

// Place the playhead + tracker according to a fraction (0..1) of the song.
function renderFromFrac(frac) {
  const sec = frac * durationSec;
  let seg = timeline.find((s) => sec < s.startSec + s.durSec) || timeline[timeline.length - 1];
  if (!seg) return;
  if (seg.pattern !== trackerPattern) {
    trackerPattern = seg.pattern;
    renderTracker(seg.pattern, -1);
  }
  highlightCol(Math.floor((sec - seg.startSec) / seg.secPerRow));
  $("playhead").style.left = `${frac * 100}%`;
}

// Prefer the real rendered length; fall back to the song-maths estimate.
function audioDur() {
  return audio.duration && isFinite(audio.duration) ? audio.duration : durationSec;
}

function seekToFrac(frac) {
  if (track.hasWav && audio.src) {
    audio.currentTime = frac * audioDur();
    $("clock").textContent = `${mmss(frac * audioDur())} / ${mmss(audioDur())}`;
  }
  renderFromFrac(frac);
}

// --- audio + live loop -----------------------------------------------------
function ensureAudioGraph() {
  if (actx) return;
  actx = new (window.AudioContext || window.webkitAudioContext)();
  const src = actx.createMediaElementSource(audio);
  analyser = actx.createAnalyser();
  analyser.fftSize = 256;
  freqData = new Uint8Array(analyser.frequencyBinCount);
  src.connect(analyser);
  analyser.connect(actx.destination);
}

$("playpause").onclick = () => {
  if (!track || !track.hasWav) return;
  ensureAudioGraph();
  if (actx.state === "suspended") actx.resume();
  if (audio.paused) audio.play();
  else audio.pause();
};
audio.onplay = () => {
  $("playpause").textContent = "⏸";
  $("playpause").classList.add("playing");
};
const stopUi = () => {
  $("playpause").textContent = "▶";
  $("playpause").classList.remove("playing");
};
audio.onpause = stopUi;
audio.onended = stopUi;

function frame() {
  if (track) {
    if (!audio.paused) {
      const d = audioDur();
      renderFromFrac(audio.currentTime / d);
      $("clock").textContent = `${mmss(audio.currentTime)} / ${mmss(d)}`;
    }
    drawScope();
  }
  requestAnimationFrame(frame);
}

function drawScope() {
  const cv = $("scope");
  const w = cv.clientWidth;
  if (cv.width !== w) cv.width = w;
  const h = cv.height;
  const ctx = cv.getContext("2d");
  ctx.clearRect(0, 0, w, h);
  if (!analyser || audio.paused) {
    ctx.fillStyle = "#2a3142";
    ctx.fillRect(0, h - 1, w, 1);
    return;
  }
  analyser.getByteFrequencyData(freqData);
  const n = freqData.length;
  const bw = w / n;
  for (let i = 0; i < n; i++) {
    const v = freqData[i] / 255;
    const bh = v * h;
    ctx.fillStyle = v > 0.85 ? "#ff7b72" : v > 0.6 ? "#ffd479" : "#38d6c4";
    ctx.fillRect(i * bw, h - bh, bw - 1, bh);
  }
}

// Keyboard: space = play/pause, arrows = seek ±5 s.
document.addEventListener("keydown", (e) => {
  if (e.code === "Space" || e.key === " ") {
    e.preventDefault();
    const btn = $("playpause");
    if (!btn.disabled) btn.click();
  } else if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && track && track.hasWav) {
    e.preventDefault();
    const d = audioDur();
    const t = audio.currentTime + (e.key === "ArrowRight" ? 5 : -5);
    seekToFrac(Math.max(0, Math.min(0.999, t / d)));
  }
});

boot();
requestAnimationFrame(frame);

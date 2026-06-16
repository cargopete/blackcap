"use strict";

let SR = 48000;
let track = null;       // selected track
let timeline = [];      // [{name, pattern, startSec, durSec, rows, secPerRow}] (per sequence entry)
let sections = [];      // [{name, startSec, durSec}] — consecutive same-pattern runs merged
let durationSec = 0;    // total, from song maths
let trackerPattern = null;
let trackerCols = [];   // column index -> [cell elements]
let activeCol = -1;

const audio = new Audio();
audio.preload = "auto";
let actx = null, analyser = null, freqData = null;

const $ = (id) => document.getElementById(id);

// Note name -> MIDI number (mirrors the SDK's Note::parse). a4 = 69.
function parseNoteMidi(s) {
  const m = /^([a-gA-G])([#sb]?)(-?\d+)$/.exec(s.trim());
  if (!m) return null;
  let semi = { c: 0, d: 2, e: 4, f: 5, g: 7, a: 9, b: 11 }[m[1].toLowerCase()];
  if (m[2] === "#" || m[2] === "s") semi += 1;
  else if (m[2] === "b") semi -= 1;
  return (parseInt(m[3], 10) + 1) * 12 + semi;
}

// Friendly instrument names + colours, so you never need to read a note name.
const LANE_INFO = {
  lead: { label: "lead", color: "#1fe0ff" },
  chug_note: { label: "guitar chug", color: "#ff2d95" },
  wobble_note: { label: "wobble bass", color: "#a857ff" },
  bass: { label: "bass", color: "#46e08a" },
  pad: { label: "pad", color: "#7aa2ff" },
  gate: { label: "chug rhythm", color: "#ff6fae" },
  kick: { label: "kick", color: "#ff9d3d" },
  snare: { label: "snare", color: "#ff5277" },
  hat: { label: "hi-hat", color: "#ffd479" },
  crash: { label: "cymbal", color: "#ffe08a" },
};
const LANE_ORDER = ["lead", "chug_note", "wobble_note", "bass", "pad", "gate", "kick", "snare", "hat", "crash"];
function laneInfo(name) {
  return LANE_INFO[name] || { label: name.replace(/_/g, " "), color: "#28c0a8" };
}
function hexA(hex, a) {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${a})`;
}

// --- song! cell tokeniser (mirrors the SDK's parse_lane) -------------------
function tokenize(cells) {
  const out = [];
  for (const tok of cells.trim().split(/\s+/).filter(Boolean)) {
    if (/[0-9]/.test(tok)) {
      out.push({ kind: "note", text: tok, midi: parseNoteMidi(tok) });
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

  // Merge runs of the same pattern for a readable arrangement (intro intro -> intro).
  sections = [];
  for (const seg of timeline) {
    const last = sections[sections.length - 1];
    if (last && last.name === seg.name) last.durSec += seg.durSec;
    else sections.push({ name: seg.name, startSec: seg.startSec, durSec: seg.durSec });
  }

  $("np-title").textContent = t.title;
  $("np-tags").textContent = t.tags.join("  ·  ");
  $("clock").textContent = `0:00 / ${mmss(durationSec)}`;

  renderArrangement();
  renderLegend();
  renderDissection();
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
  for (const seg of sections) {
    const div = document.createElement("div");
    div.className = "seg";
    div.style.flex = `${seg.durSec} 0 0`;
    div.style.setProperty("--c", patternColor(seg.name));
    // Only label blocks wide enough to fit the text, so labels never collide.
    if (seg.durSec / durationSec > 0.05) {
      const label = document.createElement("span");
      label.textContent = seg.name;
      div.appendChild(label);
    }
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

const DISSECT_GUTTER = 104;
const DISSECT_HEAD = 22;
const DISSECT_LANE_H = 30;

// Full-track activity map: one row per instrument; melodic rows draw pitch as
// height (no note-reading needed), rhythm rows draw hits as ticks.
function renderDissection() {
  if (!track) return;
  const byName = Object.fromEntries(track.patterns.map((p) => [p.name, p]));

  // Union of lane names, in a friendly order.
  const present = new Set();
  for (const p of track.patterns) for (const l of p.lanes) present.add(l.name);
  const lanes = [...present].sort((a, b) => {
    const ia = LANE_ORDER.indexOf(a), ib = LANE_ORDER.indexOf(b);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib) || a.localeCompare(b);
  });

  // Per-lane: is it melodic, and what's its pitch range across the whole song?
  const meta = {};
  for (const lane of lanes) {
    let lo = Infinity, hi = -Infinity, pitched = false;
    for (const p of track.patterns) {
      const lo2 = p.lanes.find((l) => l.name === lane);
      if (!lo2) continue;
      for (const c of tokenize(lo2.cells))
        if (c.kind === "note" && c.midi != null) { pitched = true; lo = Math.min(lo, c.midi); hi = Math.max(hi, c.midi); }
    }
    meta[lane] = { pitched, lo, hi };
  }

  const cv = $("dissect");
  const cssW = cv.parentElement.clientWidth || 800;
  const h = DISSECT_HEAD + lanes.length * DISSECT_LANE_H;
  const dpr = window.devicePixelRatio || 1;
  cv.style.height = h + "px";
  cv.width = cssW * dpr;
  cv.height = h * dpr;
  const ctx = cv.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, cssW, h);

  const plotW = cssW - DISSECT_GUTTER;
  const xOf = (sec) => DISSECT_GUTTER + (sec / durationSec) * plotW;

  // Section bands (header) + faint tints down the lanes + boundaries (merged runs).
  for (const seg of sections) {
    const x = xOf(seg.startSec), w = (seg.durSec / durationSec) * plotW;
    ctx.fillStyle = hexA(patternColor(seg.name), 0.09);
    ctx.fillRect(x, DISSECT_HEAD, w, h - DISSECT_HEAD);
    ctx.fillStyle = hexA(patternColor(seg.name), 0.55);
    ctx.fillRect(x, 0, w, DISSECT_HEAD);
    ctx.strokeStyle = "rgba(255,255,255,0.08)";
    ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, h); ctx.stroke();
    if (w > 34) {
      ctx.fillStyle = "rgba(255,255,255,0.92)";
      ctx.font = "600 10px 'Chakra Petch', sans-serif";
      ctx.fillText(seg.name, x + 5, 15);
    }
  }

  ctx.textBaseline = "middle";
  lanes.forEach((lane, li) => {
    const top = DISSECT_HEAD + li * DISSECT_LANE_H;
    const info = laneInfo(lane);
    const m = meta[lane];

    ctx.strokeStyle = "rgba(255,255,255,0.05)";
    ctx.beginPath(); ctx.moveTo(DISSECT_GUTTER, top); ctx.lineTo(cssW, top); ctx.stroke();
    ctx.fillStyle = info.color;
    ctx.font = "12px 'Share Tech Mono', monospace";
    ctx.fillText(info.label, 10, top + DISSECT_LANE_H / 2);

    for (const seg of timeline) {
      const lo2 = byName[seg.name].lanes.find((l) => l.name === lane);
      if (!lo2) continue;
      const cells = tokenize(lo2.cells);
      const segX = xOf(seg.startSec);
      const colW = (seg.secPerRow / durationSec) * plotW;

      if (m.pitched) {
        // Held note bars at pitch height (extends over sustain/ghost cells).
        for (let i = 0; i < cells.length; i++) {
          if (cells[i].kind !== "note" || cells[i].midi == null) continue;
          let j = i + 1;
          while (j < cells.length && cells[j].kind !== "note" && cells[j].kind !== "off") j++;
          const span = m.hi > m.lo ? (cells[i].midi - m.lo) / (m.hi - m.lo) : 0.5;
          const y = top + DISSECT_LANE_H - 4 - span * (DISSECT_LANE_H - 8);
          ctx.fillStyle = info.color;
          ctx.fillRect(segX + i * colW, y, Math.max((j - i) * colW - 1, 1.5), 3);
        }
      } else {
        // Rhythm ticks: accent tall/bright, hit medium, ghost faint.
        for (let i = 0; i < cells.length; i++) {
          const k = cells[i].kind;
          if (k !== "hit" && k !== "accent" && k !== "ghost") continue;
          const intensity = k === "accent" ? 1 : k === "hit" ? 0.66 : 0.3;
          const th = (DISSECT_LANE_H - 8) * intensity;
          ctx.fillStyle = hexA(info.color, 0.4 + 0.6 * intensity);
          ctx.fillRect(segX + i * colW, top + DISSECT_LANE_H - 3 - th, Math.max(colW * 0.6, 1.5), th);
        }
      }
    }
  });

  positionDissectPlayhead(lastFrac);
}

let lastFrac = 0;
function positionDissectPlayhead(frac) {
  const cv = $("dissect");
  const w = cv.clientWidth || 0;
  $("dissect-playhead").style.left = `${DISSECT_GUTTER + frac * (w - DISSECT_GUTTER)}px`;
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
  lastFrac = frac;
  positionDissectPlayhead(frac);
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

// Click the dissection map to seek (its plot starts after the label gutter).
$("dissect-wrap").onclick = (e) => {
  if (!track) return;
  const r = $("dissect").getBoundingClientRect();
  const frac = (e.clientX - r.left - DISSECT_GUTTER) / (r.width - DISSECT_GUTTER);
  seekToFrac(Math.max(0, Math.min(0.999, frac)));
};

// Redraw the canvas map when the window resizes.
window.addEventListener("resize", () => {
  if (track) renderDissection();
});

boot();
requestAnimationFrame(frame);

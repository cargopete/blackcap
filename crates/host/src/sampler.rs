//! Host-side sample playback. The host owns sample PCM (mono, at the device
//! rate) and the playback voices; cartridges trigger them. Samples come from the
//! library (`~/.jukebox/samples/<name>.wav`) or from cartridge-provided PCM.

use std::path::PathBuf;
use std::sync::Arc;

use wasmtime::component::Resource;

use crate::host::HostState;
use crate::wit::jukebox::cartridge::sampler;

/// A loaded sample: mono f32 PCM at the device sample rate.
pub struct SampleNode {
    data: Arc<[f32]>,
}

/// A pitched instrument: zones of (root pitch, PCM). Playing a target pitch
/// picks the nearest root (in cents) and shifts only the remainder.
pub struct MultisampleNode {
    zones: Vec<(f32, Arc<[f32]>)>,
}

impl MultisampleNode {
    fn new() -> Self {
        Self { zones: Vec::new() }
    }

    /// Nearest zone to `target_hz`; returns (pcm, playback speed).
    fn nearest(&self, target_hz: f32) -> Option<(Arc<[f32]>, f64)> {
        self.zones
            .iter()
            .filter(|(root, _)| *root > 0.0)
            .min_by(|(ra, _), (rb, _)| {
                let da = (target_hz / ra).ln().abs();
                let db = (target_hz / rb).ln().abs();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(root, data)| (data.clone(), (target_hz / root) as f64))
    }
}

/// A single-sample playback voice with fractional (linear-interpolated)
/// read position, so a sample can be pitch-shifted by varying `speed`.
pub struct SampleVoiceNode {
    sample: Option<Arc<[f32]>>,
    pos: f64,
    speed: f64,
    gain: f32,
    active: bool,
    looping: bool,
    loop_start: f64,
    loop_end: f64,
}

impl SampleVoiceNode {
    fn new() -> Self {
        Self {
            sample: None,
            pos: 0.0,
            speed: 1.0,
            gain: 1.0,
            active: false,
            looping: false,
            loop_start: 0.0,
            loop_end: 0.0,
        }
    }

    fn arm(&mut self, data: Arc<[f32]>, speed: f32, gain: f32) {
        let len = data.len();
        self.sample = Some(data);
        self.pos = 0.0;
        self.speed = speed.max(0.0) as f64;
        self.gain = gain;
        self.active = true;
        if !self.looping {
            self.loop_end = len as f64;
        }
    }

    fn render(&mut self, num_frames: u32) -> Vec<f32> {
        let mut out = vec![0.0f32; num_frames as usize];
        let data = match (self.active, &self.sample) {
            (true, Some(d)) => d.clone(),
            _ => return out,
        };
        let len = data.len();
        if len == 0 {
            self.active = false;
            return out;
        }

        for o in out.iter_mut() {
            if self.pos >= len as f64 {
                self.active = false;
                break;
            }
            let i = self.pos as usize;
            let frac = (self.pos - i as f64) as f32;
            let a = data[i];
            let b = if i + 1 < len { data[i + 1] } else { a };
            *o = (a + (b - a) * frac) * self.gain;

            self.pos += self.speed;
            if self.looping && self.pos >= self.loop_end {
                self.pos = self.loop_start + (self.pos - self.loop_end);
            }
        }
        out
    }
}

fn samples_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".jukebox").join("samples")
}

/// Decode `<name>.wav` to mono f32 and resample to `device_sr`.
fn load_library_sample(name: &str, device_sr: u32) -> Option<Arc<[f32]>> {
    let path = samples_dir().join(format!("{name}.wav"));
    let reader = hound::WavReader::open(&path).ok()?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;

    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            reader.into_samples::<f32>().filter_map(Result::ok).collect()
        }
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .into_samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 * scale)
                .collect()
        }
    };

    // Downmix to mono.
    let mono: Vec<f32> = if channels <= 1 {
        raw
    } else {
        raw.chunks(channels)
            .map(|c| c.iter().sum::<f32>() / channels as f32)
            .collect()
    };

    let resampled = if spec.sample_rate == device_sr {
        mono
    } else {
        linear_resample(&mono, spec.sample_rate, device_sr)
    };
    Some(Arc::from(resampled))
}

fn linear_resample(input: &[f32], src_sr: u32, dst_sr: u32) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }
    let ratio = dst_sr as f64 / src_sr as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let j = src as usize;
        let frac = (src - j as f64) as f32;
        let a = input[j.min(input.len() - 1)];
        let b = input[(j + 1).min(input.len() - 1)];
        out.push(a + (b - a) * frac);
    }
    out
}

impl sampler::Host for HostState {}

impl sampler::HostSample for HostState {
    fn from_library(&mut self, name: String) -> Option<Resource<SampleNode>> {
        let data = load_library_sample(&name, self.sample_rate)?;
        self.table.push(SampleNode { data }).ok()
    }

    fn from_pcm(&mut self, pcm: Vec<f32>) -> Resource<SampleNode> {
        self.table
            .push(SampleNode { data: Arc::from(pcm) })
            .expect("resource table push")
    }

    fn frame_count(&mut self, self_: Resource<SampleNode>) -> u64 {
        self.table.get(&self_).map(|s| s.data.len() as u64).unwrap_or(0)
    }

    fn drop(&mut self, rep: Resource<SampleNode>) -> wasmtime::Result<()> {
        self.table.delete(rep)?;
        Ok(())
    }
}

impl sampler::HostMultisample for HostState {
    fn new(&mut self) -> Resource<MultisampleNode> {
        self.table.push(MultisampleNode::new()).expect("resource table push")
    }

    fn add(&mut self, self_: Resource<MultisampleNode>, sample: Resource<SampleNode>, root_hz: f32) {
        let data = self.table.get(&sample).ok().map(|s| s.data.clone());
        if let (Some(data), Ok(ms)) = (data, self.table.get_mut(&self_)) {
            ms.zones.push((root_hz, data));
        }
    }

    fn zone_count(&mut self, self_: Resource<MultisampleNode>) -> u32 {
        self.table.get(&self_).map(|m| m.zones.len() as u32).unwrap_or(0)
    }

    fn drop(&mut self, rep: Resource<MultisampleNode>) -> wasmtime::Result<()> {
        self.table.delete(rep)?;
        Ok(())
    }
}

impl sampler::HostSampleVoice for HostState {
    fn new(&mut self) -> Resource<SampleVoiceNode> {
        self.table.push(SampleVoiceNode::new()).expect("resource table push")
    }

    fn trigger(&mut self, self_: Resource<SampleVoiceNode>, sample: Resource<SampleNode>, speed: f32, gain: f32) {
        // Borrowed sample: read its data (don't delete), then arm the voice.
        let data = self.table.get(&sample).ok().map(|s| s.data.clone());
        if let (Some(data), Ok(voice)) = (data, self.table.get_mut(&self_)) {
            voice.arm(data, speed, gain);
        }
    }

    fn trigger_pitched(&mut self, self_: Resource<SampleVoiceNode>, ms: Resource<MultisampleNode>, target_hz: f32, gain: f32) {
        let picked = self.table.get(&ms).ok().and_then(|m| m.nearest(target_hz));
        if let (Some((data, speed)), Ok(voice)) = (picked, self.table.get_mut(&self_)) {
            voice.arm(data, speed as f32, gain);
        }
    }

    fn set_loop(&mut self, self_: Resource<SampleVoiceNode>, enabled: bool, start_frame: u64, end_frame: u64) {
        if let Ok(voice) = self.table.get_mut(&self_) {
            voice.looping = enabled;
            voice.loop_start = start_frame as f64;
            voice.loop_end = end_frame as f64;
        }
    }

    fn render(&mut self, self_: Resource<SampleVoiceNode>, num_frames: u32) -> Vec<f32> {
        match self.table.get_mut(&self_) {
            Ok(voice) => voice.render(num_frames),
            Err(_) => vec![0.0; num_frames as usize],
        }
    }

    fn is_active(&mut self, self_: Resource<SampleVoiceNode>) -> bool {
        self.table.get(&self_).map(|v| v.active).unwrap_or(false)
    }

    fn stop(&mut self, self_: Resource<SampleVoiceNode>) {
        if let Ok(voice) = self.table.get_mut(&self_) {
            voice.active = false;
        }
    }

    fn drop(&mut self, rep: Resource<SampleVoiceNode>) -> wasmtime::Result<()> {
        self.table.delete(rep)?;
        Ok(())
    }
}

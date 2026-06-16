//! The mixer thread. Sits between the render workers and the cpal output ring:
//! pulls stereo frames from the current source (and an incoming one during a
//! crossfade), equal-power blends them, runs the master chain, and pushes to
//! the output ring. A buggy cartridge can be torn down mid-fade without the
//! audio thread ever noticing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::Receiver;
use rtrb::{Consumer, Producer};

use crate::master::MasterChain;
use crate::shared::Shared;

/// A request to the mixer to bring in a new source.
pub struct Switch {
    pub consumer: Consumer<f32>,
    /// Crossfade length in frames. 0 = hard switch (used for the first source).
    pub fade_frames: usize,
}

pub struct Mixer {
    output: Producer<f32>,
    cmd_rx: Receiver<Switch>,
    running: Arc<AtomicBool>,
    block_frames: usize,
    master: MasterChain,
    shared: Arc<Shared>,
    vu: f32,

    current: Option<Consumer<f32>>,
    incoming: Option<Consumer<f32>>,
    fade_pos: usize,
    fade_len: usize,
}

impl Mixer {
    pub fn new(
        output: Producer<f32>,
        cmd_rx: Receiver<Switch>,
        running: Arc<AtomicBool>,
        block_frames: usize,
        master: MasterChain,
        shared: Arc<Shared>,
    ) -> Self {
        Self {
            output,
            cmd_rx,
            running,
            block_frames,
            master,
            shared,
            vu: 0.0,
            current: None,
            incoming: None,
            fade_pos: 0,
            fade_len: 0,
        }
    }

    pub fn run(mut self) {
        let need = self.block_frames * 2;
        while self.running.load(Ordering::Relaxed) {
            self.drain_commands();

            if self.output.slots() < need {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }

            // With no source yet (watch mode before the first drop) next_frame
            // returns silence, so the device never underruns.
            let mut block_peak = 0.0f32;
            for _ in 0..self.block_frames {
                let (l, r) = self.next_frame();
                let (l, r) = self.master.process(l, r);
                block_peak = block_peak.max(l.abs()).max(r.abs());
                let _ = self.output.push(l);
                let _ = self.output.push(r);
            }

            // Smoothed VU (fast attack, slow release) + timeline frame count.
            self.vu = if block_peak > self.vu {
                block_peak
            } else {
                self.vu * 0.8 + block_peak * 0.2
            };
            self.shared.set_vu(self.vu);
            self.shared.add_frames(self.block_frames as u64);
        }
    }

    fn drain_commands(&mut self) {
        while let Ok(switch) = self.cmd_rx.try_recv() {
            if switch.fade_frames == 0 || self.current.is_none() {
                // Hard switch (or first source): become current immediately.
                self.current = Some(switch.consumer);
                self.incoming = None;
                self.fade_len = 0;
            } else {
                self.incoming = Some(switch.consumer);
                self.fade_pos = 0;
                self.fade_len = switch.fade_frames;
            }
        }
    }

    #[inline]
    fn next_frame(&mut self) -> (f32, f32) {
        let (al, ar) = pop_frame(&mut self.current);

        if self.fade_len == 0 || self.incoming.is_none() {
            return (al, ar);
        }

        let (bl, br) = pop_frame(&mut self.incoming);
        let alpha = self.fade_pos as f32 / self.fade_len as f32;
        // Equal-power crossfade: constant perceived loudness through the blend.
        let (ga, gb) = (
            (alpha * std::f32::consts::FRAC_PI_2).cos(),
            (alpha * std::f32::consts::FRAC_PI_2).sin(),
        );
        self.fade_pos += 1;
        if self.fade_pos >= self.fade_len {
            // Fade done: the incoming source is now the only one.
            self.current = self.incoming.take();
            self.fade_len = 0;
        }
        (al * ga + bl * gb, ar * ga + br * gb)
    }
}

/// Pop one stereo frame; an empty (or absent) source reads as silence.
#[inline]
fn pop_frame(src: &mut Option<Consumer<f32>>) -> (f32, f32) {
    match src {
        Some(c) => {
            let l = c.pop().unwrap_or(0.0);
            let r = c.pop().unwrap_or(0.0);
            (l, r)
        }
        None => (0.0, 0.0),
    }
}

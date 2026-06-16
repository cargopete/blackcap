//! jukebox-cartridge-sdk — write blackcap cartridges without ceremony.
//!
//! Implement [`Player`] on your type and hand it to [`export_player!`]; the SDK
//! owns the wit-bindgen glue and forwards the component exports to a single
//! instance of your type, kept alive between `render()` calls.
//!
//! ```ignore
//! use jukebox_cartridge_sdk::prelude::*;
//!
//! struct MySong { /* … */ }
//! impl Player for MySong { /* init / render / metadata … */ }
//! export_player!(MySong);
//! ```

pub mod env;
pub mod fx;
pub mod osc;
pub mod perc;
pub mod song;
pub mod voice;

/// The raw wit-bindgen output. Most authors never touch this directly — use the
/// [`prelude`] and [`Player`]/[`export_player!`] instead — but it's public so
/// the `export_player!` macro can reference the generated `Guest` trait.
pub mod bindings {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "cartridge",
        pub_export_macro: true,
        export_macro_name: "export_world",
    });
}

pub use bindings::exports::jukebox::cartridge::player::Metadata;
pub use bindings::jukebox::cartridge::types::LoopPoint;

/// Host-provided DSP imports: `dsp::Reverb`, `dsp::BiquadSvf`, `dsp::Delay`,
/// `dsp::Waveshaper`, `dsp::ShapeKind`, and the stateless `dsp::osc_*` functions.
/// (The SDK's *inline* DSP helpers live in [`fx`].)
pub use bindings::jukebox::cartridge::dsp;
/// Host sample playback: `sampler::Sample::from_library`/`from_pcm` and
/// `sampler::SampleVoice` for pitched, interpolated one-shot/looped playback.
pub use bindings::jukebox::cartridge::sampler;
/// Host `log` import — `log::log("…")` prints to the host's stderr.
pub use bindings::jukebox::cartridge::log;

/// The trait a cartridge implements. Mirrors the WIT `player` interface, but
/// with `&mut self` ergonomics — the SDK bridges to the stateless component
/// exports for you.
pub trait Player: Sized + 'static {
    /// Called once after instantiation, with the actual device sample rate.
    /// Return `Err` to refuse loading (e.g. sample-rate mismatch).
    fn init(sample_rate: u32) -> Result<Self, String>;

    /// Render `num_frames` of interleaved stereo (length `2 * num_frames`).
    fn render(&mut self, start_frame: u64, num_frames: u32) -> Vec<f32>;

    /// Snap to a frame. Default: a full reset (lazy seek).
    fn seek(&mut self, frame: u64) {
        let _ = frame;
        self.reset();
    }

    /// Reset all internal state. Default: no-op.
    fn reset(&mut self) {}

    /// Whether the host may stop calling `render()`. Default: never finishes.
    fn is_finished(&self) -> bool {
        false
    }

    fn metadata(&self) -> Metadata;
}

/// Wire a [`Player`] implementor up as the cartridge's exported component.
///
/// Expands to a hidden adapter that implements the generated `Guest` trait and
/// forwards every export to a thread-local instance of your type (wasm is
/// single-threaded, so the thread-local is effectively a global singleton).
#[macro_export]
macro_rules! export_player {
    ($ty:ty) => {
        const _: () = {
            thread_local! {
                static __BLACKCAP_PLAYER: ::core::cell::RefCell<::core::option::Option<$ty>> =
                    ::core::cell::RefCell::new(::core::option::Option::None);
            }

            struct __BlackcapAdapter;

            impl $crate::bindings::exports::jukebox::cartridge::player::Guest for __BlackcapAdapter {
                fn init(sample_rate: u32) -> ::core::result::Result<(), ::std::string::String> {
                    match <$ty as $crate::Player>::init(sample_rate) {
                        ::core::result::Result::Ok(player) => {
                            __BLACKCAP_PLAYER.with(|cell| *cell.borrow_mut() = Some(player));
                            ::core::result::Result::Ok(())
                        }
                        ::core::result::Result::Err(e) => ::core::result::Result::Err(e),
                    }
                }

                fn render(start_frame: u64, num_frames: u32) -> ::std::vec::Vec<f32> {
                    __BLACKCAP_PLAYER.with(|cell| {
                        let mut slot = cell.borrow_mut();
                        let player = slot.as_mut().expect("render() before init()");
                        $crate::Player::render(player, start_frame, num_frames)
                    })
                }

                fn seek(frame: u64) {
                    __BLACKCAP_PLAYER.with(|cell| {
                        if let Some(player) = cell.borrow_mut().as_mut() {
                            $crate::Player::seek(player, frame);
                        }
                    });
                }

                fn reset() {
                    __BLACKCAP_PLAYER.with(|cell| {
                        if let Some(player) = cell.borrow_mut().as_mut() {
                            $crate::Player::reset(player);
                        }
                    });
                }

                fn is_finished() -> bool {
                    __BLACKCAP_PLAYER.with(|cell| {
                        cell.borrow()
                            .as_ref()
                            .map_or(false, |player| $crate::Player::is_finished(player))
                    })
                }

                fn get_metadata() -> $crate::Metadata {
                    __BLACKCAP_PLAYER.with(|cell| {
                        let slot = cell.borrow();
                        let player = slot.as_ref().expect("get_metadata() before init()");
                        $crate::Player::metadata(player)
                    })
                }
            }

            $crate::bindings::export_world!(__BlackcapAdapter with_types_in $crate::bindings);
        };
    };
}

/// Everything a typical cartridge needs in one glob.
pub mod prelude {
    pub use crate::fx::{interleave, soft_clip, OnePoleHp, OnePoleLp, Svf};
    pub use crate::{dsp, log, sampler};
    pub use crate::env::{Adsr, Gate};
    pub use crate::osc::{Noise, Osc, SuperSaw, Waveform};
    pub use crate::perc::{CymbalVoice, KickVoice, SnareVoice};
    pub use crate::song::{Cell, CompiledSong, Event, Note, TrackerSong};
    pub use crate::voice::{NoiseHat, SawSuperVoice, SquareVoice};
    pub use crate::{export_player, song, LoopPoint, Metadata, Player};
}

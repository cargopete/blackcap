//! The wasmtime component bindings, generated from `wit/jukebox.wit`.
//!
//! Host DSP resources are mapped to concrete host types via `with:`; their
//! trait impls live in [`crate::host`].

wasmtime::component::bindgen!({
    path: "../../wit",
    world: "cartridge",
    with: {
        "jukebox:cartridge/dsp.biquad-svf": crate::host::SvfNode,
        "jukebox:cartridge/dsp.reverb": crate::host::ReverbNode,
        "jukebox:cartridge/dsp.delay": crate::host::DelayNode,
        "jukebox:cartridge/dsp.waveshaper": crate::host::ShaperNode,
    },
});

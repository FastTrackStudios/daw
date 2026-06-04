//! Cross-platform multi-track audio engine.
//!
//! Decodes audio files with Symphonia, mixes multiple tracks with per-track
//! gain/mute/solo, and outputs via cpal. Works on native (macOS/Windows/Linux/
//! iOS/Android) and WASM (via cpal's wasm-bindgen backend).
//!
//! # Architecture
//!
//! ```text
//! ┌──────────┐     ┌─────────────────────────────────────────┐
//! │ Symphonia │────►│ DecodedAudio (interleaved f32 PCM)      │
//! │ (decode)  │     └──────────────┬──────────────────────────┘
//! └──────────┘                     │
//!                    ┌─────────────▼──────────────┐
//!                    │ MixerState (shared state)   │
//!                    │  • playing: bool            │
//!                    │  • position: sample frame   │
//!                    │  • tracks: Vec<TrackAudio>  │
//!                    │    - gain, muted, soloed    │
//!                    └─────────────┬──────────────┘
//!                                  │ (real-time callback)
//!                    ┌─────────────▼──────────────┐
//!                    │ cpal OutputStream           │
//!                    │  • pulls mixed PCM          │
//!                    │  • platform audio output    │
//!                    └────────────────────────────┘
//! ```

#[cfg(any(feature = "decode", feature = "audio"))]
pub mod decoder;
#[cfg(any(feature = "decode", feature = "audio"))]
pub mod materialize;
#[cfg(feature = "audio")]
mod mixer;
#[cfg(feature = "clap-host")]
pub mod plugin_host;
#[cfg(all(feature = "audio", not(target_arch = "wasm32")))]
pub mod prefetch;
#[cfg(any(feature = "decode", feature = "audio"))]
pub mod render;
/// Streaming audio sources (mmap PCM + decoded memory) — REAPER's model.
pub mod source;
#[cfg(feature = "vst3-host")]
pub mod vst3_host;
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web;
// Legacy mixer-direct RPP loader retired in favor of
// `crate::project_loader::load_rpp`, which populates the full
// `ProjectState` and routes audio through the renderer. Keep the
// gate so old `rpp-loader` consumers still compile (now a no-op).
#[cfg(feature = "audio")]
pub mod test_tone;

#[cfg(any(feature = "decode", feature = "audio"))]
pub use decoder::{DecodedAudio, decode_audio, decode_audio_with_extension};
#[cfg(feature = "audio")]
pub use mixer::{AudioEngine, TrackHandle};
pub use source::AudioSource;
#[cfg(not(target_arch = "wasm32"))]
pub use source::PcmFile;

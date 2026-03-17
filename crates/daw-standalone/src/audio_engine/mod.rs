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

mod decoder;
mod mixer;
#[cfg(feature = "rpp-loader")]
pub mod rpp_loader;
pub mod test_tone;

pub use decoder::{DecodedAudio, decode_audio, decode_audio_with_extension};
pub use mixer::{AudioEngine, TrackHandle};

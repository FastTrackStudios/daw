//! Shared audio sample loading utilities for FTS plugins.
//!
//! One crate, three layers, each behind its own cargo feature so lean
//! consumers pay only for what they use:
//!
//! - [`probe`] (always available): header-only WAV inspection — rate,
//!   channels, bit depth, frame count — with no sample decode, cheap
//!   enough to sweep whole libraries.
//! - `load` — resident decodes via Symphonium (WAV, FLAC, MP3, OGG, etc.
//!   depending on enabled features), with optional resampling:
//!   [`load_audio`] (interleaved stereo `f64`), [`load_mono_f32`],
//!   [`load_planar_f32`].
//! - `write` — [`write_wav_f32`], the shared 32-bit-float WAV writer.
//! - `mapped` — [`mapped::PcmFile`], the memory-mapped uncompressed-PCM
//!   source (header parse only; samples convert to f32 per read) — the
//!   DAW's streaming WAV path. memmap2 only, none of the `engine` codecs.
//! - `budget` — the process-wide preload RAM budget (std-only; `engine`
//!   implies it).
//! - `engine` — the sample-engine file layer (RAM budgeting, the
//!   decoded-sample cache + .signalpack access, FLAC seek index, chunked
//!   streaming, on-disk stream cache), re-exported by signal-sampler as
//!   `signal_sampler::engine::*`.
//!
//! # Usage
//!
//! ```ignore
//! use fts_sample::{probe, load_mono_f32, ResampleQuality};
//!
//! // Header only — no decode.
//! let info = probe("kick.wav".as_ref())?;
//! println!("{} Hz, {} ch, {} frames", info.sample_rate, info.channels, info.num_frames);
//!
//! // Full decode, downmixed to mono at 48 kHz.
//! let (samples, sr) = load_mono_f32("kick.wav".as_ref(), Some(48_000), ResampleQuality::default())?;
//! ```

pub mod dialog;
pub mod probe;

#[cfg(feature = "load")]
mod load;
#[cfg(feature = "mapped")]
pub mod mapped;
#[cfg(feature = "write")]
mod write;

// ── Sample-engine file layer (moved from signal-sampler's `engine/`) ─────────
//
// The audio-file half of the sampler engine: RAM budgeting, the decoded-sample
// cache + .signalpack access, the FLAC seek index, chunked streaming, and the
// on-disk stream cache. Voice/playback types stay in signal-sampler; these
// modules are re-exported there as `signal_sampler::engine::{budget, cache,
// flac_index, stream, stream_cache}`.
#[cfg(feature = "budget")]
pub mod budget;
#[cfg(feature = "engine")]
pub mod cache;
#[cfg(feature = "engine")]
pub mod flac_index;
#[cfg(feature = "engine")]
pub mod stream;
#[cfg(feature = "engine")]
pub mod stream_cache;

mod error;

pub use error::SamplerError;
pub use probe::{AudioInfo, WavFormat, probe};

#[cfg(feature = "load")]
pub use load::{
    AudioData, LoadedAudio, ResampleQuality, decode_bytes, load_audio, load_audio_async,
    load_mono_f32, load_planar_f32,
};
#[cfg(feature = "mapped")]
pub use mapped::{PcmFile, PcmFormat};
#[cfg(feature = "write")]
pub use write::write_wav_f32;

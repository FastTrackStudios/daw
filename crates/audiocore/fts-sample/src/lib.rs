//! Shared audio sample loading utilities for FTS plugins.
//!
//! Provides a common interface for loading audio files (WAV, FLAC, MP3, etc.)
//! via Symphonium, with automatic resampling and format conversion.
//!
//! # Usage
//!
//! ```ignore
//! use fts_sample::{load_audio, load_audio_async, AudioData};
//!
//! // Synchronous loading
//! let audio = load_audio("kick.wav", 48000.0)?;
//! println!("{} frames, {} channels", audio.num_frames(), audio.num_channels);
//!
//! // Async loading via channel
//! let (tx, rx) = crossbeam_channel::bounded(8);
//! load_audio_async("kick.wav".into(), 48000.0, tx, 0, |data, id| {
//!     MyMessage { slot: id, audio: data }
//! });
//! ```

pub mod dialog;

use std::path::Path;

use symphonium::{ResampleQuality, SymphoniumLoader};
use tracing::error;

/// Decoded audio data in a format suitable for DSP processing.
///
/// Stores interleaved stereo `f64` sample pairs. Mono sources are
/// duplicated to both channels during loading.
#[derive(Clone)]
pub struct AudioData {
    /// Stereo sample data: `[left, right]` pairs.
    pub data: Vec<[f64; 2]>,
    /// Sample rate of the decoded audio (after resampling).
    pub sample_rate: f64,
    /// Original file name (stem only, no extension).
    pub name: String,
}

impl AudioData {
    /// Number of sample frames.
    pub fn num_frames(&self) -> usize {
        self.data.len()
    }

    /// Duration in seconds.
    pub fn duration_secs(&self) -> f64 {
        self.data.len() as f64 / self.sample_rate
    }

    /// Number of channels (always 2 — mono sources are duplicated).
    pub fn num_channels(&self) -> usize {
        2
    }
}

/// Load an audio file synchronously, resampling to `target_sr`.
///
/// Supports any format Symphonium can decode (WAV, FLAC, MP3, OGG, etc.
/// depending on enabled features).
pub fn load_audio(
    path: &Path,
    target_sr: f64,
) -> Result<AudioData, Box<dyn std::error::Error + Send + Sync>> {
    let mut loader = SymphoniumLoader::new();
    let decoded = loader
        .load_f32(
            path.to_str().unwrap_or(""),
            Some(target_sr as u32),
            ResampleQuality::default(),
            None,
        )
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("{e}"),
            ))
        })?;

    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("sample")
        .to_string();

    let num_channels = decoded.channels() as usize;
    if num_channels == 0 {
        return Err("Empty audio file".into());
    }

    let num_frames = decoded.data[0].len();
    let mut data = Vec::with_capacity(num_frames);
    for frame in 0..num_frames {
        let l = decoded.data[0][frame] as f64;
        let r = if num_channels > 1 {
            decoded.data[1][frame] as f64
        } else {
            l
        };
        data.push([l, r]);
    }

    Ok(AudioData {
        data,
        sample_rate: target_sr,
        name,
    })
}

/// Spawn a background thread to load an audio file and send the result
/// through a crossbeam channel.
///
/// `id` is an opaque identifier passed through to the message (e.g. slot index).
/// `make_msg` converts the loaded `AudioData` + `id` into the channel's message type.
pub fn load_audio_async<M: Send + 'static>(
    path: String,
    target_sr: f64,
    tx: crossbeam_channel::Sender<M>,
    id: usize,
    make_msg: fn(AudioData, usize) -> M,
) {
    std::thread::spawn(move || match load_audio(Path::new(&path), target_sr) {
        Ok(audio) => {
            let _ = tx.send(make_msg(audio, id));
        }
        Err(e) => {
            error!("Failed to load audio (id={id}): {e}");
        }
    });
}

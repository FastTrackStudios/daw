//! Audio file decoding via Symphonia.
//!
//! Decodes audio files into interleaved f32 PCM buffers that the mixer
//! can play back. Supports WAV, MP3, OGG/Vorbis, FLAC, and AAC.

use std::io::Cursor;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tracing::{debug, info};

/// Decoded audio data: interleaved f32 PCM samples.
#[derive(Clone)]
pub struct DecodedAudio {
    /// Interleaved f32 samples (e.g., [L0, R0, L1, R1, ...] for stereo)
    pub samples: Vec<f32>,
    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: u16,
    /// Sample rate in Hz (e.g., 44100, 48000)
    pub sample_rate: u32,
}

impl DecodedAudio {
    /// Total number of sample frames (samples per channel).
    pub fn frame_count(&self) -> usize {
        if self.channels == 0 {
            return 0;
        }
        self.samples.len() / self.channels as usize
    }

    /// Duration in seconds.
    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frame_count() as f64 / self.sample_rate as f64
    }

    /// Convert a time in seconds to a sample frame index.
    pub fn seconds_to_frame(&self, seconds: f64) -> usize {
        (seconds * self.sample_rate as f64) as usize
    }
}

/// Decode audio from raw bytes.
///
/// The format is auto-detected from the data (no file extension needed).
/// Returns `None` if the data cannot be decoded.
pub fn decode_audio(data: &[u8]) -> Option<DecodedAudio> {
    decode_audio_with_hint(data, &Hint::new())
}

/// Decode audio from raw bytes with a format hint.
///
/// Use this when you know the file extension (e.g., "mp3", "wav").
pub fn decode_audio_with_extension(data: &[u8], extension: &str) -> Option<DecodedAudio> {
    let mut hint = Hint::new();
    hint.with_extension(extension);
    decode_audio_with_hint(data, &hint)
}

fn decode_audio_with_hint(data: &[u8], hint: &Hint) -> Option<DecodedAudio> {
    let cursor = Cursor::new(data.to_vec());
    let source = MediaSourceStream::new(Box::new(cursor), Default::default());

    let probed = symphonia::default::get_probe()
        .format(
            hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;

    let mut format = probed.format;

    // Find the first audio track
    let track = format.default_track()?;
    let track_id = track.id;
    let channels = track.codec_params.channels?.count() as u16;
    let sample_rate = track.codec_params.sample_rate?;

    info!("Decoding audio: {} channels, {} Hz", channels, sample_rate);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;

    let mut all_samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break; // End of stream
            }
            Err(_) => break,
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(_) => continue,
        };

        let spec = *decoded.spec();
        let num_frames = decoded.capacity();

        let mut sample_buf = SampleBuffer::<f32>::new(num_frames as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);
        all_samples.extend_from_slice(sample_buf.samples());
    }

    debug!(
        "Decoded {} frames ({:.2}s) of {}-channel audio at {} Hz",
        all_samples.len() / channels as usize,
        all_samples.len() as f64 / channels as f64 / sample_rate as f64,
        channels,
        sample_rate
    );

    Some(DecodedAudio {
        samples: all_samples,
        channels,
        sample_rate,
    })
}

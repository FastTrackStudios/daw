//! Multi-track audio mixer with cpal output.
//!
//! The mixer maintains shared state between the control thread (play/stop/seek/
//! gain changes) and the cpal audio callback thread. The callback pulls mixed
//! PCM samples in real-time.

use super::DecodedAudio;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use std::sync::{Arc, Mutex};
use tracing::{error, info};

/// Handle to a loaded track in the audio engine.
///
/// Use this to control per-track gain, mute, and solo after loading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TrackHandle(pub(crate) usize);

/// Per-track audio state shared with the mixer callback.
struct TrackAudio {
    /// Decoded PCM data for this track
    buffer: Arc<DecodedAudio>,
    /// Linear gain multiplier (0.0 = silent, 1.0 = unity)
    gain: f32,
    /// Whether this track is muted
    muted: bool,
    /// Whether this track is soloed
    soloed: bool,
}

/// Shared state between the control API and the cpal callback.
struct MixerState {
    /// Whether playback is active
    playing: bool,
    /// Current position in sample frames (at the output sample rate)
    position: usize,
    /// Output sample rate
    sample_rate: u32,
    /// Output channel count
    channels: u16,
    /// All loaded tracks
    tracks: Vec<TrackAudio>,
    /// Master gain
    master_gain: f32,
}

impl MixerState {
    /// Check if any track is soloed
    fn any_soloed(&self) -> bool {
        self.tracks.iter().any(|t| t.soloed)
    }

    /// Mix all tracks into the output buffer at the current position.
    ///
    /// This is called from the cpal audio callback — it must be fast and
    /// must not allocate or block.
    fn fill_buffer(&mut self, output: &mut [f32]) {
        let channels = self.channels as usize;
        if channels == 0 || !self.playing {
            // Fill with silence
            output.fill(0.0);
            return;
        }

        let num_frames = output.len() / channels;
        let any_soloed = self.any_soloed();

        // Zero the output buffer
        output.fill(0.0);

        for track in &self.tracks {
            // Skip muted tracks
            if track.muted {
                continue;
            }
            // If any track is soloed, only play soloed tracks
            if any_soloed && !track.soloed {
                continue;
            }
            // Skip silent tracks
            if track.gain == 0.0 {
                continue;
            }

            let buf = &track.buffer;
            let track_channels = buf.channels as usize;
            let track_rate = buf.sample_rate;

            for frame in 0..num_frames {
                // Convert output frame position to track's sample space
                let out_frame = self.position + frame;
                let track_frame = if track_rate == self.sample_rate {
                    out_frame
                } else {
                    // Simple sample rate conversion (nearest-neighbor)
                    (out_frame as f64 * track_rate as f64 / self.sample_rate as f64) as usize
                };

                if track_frame >= buf.frame_count() {
                    continue; // Past end of this track
                }

                let src_offset = track_frame * track_channels;
                let dst_offset = frame * channels;

                // Mix: handle mono→stereo, stereo→stereo, etc.
                for ch in 0..channels {
                    let src_ch = if ch < track_channels { ch } else { 0 };
                    let sample = buf.samples.get(src_offset + src_ch).copied().unwrap_or(0.0);
                    output[dst_offset + ch] += sample * track.gain;
                }
            }
        }

        // Apply master gain
        if self.master_gain != 1.0 {
            for sample in output.iter_mut() {
                *sample *= self.master_gain;
            }
        }

        // Advance position
        self.position += num_frames;
    }
}

/// Multi-track audio engine.
///
/// Load audio tracks, control playback (play/stop/seek), adjust per-track
/// gain/mute/solo. Audio is mixed and output via cpal on all platforms.
pub struct AudioEngine {
    state: Arc<Mutex<MixerState>>,
    // cpal stream is kept alive; dropping it stops audio output
    _stream: Stream,
}

impl AudioEngine {
    /// Create a new audio engine with default output device.
    ///
    /// This initializes cpal and starts the audio output stream (initially paused).
    pub fn new() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("No audio output device found")?;

        let supported_config = device
            .default_output_config()
            .map_err(|e| format!("Failed to get default output config: {e}"))?;

        let sample_rate = supported_config.sample_rate();
        let channels = supported_config.channels();

        info!(
            "Audio engine: {} channels, {} Hz, format {:?}",
            channels,
            sample_rate,
            supported_config.sample_format()
        );

        let state = Arc::new(Mutex::new(MixerState {
            playing: false,
            position: 0,
            sample_rate,
            channels,
            tracks: Vec::new(),
            master_gain: 1.0,
        }));

        let config = StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let stream = match supported_config.sample_format() {
            SampleFormat::F32 => Self::build_stream::<f32>(&device, &config, Arc::clone(&state))?,
            SampleFormat::I16 => Self::build_stream::<i16>(&device, &config, Arc::clone(&state))?,
            SampleFormat::U16 => Self::build_stream::<u16>(&device, &config, Arc::clone(&state))?,
            format => return Err(format!("Unsupported sample format: {format:?}")),
        };

        stream.play().map_err(|e| format!("Failed to start audio stream: {e}"))?;

        Ok(Self {
            state,
            _stream: stream,
        })
    }

    fn build_stream<T: cpal::SizedSample + cpal::FromSample<f32>>(
        device: &cpal::Device,
        config: &StreamConfig,
        state: Arc<Mutex<MixerState>>,
    ) -> Result<Stream, String> {
        let channels = config.channels as usize;

        // Pre-allocate a mix buffer to avoid allocation in the callback
        let max_buffer_size = 8192 * channels;
        let mix_buffer = Arc::new(Mutex::new(vec![0.0f32; max_buffer_size]));

        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let num_samples = data.len();
                    let mut mix = mix_buffer.lock().unwrap();
                    if mix.len() < num_samples {
                        mix.resize(num_samples, 0.0);
                    }

                    {
                        let mut state = state.lock().unwrap();
                        state.fill_buffer(&mut mix[..num_samples]);
                    }

                    for (out, &mixed) in data.iter_mut().zip(mix.iter()) {
                        *out = T::from_sample(mixed);
                    }
                },
                move |err| {
                    error!("Audio stream error: {err}");
                },
                None,
            )
            .map_err(|e| format!("Failed to build output stream: {e}"))?;

        Ok(stream)
    }

    // ─── Track Management ────────────────────────────────────────────────

    /// Load a decoded audio buffer as a new track. Returns a handle for control.
    pub fn add_track(&self, audio: DecodedAudio) -> TrackHandle {
        let mut state = self.state.lock().unwrap();
        let index = state.tracks.len();
        state.tracks.push(TrackAudio {
            buffer: Arc::new(audio),
            gain: 1.0,
            muted: false,
            soloed: false,
        });
        info!("Added track {index}");
        TrackHandle(index)
    }

    /// Remove all tracks.
    pub fn clear_tracks(&self) {
        let mut state = self.state.lock().unwrap();
        state.tracks.clear();
        state.position = 0;
        info!("Cleared all tracks");
    }

    /// Get the number of loaded tracks.
    pub fn track_count(&self) -> usize {
        self.state.lock().unwrap().tracks.len()
    }

    // ─── Per-Track Control ───────────────────────────────────────────────

    /// Set the gain (volume) for a track. 0.0 = silent, 1.0 = unity.
    pub fn set_track_gain(&self, handle: TrackHandle, gain: f32) {
        let mut state = self.state.lock().unwrap();
        if let Some(track) = state.tracks.get_mut(handle.0) {
            track.gain = gain.max(0.0);
        }
    }

    /// Get the gain for a track.
    pub fn track_gain(&self, handle: TrackHandle) -> f32 {
        self.state
            .lock()
            .unwrap()
            .tracks
            .get(handle.0)
            .map(|t| t.gain)
            .unwrap_or(0.0)
    }

    /// Set mute state for a track.
    pub fn set_track_muted(&self, handle: TrackHandle, muted: bool) {
        let mut state = self.state.lock().unwrap();
        if let Some(track) = state.tracks.get_mut(handle.0) {
            track.muted = muted;
        }
    }

    /// Set solo state for a track.
    pub fn set_track_soloed(&self, handle: TrackHandle, soloed: bool) {
        let mut state = self.state.lock().unwrap();
        if let Some(track) = state.tracks.get_mut(handle.0) {
            track.soloed = soloed;
        }
    }

    // ─── Transport Control ───────────────────────────────────────────────

    /// Start or resume playback from the current position.
    pub fn play(&self) {
        let mut state = self.state.lock().unwrap();
        state.playing = true;
        info!("Playback started at frame {}", state.position);
    }

    /// Pause playback, preserving position.
    pub fn pause(&self) {
        let mut state = self.state.lock().unwrap();
        state.playing = false;
        info!("Playback paused at frame {}", state.position);
    }

    /// Stop playback and reset position to start.
    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        state.playing = false;
        state.position = 0;
        info!("Playback stopped");
    }

    /// Whether playback is active.
    pub fn is_playing(&self) -> bool {
        self.state.lock().unwrap().playing
    }

    /// Seek to a position in seconds.
    pub fn seek(&self, seconds: f64) {
        let mut state = self.state.lock().unwrap();
        let frame = (seconds * state.sample_rate as f64) as usize;
        state.position = frame;
    }

    /// Get the current playback position in seconds.
    pub fn position_seconds(&self) -> f64 {
        let state = self.state.lock().unwrap();
        if state.sample_rate == 0 {
            return 0.0;
        }
        state.position as f64 / state.sample_rate as f64
    }

    /// Get the output sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.state.lock().unwrap().sample_rate
    }

    /// Set the master gain (applied after mixing all tracks).
    pub fn set_master_gain(&self, gain: f32) {
        self.state.lock().unwrap().master_gain = gain.max(0.0);
    }

    /// Get the longest track duration in seconds.
    pub fn duration_seconds(&self) -> f64 {
        let state = self.state.lock().unwrap();
        state
            .tracks
            .iter()
            .map(|t| t.buffer.duration_seconds())
            .fold(0.0f64, f64::max)
    }
}

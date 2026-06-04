//! Multi-track audio mixer with cpal output.
//!
//! Playhead + play state live in the shared [`TransportShared`] atomic
//! (see `crate::transport_engine`). The cpal callback reads the
//! playhead, mixes frames, then advances the engine via
//! `shared.advance(frames)` — so the engine drives **both** the soft
//! clock and the audio mixer when audio is active.
//!
//! Per-track gain / mute / solo + the track list are mixer-private
//! state, still held in `Arc<Mutex<MixerState>>`. The mutex is only
//! locked on control-thread mutations + once per audio callback (for
//! the track list snapshot); never blocking under contention because
//! both sides are short-lived.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use tracing::{error, info};

use crate::metering::{HOLD_DECAY, Meters};
use crate::sync::Standalone;
use crate::transport_engine::{PlayStateRepr, TransportShared};

use super::DecodedAudio;
use super::render::ProjectRenderer;

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
    /// Index of this track's cell in the [`Meters`] bank — i.e. the *project*
    /// track index this audio is metering, which need not equal the mixer's own
    /// track ordering.
    meter_index: usize,
}

/// Mixer-private state. Playhead + play state are *not* here — they
/// live in [`TransportShared`].
struct MixerState {
    /// Output sample rate (mirrors `shared.sample_rate()`; cached so
    /// the callback doesn't take an atomic load per frame).
    sample_rate: u32,
    /// Output channel count.
    channels: u16,
    /// All loaded tracks.
    tracks: Vec<TrackAudio>,
    /// Master gain.
    master_gain: f32,
    /// Per-track peak-meter bank, written once per block from `fill_buffer`.
    meters: Arc<Meters>,
}

impl MixerState {
    /// Check if any track is soloed
    fn any_soloed(&self) -> bool {
        self.tracks.iter().any(|t| t.soloed)
    }

    /// Mix all tracks into the output buffer starting at `position`
    /// (in output-rate sample frames). Does not advance the engine —
    /// the caller (audio callback) does that.
    ///
    /// As a side effect, each contributing track's L/R block peak is written
    /// into the [`Meters`] bank (after master gain) so the UI can show real
    /// per-track levels. When stopped (or silent) every cell is written `0`, so
    /// the meters decay to silence rather than freezing.
    fn fill_buffer(&self, output: &mut [f32], position: i64, playing: bool) {
        let channels = self.channels as usize;

        // Per-track block peak accumulators (L, R), indexed by meter cell.
        let mut peaks = vec![(0.0f32, 0.0f32); self.meters.len()];

        if channels == 0 || !playing {
            output.fill(0.0);
            self.flush_meters(&peaks);
            return;
        }

        let num_frames = output.len() / channels;
        let any_soloed = self.any_soloed();

        output.fill(0.0);

        for track in &self.tracks {
            if track.muted || (any_soloed && !track.soloed) || track.gain == 0.0 {
                continue;
            }

            let buf = &track.buffer;
            let track_channels = buf.channels as usize;
            let track_rate = buf.sample_rate;
            let (mut peak_l, mut peak_r) = (0.0f32, 0.0f32);

            for frame in 0..num_frames {
                let out_frame = position + frame as i64;
                if out_frame < 0 {
                    continue;
                }
                let out_frame_u = out_frame as u64;
                let track_frame = if track_rate == self.sample_rate {
                    out_frame_u as usize
                } else {
                    (out_frame_u as f64 * track_rate as f64 / self.sample_rate as f64) as usize
                };

                if track_frame >= buf.frame_count() {
                    continue;
                }

                let src_offset = track_frame * track_channels;
                let dst_offset = frame * channels;

                for ch in 0..channels {
                    let src_ch = if ch < track_channels { ch } else { 0 };
                    let sample = buf.samples.get(src_offset + src_ch).copied().unwrap_or(0.0);
                    let contribution = sample * track.gain * self.master_gain;
                    output[dst_offset + ch] += contribution;
                    match ch {
                        0 => peak_l = peak_l.max(contribution.abs()),
                        1 => peak_r = peak_r.max(contribution.abs()),
                        _ => {}
                    }
                }
            }

            // Mono sources meter as dual-mono so the strip's two columns match.
            if track_channels < 2 {
                peak_r = peak_l;
            }
            if let Some(slot) = peaks.get_mut(track.meter_index) {
                slot.0 = slot.0.max(peak_l);
                slot.1 = slot.1.max(peak_r);
            }
        }

        self.flush_meters(&peaks);
    }

    /// Push the per-track block peaks into the shared [`Meters`] bank.
    fn flush_meters(&self, peaks: &[(f32, f32)]) {
        for (i, &(l, r)) in peaks.iter().enumerate() {
            if let Some(cell) = self.meters.cell(i) {
                cell.write(l, r, HOLD_DECAY);
            }
        }
    }
}

/// Multi-track audio engine.
///
/// Load audio tracks, control playback (play/stop/seek), adjust per-track
/// gain/mute/solo. Audio is mixed and output via cpal on all platforms.
pub struct AudioEngine {
    state: Arc<Mutex<MixerState>>,
    shared: Arc<TransportShared>,
    // cpal stream is kept alive; dropping it stops audio output
    _stream: Stream,
}

impl AudioEngine {
    /// Create a new audio engine with its own private [`TransportShared`].
    /// Use [`with_shared`](Self::with_shared) when wiring into a
    /// `Standalone` so the engine + service share a playhead.
    pub fn new() -> Result<Self, String> {
        Self::with_shared(Arc::new(TransportShared::new(48_000, 120.0)))
    }

    /// Attach a private-mixer engine to a project's transport clock *and*
    /// peak-meter bank: shares `daw`'s playhead (disabling the soft clock so the
    /// audio callback drives `advance`), and installs a freshly-sized
    /// [`Meters`] bank on `daw` so per-track levels reach the `Peaks` service.
    /// Load one track per project track with [`add_track_metered`](Self::add_track_metered).
    pub fn metered_for(
        daw: &Standalone,
        project_guid: &str,
        track_count: usize,
    ) -> Result<Self, String> {
        let bundle = daw.transport_engine_for(project_guid);
        bundle.disable_soft_clock();
        let meters = Meters::new(track_count);
        daw.set_meters(meters.clone());
        Self::with_shared_metered(bundle.shared.clone(), meters)
    }

    /// Create an engine wired to a specific project on `daw`. The
    /// callback renders via [`ProjectRenderer`] every block, so the
    /// project's tracks / items / routing / audio sources are heard
    /// the moment they're loaded — no `add_track` boilerplate needed.
    ///
    /// Sample rate, transport shared state, and soft-clock disable are
    /// all handled here. Drop the returned `AudioEngine` to stop the
    /// stream.
    pub fn attached_to(daw: &Standalone, project_guid: &str) -> Result<Self, String> {
        let bundle = daw.transport_engine_for(project_guid);
        bundle.disable_soft_clock();
        Self::with_project(daw.clone(), project_guid.to_string(), bundle.shared.clone())
    }

    /// Create an engine that shares its sample clock with the given
    /// `TransportShared`. The shared sample-rate is rewritten to the
    /// device's actual rate at startup. Metering is disabled (empty bank).
    pub fn with_shared(shared: Arc<TransportShared>) -> Result<Self, String> {
        Self::with_shared_metered(shared, Meters::empty())
    }

    /// As [`with_shared`](Self::with_shared), but the mixer writes per-track
    /// block peaks into `meters` (one cell per project track index).
    pub fn with_shared_metered(
        shared: Arc<TransportShared>,
        meters: Arc<Meters>,
    ) -> Result<Self, String> {
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
            "Audio engine: {} channels, {} Hz, format {:?}, {} meter cells",
            channels,
            sample_rate,
            supported_config.sample_format(),
            meters.len(),
        );

        shared.set_sample_rate(sample_rate);

        let state = Arc::new(Mutex::new(MixerState {
            sample_rate,
            channels,
            tracks: Vec::new(),
            master_gain: 1.0,
            meters,
        }));

        let config = StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let stream = match supported_config.sample_format() {
            SampleFormat::F32 => {
                Self::build_stream::<f32>(&device, &config, state.clone(), shared.clone())?
            }
            SampleFormat::I16 => {
                Self::build_stream::<i16>(&device, &config, state.clone(), shared.clone())?
            }
            SampleFormat::U16 => {
                Self::build_stream::<u16>(&device, &config, state.clone(), shared.clone())?
            }
            format => return Err(format!("Unsupported sample format: {format:?}")),
        };

        stream
            .play()
            .map_err(|e| format!("Failed to start audio stream: {e}"))?;

        Ok(Self {
            state,
            shared,
            _stream: stream,
        })
    }

    /// Project-mode constructor. Same as [`with_shared`](Self::with_shared)
    /// but the callback renders via [`ProjectRenderer`] for
    /// `(daw, project_guid)` instead of mixing the engine's private
    /// track list. Use [`attached_to`](Self::attached_to) for the
    /// common case where you want to discover/share the
    /// `TransportShared` from a `Standalone`.
    pub fn with_project(
        daw: Standalone,
        project_guid: String,
        shared: Arc<TransportShared>,
    ) -> Result<Self, String> {
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
            "Audio engine (project mode): {} channels, {} Hz, format {:?}, guid={}",
            channels,
            sample_rate,
            supported_config.sample_format(),
            project_guid,
        );
        shared.set_sample_rate(sample_rate);

        // The project-render path meters via `ProjectRenderer`, not the private
        // track list, so the mixer's own bank stays empty here.
        let state = Arc::new(Mutex::new(MixerState {
            sample_rate,
            channels,
            tracks: Vec::new(),
            master_gain: 1.0,
            meters: Meters::empty(),
        }));

        let config = StreamConfig {
            channels,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let stream = match supported_config.sample_format() {
            SampleFormat::F32 => Self::build_project_stream::<f32>(
                &device,
                &config,
                daw.clone(),
                project_guid.clone(),
                shared.clone(),
            )?,
            SampleFormat::I16 => Self::build_project_stream::<i16>(
                &device,
                &config,
                daw.clone(),
                project_guid.clone(),
                shared.clone(),
            )?,
            SampleFormat::U16 => Self::build_project_stream::<u16>(
                &device,
                &config,
                daw.clone(),
                project_guid.clone(),
                shared.clone(),
            )?,
            format => return Err(format!("Unsupported sample format: {format:?}")),
        };
        stream
            .play()
            .map_err(|e| format!("Failed to start audio stream: {e}"))?;
        Ok(Self {
            state,
            shared,
            _stream: stream,
        })
    }

    fn build_project_stream<T: cpal::SizedSample + cpal::FromSample<f32>>(
        device: &cpal::Device,
        config: &StreamConfig,
        daw: Standalone,
        project_guid: String,
        shared: Arc<TransportShared>,
    ) -> Result<Stream, String> {
        let channels = config.channels as usize;
        let sample_rate = config.sample_rate;
        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let num_samples = data.len();
                    if channels == 0 || num_samples == 0 {
                        return;
                    }
                    let num_frames = num_samples / channels;
                    let playing = shared.play_state().is_advancing();
                    if !playing {
                        for s in data.iter_mut() {
                            *s = T::from_sample(0.0);
                        }
                        return;
                    }
                    let start = shared.playhead_samples().0.max(0) as u64;

                    // Snapshot + render. ProjectRenderer briefly
                    // acquires the project Mutex (Vec snapshot only,
                    // not the render itself). Future work: lock-free
                    // RCU snapshot so the callback never blocks.
                    let block = ProjectRenderer::new(&daw, &project_guid, sample_rate)
                        .render_block(start, num_frames);

                    // Interleave the stereo block into the cpal
                    // buffer (handle channel-count mismatch by
                    // duplicating or summing).
                    for frame in 0..num_frames {
                        let l = block.samples.get(frame * 2).copied().unwrap_or(0.0);
                        let r = block.samples.get(frame * 2 + 1).copied().unwrap_or(0.0);
                        for ch in 0..channels {
                            let v = match ch {
                                0 => l,
                                1 => r,
                                _ => (l + r) * 0.5, // surround channels get the mono sum
                            };
                            data[frame * channels + ch] = T::from_sample(v);
                        }
                    }

                    shared.advance(num_frames as u32);
                },
                move |err| {
                    error!("Audio stream error: {err}");
                },
                None,
            )
            .map_err(|e| format!("Failed to build output stream: {e}"))?;
        Ok(stream)
    }

    /// Access the shared transport state — useful for wiring into the
    /// transport service / soft clock.
    pub fn shared(&self) -> &Arc<TransportShared> {
        &self.shared
    }

    fn build_stream<T: cpal::SizedSample + cpal::FromSample<f32>>(
        device: &cpal::Device,
        config: &StreamConfig,
        state: Arc<Mutex<MixerState>>,
        shared: Arc<TransportShared>,
    ) -> Result<Stream, String> {
        let channels = config.channels as usize;
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

                    // Snapshot engine state — playhead at *start* of
                    // block, advance after mixing so the next block
                    // sees the post-advance position.
                    let playing = shared.play_state().is_advancing();
                    let start = shared.playhead_samples().0;
                    let num_frames = (num_samples / channels) as u32;

                    {
                        let st = state.lock().unwrap();
                        st.fill_buffer(&mut mix[..num_samples], start, playing);
                    }

                    if playing {
                        shared.advance(num_frames);
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
    /// The track meters into the cell matching its own load order; use
    /// [`add_track_metered`](Self::add_track_metered) to meter into a specific
    /// project track index instead.
    pub fn add_track(&self, audio: DecodedAudio) -> TrackHandle {
        let index = self.state.lock().unwrap().tracks.len();
        self.add_track_metered(audio, index)
    }

    /// Load a decoded buffer as a new track that meters into `meter_index` (the
    /// project track index this audio represents). Returns a handle for control.
    pub fn add_track_metered(&self, audio: DecodedAudio, meter_index: usize) -> TrackHandle {
        let mut state = self.state.lock().unwrap();
        let index = state.tracks.len();
        state.tracks.push(TrackAudio {
            buffer: Arc::new(audio),
            gain: 1.0,
            muted: false,
            soloed: false,
            meter_index,
        });
        info!("Added track {index} (meter cell {meter_index})");
        TrackHandle(index)
    }

    /// Remove all tracks.
    pub fn clear_tracks(&self) {
        let mut state = self.state.lock().unwrap();
        state.tracks.clear();
        self.shared
            .set_playhead(crate::transport_engine::InstantSamples(0));
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

    // ─── Transport Control (delegated to TransportShared) ────────────────

    /// Start or resume playback from the current position.
    pub fn play(&self) {
        self.shared.set_play_state(PlayStateRepr::Playing);
        info!(
            "Playback started at frame {}",
            self.shared.playhead_samples().0
        );
    }

    /// Pause playback, preserving position.
    pub fn pause(&self) {
        self.shared.set_play_state(PlayStateRepr::Paused);
        info!(
            "Playback paused at frame {}",
            self.shared.playhead_samples().0
        );
    }

    /// Stop playback and reset position to start.
    pub fn stop(&self) {
        self.shared.set_play_state(PlayStateRepr::Stopped);
        self.shared
            .set_playhead(crate::transport_engine::InstantSamples(0));
        info!("Playback stopped");
    }

    /// Whether playback is active.
    pub fn is_playing(&self) -> bool {
        self.shared.play_state().is_advancing()
    }

    /// Seek to a position in seconds.
    pub fn seek(&self, seconds: f64) {
        let clock = crate::transport_engine::SampleClock::new(self.shared.sample_rate());
        let samples = clock.seconds_to_samples(crate::transport_engine::InstantSeconds(seconds));
        self.shared.set_playhead(samples);
    }

    /// Get the current playback position in seconds.
    pub fn position_seconds(&self) -> f64 {
        let clock = crate::transport_engine::SampleClock::new(self.shared.sample_rate());
        clock.samples_to_seconds(self.shared.playhead_samples()).0
    }

    /// Get the output sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.shared.sample_rate()
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

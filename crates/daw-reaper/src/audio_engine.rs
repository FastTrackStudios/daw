//! REAPER Audio Engine Implementation
//!
//! Implements AudioEngineService by dispatching REAPER API calls to the main thread.
//!
//! # Main Thread Safety
//!
//! REAPER audio APIs can be called from any thread, but we dispatch to the main
//! thread for consistency and to ensure proper initialization.

use crate::main_thread;
use crate::safe_wrappers::audio as sw;
use daw_proto::{
    AudioEngineService, AudioEngineState, AudioInputChannel, AudioInputInfo, AudioLatency,
};
use reaper_high::Reaper;
use tracing::debug;

/// REAPER audio engine implementation
#[derive(Clone)]
pub struct ReaperAudioEngine;

impl ReaperAudioEngine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperAudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngineService for ReaperAudioEngine {
    async fn get_state(&self) -> AudioEngineState {
        debug!("ReaperAudioEngine: get_state called");
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            let is_running = medium.audio_is_running();
            let is_prebuffer = medium.low().Audio_IsPreBuffer() != 0;

            // Always try to get latency info - REAPER reports configured latency
            // even when audio engine isn't actively running
            let latency = get_audio_latency_internal(medium);

            debug!(
                "AudioEngineState: running={}, prebuffer={}, in={}, out={}, rate={}",
                is_running,
                is_prebuffer,
                latency.input_samples,
                latency.output_samples,
                latency.sample_rate
            );

            AudioEngineState {
                is_running,
                is_prebuffer,
                latency,
            }
        })
        .await
        .unwrap_or_default()
    }

    async fn get_latency(&self) -> AudioLatency {
        debug!("ReaperAudioEngine: get_latency");
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            get_audio_latency_internal(medium)
        })
        .await
        .unwrap_or_default()
    }

    async fn get_output_latency_seconds(&self) -> f64 {
        debug!("ReaperAudioEngine: get_output_latency_seconds");
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();

            // Check if audio is running first
            if !medium.audio_is_running() {
                return 0.0;
            }

            // GetOutputLatency returns seconds directly
            medium.low().GetOutputLatency()
        })
        .await
        .unwrap_or(0.0)
    }

    async fn is_running(&self) -> bool {
        debug!("ReaperAudioEngine: is_running");
        main_thread::query(|| {
            let reaper = Reaper::get();
            reaper.medium_reaper().audio_is_running()
        })
        .await
        .unwrap_or(false)
    }

    async fn get_audio_inputs(&self) -> AudioInputInfo {
        debug!("ReaperAudioEngine: get_audio_inputs");
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let low = medium.low();

            // Get device name via GetAudioDeviceInfo("IDENT_IN")
            let device_name = sw::get_audio_device_info(low, c"IDENT_IN", 256).unwrap_or_default();

            // Get number of audio inputs
            let num_inputs = low.GetNumAudioInputs() as u32;

            // Enumerate each channel name
            let channels: Vec<AudioInputChannel> = (0..num_inputs)
                .map(|i| {
                    let name = medium.get_input_channel_name(i, |cstr| {
                        cstr.map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| format!("Input {}", i + 1))
                    });
                    AudioInputChannel { index: i, name }
                })
                .collect();

            debug!(
                "Audio inputs: device='{}', {} channels",
                device_name,
                channels.len()
            );

            AudioInputInfo {
                device_name,
                channels,
            }
        })
        .await
        .unwrap_or_default()
    }

    async fn init(&self) {
        debug!("ReaperAudioEngine: init (Audio_Init)");
        main_thread::query(|| {
            let reaper = Reaper::get();
            reaper.medium_reaper().low().Audio_Init();
        })
        .await;
    }

    async fn quit(&self) {
        debug!("ReaperAudioEngine: quit (Audio_Quit)");
        main_thread::query(|| {
            let reaper = Reaper::get();
            reaper.medium_reaper().low().Audio_Quit();
        })
        .await;
    }
}

/// Internal helper to get audio latency from REAPER.
/// MUST be called from the main thread.
fn get_audio_latency_internal(medium: &reaper_medium::Reaper) -> AudioLatency {
    // Get latency in samples
    let lat_result = medium.get_input_output_latency();

    // Get sample rate - prefer audio device rate, fall back to project rate
    let sample_rate = get_sample_rate(medium);

    // Compute output latency in seconds
    let output_seconds = if sample_rate > 0 {
        lat_result.output_latency as f64 / sample_rate as f64
    } else {
        0.0
    };

    AudioLatency {
        input_samples: lat_result.input_latency,
        output_samples: lat_result.output_latency,
        output_seconds,
        sample_rate,
    }
}

/// Get the current sample rate from REAPER.
/// Tries audio device first, falls back to project sample rate.
fn get_sample_rate(medium: &reaper_medium::Reaper) -> u32 {
    let low = medium.low();

    // First try to get audio device sample rate (most accurate when audio is running)
    if let Some(srate_str) = sw::get_audio_device_info(low, c"SRATE", 64)
        && let Ok(rate) = srate_str.parse::<u32>()
        && rate > 0
    {
        return rate;
    }

    // Fall back to project sample rate
    let reaper = Reaper::get();
    let project = reaper.current_project();

    let use_custom = sw::get_set_project_info(low, project.raw(), c"PROJECT_SRATE_USE", 0.0, false);

    if use_custom > 0.0 {
        let rate = sw::get_set_project_info(low, project.raw(), c"PROJECT_SRATE", 0.0, false);
        if rate > 0.0 {
            return rate as u32;
        }
    }

    // Default fallback
    44100
}

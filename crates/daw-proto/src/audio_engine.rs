//! Audio Engine Service
//!
//! This module provides access to the DAW's audio engine state and configuration,
//! including latency information useful for synchronization.

use facet::Facet;
use roam::service;

/// Audio latency information from the audio device
///
/// Latency values are provided in both samples and seconds for convenience.
/// The sample rate is included to allow conversion between the two.
#[derive(Clone, Debug, Default, Facet)]
pub struct AudioLatency {
    /// Input latency in samples
    pub input_samples: u32,
    /// Output latency in samples
    pub output_samples: u32,
    /// Output latency in seconds (computed from samples / sample_rate)
    pub output_seconds: f64,
    /// Current sample rate in Hz
    pub sample_rate: u32,
}

/// Complete audio engine state
#[derive(Clone, Debug, Default, Facet)]
pub struct AudioEngineState {
    /// Whether the audio engine is currently running
    pub is_running: bool,
    /// Whether the audio engine is in pre-buffer mode
    pub is_prebuffer: bool,
    /// Current latency information
    pub latency: AudioLatency,
}

/// Audio engine service for querying audio device state and latency
///
/// This service provides read-only access to the audio engine's current state,
/// which is useful for:
/// - Determining if audio is active
/// - Computing latency compensation for visual sync
/// - Monitoring audio device health
///
/// Unlike TransportService, this service does not require a ProjectContext
/// because audio engine settings are global to the DAW instance.
#[service]
pub trait AudioEngineService {
    /// Get complete audio engine state including latency
    async fn get_state(&self) -> AudioEngineState;

    /// Get current latency information
    ///
    /// Returns input and output latency in both samples and seconds.
    /// Useful for computing visual compensation offsets.
    async fn get_latency(&self) -> AudioLatency;

    /// Get output latency in seconds
    ///
    /// This is a convenience method that returns just the output latency
    /// as a floating-point seconds value, which is directly usable for
    /// compensating visual elements to sync with audio output.
    ///
    /// Returns 0.0 if the audio engine is not running.
    async fn get_output_latency_seconds(&self) -> f64;

    /// Check if the audio engine is currently running
    ///
    /// Returns true if audio is actively processing, false if stopped
    /// or in an error state.
    async fn is_running(&self) -> bool;
}

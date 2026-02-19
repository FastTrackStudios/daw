//! Peak metering and waveform data types

use facet::Facet;

/// Peak data for a take waveform
#[derive(Clone, Debug, Facet)]
pub struct TakePeakData {
    /// Sample rate of the peak data
    pub sample_rate: f64,
    /// Number of channels
    pub num_channels: u32,
    /// Peak values per channel (min/max pairs)
    /// Format: [ch0_min, ch0_max, ch1_min, ch1_max, ...]
    pub peaks: Vec<f64>,
    /// Number of samples per peak block
    pub samples_per_peak: u32,
}

/// Track peak meter reading
#[derive(Clone, Copy, Debug, Facet)]
pub struct TrackPeak {
    /// Peak level in dB (negative values, 0.0 = full scale)
    pub peak_db: f64,
    /// Peak hold level in dB
    pub peak_hold_db: f64,
}

impl Default for TakePeakData {
    fn default() -> Self {
        Self {
            sample_rate: 44100.0,
            num_channels: 2,
            peaks: Vec::new(),
            samples_per_peak: 1024,
        }
    }
}

impl Default for TrackPeak {
    fn default() -> Self {
        Self {
            peak_db: -150.0, // Silence
            peak_hold_db: -150.0,
        }
    }
}

//! MIDI analysis service for chart/chord generation.
//!
//! This service is read-only and DAW-backed. It is intentionally separate from
//! `MidiService`, which focuses on take-level MIDI editing operations.

use crate::ProjectContext;
use facet::Facet;
use roam::service;

/// Request to generate chart/chord data from a DAW project.
#[derive(Clone, Debug, Facet)]
pub struct MidiChartRequest {
    /// Project to analyze (current or explicit GUID)
    pub project: ProjectContext,
    /// Optional track tag token to match against track names
    pub track_tag: Option<String>,
}

impl MidiChartRequest {
    /// Construct a new request.
    pub fn new(project: ProjectContext, track_tag: Option<String>) -> Self {
        Self { project, track_tag }
    }
}

/// Detected chord event from source MIDI.
#[derive(Clone, Debug, PartialEq, Facet)]
pub struct MidiDetectedChord {
    /// Chord symbol text
    pub symbol: String,
    /// Start position in PPQ ticks
    pub start_ppq: i64,
    /// End position in PPQ ticks
    pub end_ppq: i64,
    /// Root pitch in MIDI note numbers
    pub root_pitch: u8,
    /// Max velocity observed in chord notes
    pub velocity: u8,
}

/// Result of project MIDI analysis for chart rendering/hydration.
#[derive(Clone, Debug, PartialEq, Facet)]
pub struct MidiChartData {
    /// Name of the track selected as analysis source
    pub source_track_name: String,
    /// Fingerprint used for cache invalidation and live updates
    pub source_fingerprint: String,
    /// Generated chart text
    pub chart_text: String,
    /// Detected chord events from source MIDI
    pub chords: Vec<MidiDetectedChord>,
}

/// Read-only MIDI analysis service.
#[service]
pub trait MidiAnalysisService {
    /// Analyze MIDI for the given request and return chart/chord data.
    async fn generate_chart_data(&self, request: MidiChartRequest) -> Result<MidiChartData, String>;
}

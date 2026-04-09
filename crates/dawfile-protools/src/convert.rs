//! Conversion from Pro Tools types to `daw_proto` types.
//!
//! Maps `ProToolsSession` data to the format-agnostic protocol types
//! used by the rest of the DAW system.

use crate::types::ProToolsSession;
use daw_proto::capability::{Capability, FeatureSupport};

/// The capability declaration for the Pro Tools file format parser.
///
/// This is read-only: we can parse session data but cannot write .ptx files.
pub fn feature_support() -> FeatureSupport {
    FeatureSupport::new().read_only(&[
        Capability::Project,
        Capability::Tracks,
        Capability::Items,
        Capability::Regions,
        Capability::Midi,
        Capability::TempoMap, // TODO: not yet implemented, but the block type exists
    ])
}

/// Summary of a parsed session, suitable for display.
pub fn session_summary(session: &ProToolsSession) -> String {
    format!(
        "Pro Tools v{} session @ {}Hz: {} audio files, {} audio regions, \
         {} audio tracks, {} MIDI regions, {} MIDI tracks",
        session.version,
        session.session_sample_rate,
        session.audio_files.len(),
        session.audio_regions.len(),
        session.audio_tracks.len(),
        session.midi_regions.len(),
        session.midi_tracks.len(),
    )
}

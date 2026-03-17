//! Structural diff engine for REAPER projects.
//!
//! Compares two `ReaperProject` snapshots and produces a [`ProjectDiff`]
//! describing exactly what changed — tracks added/removed/modified,
//! items moved, envelope points changed, MIDI events inserted, etc.
//!
//! # Identity Matching
//!
//! Entities are matched by their GUID where available:
//! - Tracks: `track_id`
//! - Items: `item_guid` (IGUID)
//! - Takes: `take_guid`
//! - FX: `fxid`
//! - Envelopes: `guid` + `envelope_type`
//!
//! Entities without GUIDs (envelope points, MIDI events, tempo points) are
//! matched by position using a two-pointer merge algorithm.
//!
//! # Example
//!
//! ```rust,no_run
//! use dawfile_reaper::io::read_project;
//! use dawfile_reaper::diff::diff_projects;
//!
//! let old = read_project("before.RPP").unwrap();
//! let new = read_project("after.RPP").unwrap();
//! let diff = diff_projects(&old, &new);
//!
//! println!("{}", diff.summary());
//! for track in &diff.tracks {
//!     println!("  track {:?}: {:?}", track.name, track.kind);
//! }
//! ```

pub mod types;
mod envelope;
mod fx;
mod item;
mod markers;
mod midi;
mod track;

pub use types::*;

use crate::types::ReaperProject;

/// Float comparison with epsilon tolerance.
pub(crate) fn f64_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Compare two `ReaperProject` snapshots and produce a structured diff.
pub fn diff_projects(old: &ReaperProject, new: &ReaperProject) -> ProjectDiff {
    let mut property_changes = Vec::new();

    // Project-level scalar properties
    if !f64_eq(old.version, new.version) {
        property_changes.push(PropertyChange {
            field: "version".into(),
            old_value: format!("{}", old.version),
            new_value: format!("{}", new.version),
        });
    }
    if old.version_string != new.version_string {
        property_changes.push(PropertyChange {
            field: "version_string".into(),
            old_value: old.version_string.clone(),
            new_value: new.version_string.clone(),
        });
    }

    // Properties struct — compare as a whole for now
    if old.properties != new.properties {
        property_changes.push(PropertyChange {
            field: "properties".into(),
            old_value: "(changed)".into(),
            new_value: "(changed)".into(),
        });
    }

    let tracks = track::diff_tracks(&old.tracks, &new.tracks);
    let envelopes = envelope::diff_envelopes(&old.envelopes, &new.envelopes);
    let markers_regions =
        markers::diff_markers_regions(&old.markers_regions, &new.markers_regions);
    let tempo_envelope =
        envelope::diff_tempo_envelope(old.tempo_envelope.as_ref(), new.tempo_envelope.as_ref());

    let ruler_lanes_changed = old.ruler_lanes != new.ruler_lanes
        || old.ruler_height != new.ruler_height;

    ProjectDiff {
        property_changes,
        tracks,
        envelopes,
        markers_regions,
        tempo_envelope,
        ruler_lanes_changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_projects_empty_diff() {
        let project = ReaperProject::default();
        let diff = diff_projects(&project, &project);
        assert!(diff.is_empty());
        assert_eq!(diff.summary(), "no changes");
    }

    #[test]
    fn version_change_detected() {
        let mut old = ReaperProject::default();
        old.version = 6.75;
        let mut new = old.clone();
        new.version = 7.0;

        let diff = diff_projects(&old, &new);
        assert!(!diff.is_empty());
        assert!(diff.property_changes.iter().any(|p| p.field == "version"));
    }
}

//! Track-level diffing — GUID-based matching with property comparison.

use std::collections::HashMap;

use crate::types::Track;
use super::{envelope, fx, item};
use super::types::*;
use super::f64_eq;

/// Diff two lists of tracks, matched by `track_id` (GUID).
pub(crate) fn diff_tracks(old: &[Track], new: &[Track], options: &DiffOptions) -> Vec<TrackDiff> {
    let mut diffs = Vec::new();

    let old_map: HashMap<&str, &Track> = old
        .iter()
        .filter_map(|t| t.track_id.as_deref().map(|g| (g, t)))
        .collect();

    let new_map: HashMap<&str, &Track> = new
        .iter()
        .filter_map(|t| t.track_id.as_deref().map(|g| (g, t)))
        .collect();

    // Tracks without GUIDs — match by index
    let old_no_guid: Vec<(usize, &Track)> = old
        .iter()
        .enumerate()
        .filter(|(_, t)| t.track_id.is_none())
        .collect();
    let new_no_guid: Vec<(usize, &Track)> = new
        .iter()
        .enumerate()
        .filter(|(_, t)| t.track_id.is_none())
        .collect();

    // Removed or modified (GUID-matched)
    for (&guid, &old_track) in &old_map {
        match new_map.get(guid) {
            None => {
                diffs.push(TrackDiff {
                    guid: Some(guid.to_string()),
                    name: old_track.name.clone(),
                    kind: ChangeKind::Removed,
                    property_changes: Vec::new(),
                    items: Vec::new(),
                    envelopes: Vec::new(),
                    fx_chain: None,
                });
            }
            Some(&new_track) => {
                if let Some(diff) = diff_single_track(old_track, new_track, options) {
                    diffs.push(diff);
                }
            }
        }
    }

    // Added (GUID-matched)
    for (&guid, &new_track) in &new_map {
        if !old_map.contains_key(guid) {
            diffs.push(TrackDiff {
                guid: Some(guid.to_string()),
                name: new_track.name.clone(),
                kind: ChangeKind::Added,
                property_changes: Vec::new(),
                items: Vec::new(),
                envelopes: Vec::new(),
                fx_chain: None,
            });
        }
    }

    // No-GUID tracks by index
    let common = old_no_guid.len().min(new_no_guid.len());
    for i in 0..common {
        if let Some(diff) = diff_single_track(old_no_guid[i].1, new_no_guid[i].1, options) {
            diffs.push(diff);
        }
    }
    for i in common..old_no_guid.len() {
        diffs.push(TrackDiff {
            guid: None,
            name: old_no_guid[i].1.name.clone(),
            kind: ChangeKind::Removed,
            property_changes: Vec::new(),
            items: Vec::new(),
            envelopes: Vec::new(),
            fx_chain: None,
        });
    }
    for i in common..new_no_guid.len() {
        diffs.push(TrackDiff {
            guid: None,
            name: new_no_guid[i].1.name.clone(),
            kind: ChangeKind::Added,
            property_changes: Vec::new(),
            items: Vec::new(),
            envelopes: Vec::new(),
            fx_chain: None,
        });
    }

    diffs
}

fn diff_single_track(old: &Track, new: &Track, options: &DiffOptions) -> Option<TrackDiff> {
    let mut props = Vec::new();

    // Compare scalar properties
    macro_rules! check {
        ($field:ident) => {
            if old.$field != new.$field {
                props.push(PropertyChange {
                    field: stringify!($field).into(),
                    old_value: format!("{:?}", old.$field),
                    new_value: format!("{:?}", new.$field),
                });
            }
        };
    }

    check!(name);
    check!(selected);
    check!(locked);
    check!(peak_color);
    check!(automation_mode);
    check!(volpan);
    check!(mutesolo);
    check!(invert_phase);
    check!(folder);
    check!(channel_count);
    check!(fx_enabled);
    check!(record);
    check!(track_height);
    check!(master_send);

    // Sub-diffs
    let item_diffs = item::diff_items(&old.items, &new.items, options);
    let env_diffs = envelope::diff_envelopes(&old.envelopes, &new.envelopes, options);
    let fx_diff = fx::diff_fx_chains(old.fx_chain.as_ref(), new.fx_chain.as_ref());

    if props.is_empty()
        && item_diffs.is_empty()
        && env_diffs.is_empty()
        && fx_diff.is_none()
    {
        None
    } else {
        Some(TrackDiff {
            guid: new.track_id.clone(),
            name: new.name.clone(),
            kind: ChangeKind::Modified,
            property_changes: props,
            items: item_diffs,
            envelopes: env_diffs,
            fx_chain: fx_diff,
        })
    }
}

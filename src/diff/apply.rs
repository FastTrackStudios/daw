//! Patch applicator — applies a [`ProjectDiff`] to a target [`ReaperProject`].
//!
//! # Semantics
//!
//! The diff is computed as `diff_projects_with_options(song, setlist_section, opts)`
//! where *old* = individual song RPP and *new* = the setlist section.
//!
//! When applying the diff to update the setlist target:
//!
//! | `ChangeKind` | Meaning in the diff | Action on the target |
//! |---|---|---|
//! | `Removed` | In song, not in setlist | **Insert** into target (with offset) |
//! | `Added` | In setlist, not in song | **Keep** (setlist-only content) |
//! | `Modified` | In both, but differs | **Update** target to match song |
//!
//! # Limitations (TODO)
//!
//! - FX chain state changes are detected but not applied.
//! - MIDI event-level changes are detected but not applied.
//! - Tempo envelope changes are not yet applied.
//! - Project-level property changes are not yet applied.

use crate::types::ReaperProject;
use crate::types::marker_region::MarkerRegion;

use super::f64_eq;
use super::types::*;

/// Apply a [`ProjectDiff`] to a target [`ReaperProject`], mutating it in place.
///
/// See the [module-level docs](self) for semantics of each `ChangeKind`.
pub fn apply_diff(target: &mut ReaperProject, diff: &ProjectDiff, options: &ApplyOptions) {
    apply_tracks(target, &diff.tracks, options);
    apply_markers_regions(target, &diff.markers_regions, options);
    apply_project_envelopes(target, &diff.envelopes, options);
    // TODO: apply tempo envelope changes
    // TODO: apply project-level property changes
}

// ── Tracks ──────────────────────────────────────────────────────────────────

fn apply_tracks(target: &mut ReaperProject, track_diffs: &[TrackDiff], options: &ApplyOptions) {
    for td in track_diffs {
        match td.kind {
            ChangeKind::Added => {
                // Item exists only in the setlist — already in target, nothing to do.
            }
            ChangeKind::Removed => {
                // Item exists in the song but not the setlist — we need to add
                // a new track to the target. We can't reconstruct a full Track
                // from a TrackDiff alone (the diff only carries changed fields),
                // so for Removed tracks we create a stub with the known identity.
                //
                // In practice, track-level Removed is rare in the setlist sync
                // workflow (tracks are matched by name and should exist in both).
                // The main use case for Removed is items/markers within a track.
                //
                // TODO: carry source Track data in the diff or pass the source
                // project alongside the diff so we can clone the full track.
            }
            ChangeKind::Modified => {
                if let Some(track) = find_track_mut(target, td, options) {
                    apply_track_items(track, &td.items, options);
                    apply_track_envelopes(track, &td.envelopes, options);
                    // TODO: apply FX chain changes (td.fx_chain)
                    // TODO: apply scalar property changes (td.property_changes)
                }
            }
        }
    }
}

fn find_track_mut<'a>(
    target: &'a mut ReaperProject,
    td: &TrackDiff,
    options: &ApplyOptions,
) -> Option<&'a mut crate::types::track::Track> {
    if options.match_tracks_by_name {
        target.tracks.iter_mut().find(|t| t.name == td.name)
    } else {
        match &td.guid {
            Some(guid) => target
                .tracks
                .iter_mut()
                .find(|t| t.track_id.as_deref() == Some(guid.as_str())),
            None => target.tracks.iter_mut().find(|t| t.name == td.name),
        }
    }
}

// ── Items ───────────────────────────────────────────────────────────────────

fn apply_track_items(
    track: &mut crate::types::track::Track,
    item_diffs: &[ItemDiff],
    options: &ApplyOptions,
) {
    use crate::types::Item;

    for id in item_diffs {
        match id.kind {
            ChangeKind::Added => {
                // Exists only in setlist — already in target, nothing to do.
            }
            ChangeKind::Removed => {
                // Exists in song but not setlist — insert a stub item into
                // the target track. We set the position with offset applied.
                //
                // We cannot fully reconstruct the item from the diff alone
                // (the diff only records identity + property changes, not the
                // full source item). In a real pipeline the caller would pair
                // the diff with the source project to clone the full item.
                //
                // For now we insert a minimal placeholder with the GUID and
                // name so that a subsequent diff pass will detect it.
                let item = Item {
                    item_guid: id.guid.clone(),
                    name: id.name.clone(),
                    ..Item::default()
                };
                // Apply position offset — the item's original position in
                // the song coordinate space maps to (position + offset) in
                // the setlist coordinate space.
                // Since this is a Removed item, we don't have the original
                // position directly — it would come from the source project.
                // Callers who need full fidelity should use apply_diff_with_source.
                track.items.push(item);
            }
            ChangeKind::Modified => {
                if let Some(item) = find_item_mut(&mut track.items, id) {
                    apply_item_properties(item, &id.property_changes, options);
                    // TODO: apply take-level changes (id.takes)
                }
            }
        }
    }
}

fn find_item_mut<'a>(
    items: &'a mut [crate::types::Item],
    id: &ItemDiff,
) -> Option<&'a mut crate::types::Item> {
    // Find the index first to avoid multiple mutable borrows.
    let idx = if let Some(guid) = &id.guid {
        items
            .iter()
            .position(|i| i.item_guid.as_deref() == Some(guid.as_str()))
    } else {
        None
    };

    let idx = idx.or_else(|| items.iter().position(|i| i.name == id.name));

    idx.map(move |i| &mut items[i])
}

fn apply_item_properties(
    item: &mut crate::types::Item,
    changes: &[PropertyChange],
    options: &ApplyOptions,
) {
    let offset = options.position_offset;

    for change in changes {
        match change.field.as_str() {
            "position" => {
                if let Ok(v) = change.old_value.parse::<f64>() {
                    // The old_value is the song-space position; in the diff
                    // engine the new_value is the setlist-space position after
                    // offset subtraction. We want to set the target to the
                    // song-space position + offset.
                    item.position = v + offset;
                }
            }
            "length" => {
                if let Ok(v) = change.old_value.parse::<f64>() {
                    item.length = v;
                }
            }
            "name" => {
                item.name = change.old_value.clone();
            }
            "snap_offset" => {
                if let Ok(v) = change.old_value.parse::<f64>() {
                    item.snap_offset = v;
                }
            }
            "loop_source" => {
                item.loop_source = change.old_value == "true";
            }
            "mute" => {
                // Mute is stored as a formatted string; skip complex parsing
                // for now.
            }
            _ => {
                // Unknown or unhandled property — skip.
            }
        }
    }
}

// ── Envelopes ───────────────────────────────────────────────────────────────

fn apply_track_envelopes(
    track: &mut crate::types::track::Track,
    env_diffs: &[EnvelopeDiff],
    options: &ApplyOptions,
) {
    apply_envelope_list(&mut track.envelopes, env_diffs, options);
}

fn apply_project_envelopes(
    target: &mut ReaperProject,
    env_diffs: &[EnvelopeDiff],
    options: &ApplyOptions,
) {
    apply_envelope_list(&mut target.envelopes, env_diffs, options);
}

fn apply_envelope_list(
    envelopes: &mut [crate::types::envelope::Envelope],
    env_diffs: &[EnvelopeDiff],
    options: &ApplyOptions,
) {
    for ed in env_diffs {
        match ed.kind {
            ChangeKind::Added => {
                // Exists only in setlist — already in target.
            }
            ChangeKind::Removed => {
                // Exists in song but not setlist — we would need to insert
                // the full envelope from the source. For now, skip.
                // TODO: insert envelope from source project.
            }
            ChangeKind::Modified => {
                if let Some(env) = envelopes
                    .iter_mut()
                    .find(|e| e.guid == ed.guid && e.envelope_type == ed.envelope_type)
                {
                    apply_envelope_point_changes(env, &ed.point_changes, options);
                    // TODO: apply automation item changes
                }
            }
        }
    }
}

fn apply_envelope_point_changes(
    env: &mut crate::types::envelope::Envelope,
    changes: &[PointChange],
    options: &ApplyOptions,
) {
    use crate::types::envelope::{EnvelopePoint, EnvelopePointShape};
    let offset = options.position_offset;

    for change in changes {
        match change {
            PointChange::Removed(snapshot) => {
                // Point exists in song but not in setlist → add it to the
                // target at the offset position.
                let point = EnvelopePoint {
                    position: snapshot.position + offset,
                    value: snapshot.value,
                    shape: EnvelopePointShape::from(snapshot.shape),
                    time_sig: None,
                    selected: None,
                    unknown_field_6: None,
                    bezier_tension: None,
                };
                // Insert in sorted order
                let idx = env
                    .points
                    .binary_search_by(|p| {
                        p.position
                            .partial_cmp(&point.position)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap_or_else(|i| i);
                env.points.insert(idx, point);
            }
            PointChange::Added(_) => {
                // Point exists only in setlist — keep it in the target.
            }
            PointChange::Modified { old, new: _ } => {
                // Update the target point to match the song version.
                // Find by the *new* position (setlist-space) since that's
                // what's in the target.
                if let Some(pt) = env.points.iter_mut().find(|p| {
                    // The new snapshot position was already offset-adjusted
                    // by the diff engine, so we compare directly.
                    f64_eq(p.position, old.position + offset)
                }) {
                    pt.value = old.value;
                    pt.shape = EnvelopePointShape::from(old.shape);
                }
            }
        }
    }
}

// ── Markers / Regions ───────────────────────────────────────────────────────

fn apply_markers_regions(
    target: &mut ReaperProject,
    diffs: &[MarkerRegionDiff],
    options: &ApplyOptions,
) {
    let offset = options.position_offset;

    for md in diffs {
        match md.kind {
            ChangeKind::Added => {
                // Exists only in setlist — already in target, nothing to do.
            }
            ChangeKind::Removed => {
                // Exists in the song but not the setlist → insert it into
                // the target. We reconstruct a MarkerRegion from the diff.
                let marker = MarkerRegion {
                    id: md.id,
                    name: md.name.clone(),
                    // We don't have the original position from the diff
                    // alone. In a real pipeline the caller would provide it.
                    // For markers, the diff stores the id + name which is
                    // enough identity to detect duplicates; position should
                    // come from the source project.
                    //
                    // As a pragmatic default we set position = offset (start
                    // of the song region in the setlist).
                    position: offset,
                    end_position: if md.is_region { Some(offset) } else { None },
                    color: 0,
                    flags: if md.is_region { 1 } else { 0 },
                    locked: 0,
                    guid: String::new(),
                    additional: 0,
                    lane: None,
                    beat_position: None,
                };
                target.markers_regions.add(marker);
            }
            ChangeKind::Modified => {
                // Find the marker/region in the target by (is_region, id)
                // and apply property changes.
                let entry = if md.is_region {
                    target
                        .markers_regions
                        .regions
                        .iter_mut()
                        .find(|m| m.id == md.id)
                } else {
                    target
                        .markers_regions
                        .markers
                        .iter_mut()
                        .find(|m| m.id == md.id)
                };

                if let Some(m) = entry {
                    for change in &md.property_changes {
                        match change.field.as_str() {
                            "name" => {
                                // Update to match the song version (old_value).
                                m.name = change.old_value.clone();
                            }
                            "position" => {
                                if let Ok(v) = change.old_value.parse::<f64>() {
                                    m.position = v + offset;
                                }
                            }
                            "end_position" => {
                                if change.old_value == "none" {
                                    m.end_position = None;
                                } else if let Ok(v) = change.old_value.parse::<f64>() {
                                    m.end_position = Some(v + offset);
                                }
                            }
                            "color" => {
                                if let Ok(v) = change.old_value.parse::<i32>() {
                                    m.color = v;
                                }
                            }
                            _ => {}
                        }
                    }

                    // Also update the entry in the `all` vec.
                    if let Some(all_entry) = target
                        .markers_regions
                        .all
                        .iter_mut()
                        .find(|a| a.id == md.id && a.is_region() == md.is_region)
                    {
                        *all_entry = m.clone();
                    }
                }
            }
        }
    }
}

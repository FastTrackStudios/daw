//! MIDI source diffing — two-pointer merge on absolute tick positions.

use super::types::*;
use crate::types::{MidiEvent, MidiSource};

/// Diff two MIDI sources. Returns None if both are None or identical.
pub(crate) fn diff_midi_sources(
    old: Option<&MidiSource>,
    new: Option<&MidiSource>,
) -> Option<MidiDiff> {
    match (old, new) {
        (None, None) => None,
        (None, Some(_)) => Some(MidiDiff {
            property_changes: vec![PropertyChange {
                field: "has_data".into(),
                old_value: "false".into(),
                new_value: "true".into(),
            }],
            event_changes: Vec::new(),
        }),
        (Some(_), None) => Some(MidiDiff {
            property_changes: vec![PropertyChange {
                field: "has_data".into(),
                old_value: "true".into(),
                new_value: "false".into(),
            }],
            event_changes: Vec::new(),
        }),
        (Some(old_midi), Some(new_midi)) => {
            // If pooled and same GUID, no event-level diff needed
            if let (Some(old_guid), Some(new_guid)) =
                (&old_midi.pooled_evts_guid, &new_midi.pooled_evts_guid)
                && old_guid == new_guid
            {
                return None;
            }

            let mut prop_changes = Vec::new();
            if old_midi.ticks_per_qn != new_midi.ticks_per_qn {
                prop_changes.push(PropertyChange {
                    field: "ticks_per_qn".into(),
                    old_value: old_midi.ticks_per_qn.to_string(),
                    new_value: new_midi.ticks_per_qn.to_string(),
                });
            }

            let event_changes = diff_midi_events(&old_midi.events, &new_midi.events);

            if prop_changes.is_empty() && event_changes.is_empty() {
                None
            } else {
                Some(MidiDiff {
                    property_changes: prop_changes,
                    event_changes,
                })
            }
        }
    }
}

/// Two-pointer merge on MIDI events using absolute tick positions.
///
/// Converts delta ticks to absolute, then merges like envelope points.
/// Events at the same tick are matched by bytes content.
fn diff_midi_events(old: &[MidiEvent], new: &[MidiEvent]) -> Vec<MidiEventChange> {
    let old_abs = to_absolute(old);
    let new_abs = to_absolute(new);

    let mut changes = Vec::new();
    let mut i = 0;
    let mut j = 0;

    while i < old_abs.len() && j < new_abs.len() {
        let (ot, ob) = &old_abs[i];
        let (nt, nb) = &new_abs[j];

        match ot.cmp(nt) {
            std::cmp::Ordering::Equal => {
                if ob != nb {
                    // Same tick, different bytes — treat as remove + add
                    changes.push(MidiEventChange::Removed {
                        absolute_tick: *ot,
                        bytes: ob.clone(),
                    });
                    changes.push(MidiEventChange::Added {
                        absolute_tick: *nt,
                        bytes: nb.clone(),
                    });
                }
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => {
                changes.push(MidiEventChange::Removed {
                    absolute_tick: *ot,
                    bytes: ob.clone(),
                });
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                changes.push(MidiEventChange::Added {
                    absolute_tick: *nt,
                    bytes: nb.clone(),
                });
                j += 1;
            }
        }
    }

    while i < old_abs.len() {
        let (t, b) = &old_abs[i];
        changes.push(MidiEventChange::Removed {
            absolute_tick: *t,
            bytes: b.clone(),
        });
        i += 1;
    }
    while j < new_abs.len() {
        let (t, b) = &new_abs[j];
        changes.push(MidiEventChange::Added {
            absolute_tick: *t,
            bytes: b.clone(),
        });
        j += 1;
    }

    changes
}

/// Convert delta-tick MIDI events to (absolute_tick, bytes) pairs.
fn to_absolute(events: &[MidiEvent]) -> Vec<(u64, Vec<u8>)> {
    let mut abs = Vec::with_capacity(events.len());
    let mut tick: u64 = 0;
    for e in events {
        tick += e.delta_ticks as u64;
        abs.push((tick, e.bytes.clone()));
    }
    abs
}

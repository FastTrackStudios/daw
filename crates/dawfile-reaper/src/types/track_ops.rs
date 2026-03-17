//! Track organization operations — folder wrapping, grouping, and reordering.
//!
//! These operate on flat `Vec<Track>` (REAPER's track list representation)
//! and manipulate `ISBUS` / folder settings to organize tracks into folders.

use super::track::{FolderSettings, FolderState, Track};

/// Wrap a set of tracks inside a new folder track.
///
/// Creates a folder track with the given name, places all provided tracks
/// inside it, and sets the last track to close the folder. Handles tracks
/// that already have folder structure by adjusting the final close depth.
///
/// # Example
///
/// ```ignore
/// let tracks = vec![drums_track, bass_track, guitar_track];
/// let wrapped = wrap_in_folder("Band", tracks);
/// // Result: [Band (folder), Drums, Bass, Guitar (closes folder)]
/// ```
pub fn wrap_in_folder(folder_name: &str, mut tracks: Vec<Track>) -> Vec<Track> {
    if tracks.is_empty() {
        return vec![make_folder_track(folder_name)];
    }

    // Calculate the net folder depth of the tracks we're wrapping.
    // If they have internal folder structure, the last track's close
    // needs to account for it.
    let net_depth: i32 = tracks
        .iter()
        .map(|t| t.folder.as_ref().map_or(0, |f| f.indentation))
        .sum();

    // The last track must close our new folder plus any unclosed inner folders
    let close_depth = -1 - net_depth;
    let last = tracks.last_mut().unwrap();
    last.folder = Some(FolderSettings {
        folder_state: FolderState::LastInFolder,
        indentation: close_depth,
    });

    let mut result = Vec::with_capacity(tracks.len() + 1);
    result.push(make_folder_track(folder_name));
    result.extend(tracks);
    result
}

/// Extract tracks at the given indices from a track list and wrap them
/// in a new folder, inserting the folder at the position of the first
/// extracted track.
///
/// Indices must be valid and non-empty. Tracks are removed from their
/// original positions and replaced with the folder group.
pub fn group_into_folder(tracks: &mut Vec<Track>, folder_name: &str, indices: &[usize]) {
    if indices.is_empty() {
        return;
    }

    let mut sorted_indices: Vec<usize> = indices.to_vec();
    sorted_indices.sort_unstable();
    sorted_indices.dedup();

    let insert_pos = sorted_indices[0];

    // Extract tracks in reverse order to preserve indices
    let mut extracted = Vec::with_capacity(sorted_indices.len());
    for &idx in sorted_indices.iter().rev() {
        if idx < tracks.len() {
            extracted.push(tracks.remove(idx));
        }
    }
    extracted.reverse();

    let wrapped = wrap_in_folder(folder_name, extracted);
    for (i, track) in wrapped.into_iter().enumerate() {
        tracks.insert(insert_pos + i, track);
    }
}

/// Group tracks matching a predicate into a new folder.
///
/// Scans the track list, collects all tracks where `predicate` returns true,
/// removes them, wraps them in a folder, and inserts at the position of
/// the first matched track.
pub fn group_by_predicate(
    tracks: &mut Vec<Track>,
    folder_name: &str,
    predicate: impl Fn(&Track) -> bool,
) {
    let indices: Vec<usize> = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| predicate(t))
        .map(|(i, _)| i)
        .collect();

    group_into_folder(tracks, folder_name, &indices);
}

/// Group tracks by name into a new folder.
///
/// Convenience wrapper around [`group_by_predicate`] that matches
/// track names exactly (case-sensitive).
pub fn group_by_names(tracks: &mut Vec<Track>, folder_name: &str, names: &[&str]) {
    group_by_predicate(tracks, folder_name, |t| names.contains(&t.name.as_str()));
}

/// Move tracks at the given indices into an existing folder track.
///
/// The `folder_idx` must point to a track with `FolderState::FolderParent`.
/// The tracks are removed from their current positions and inserted as
/// children of the folder (before the folder's closing track).
pub fn move_into_existing_folder(
    tracks: &mut Vec<Track>,
    folder_idx: usize,
    track_indices: &[usize],
) {
    if track_indices.is_empty() || folder_idx >= tracks.len() {
        return;
    }

    // Verify the target is a folder
    let is_folder = tracks[folder_idx]
        .folder
        .as_ref()
        .map_or(false, |f| f.folder_state == FolderState::FolderParent);
    if !is_folder {
        return;
    }

    // Find the folder's closing track by scanning forward for matching depth
    let mut depth = 1i32;
    let mut close_idx = tracks.len();
    for i in (folder_idx + 1)..tracks.len() {
        let indent = tracks[i].folder.as_ref().map_or(0, |f| f.indentation);
        depth += indent;
        if depth <= 0 {
            close_idx = i;
            break;
        }
    }

    // Filter out indices already inside the folder or the folder itself
    let mut sorted: Vec<usize> = track_indices.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    sorted.retain(|&idx| idx != folder_idx && (idx <= folder_idx || idx >= close_idx));

    if sorted.is_empty() {
        return;
    }

    // Extract tracks in reverse order
    let mut extracted = Vec::with_capacity(sorted.len());
    for &idx in sorted.iter().rev() {
        if idx < tracks.len() {
            extracted.push(tracks.remove(idx));
        }
    }
    extracted.reverse();

    // Re-find the folder close position (indices shifted after removals)
    let folder_pos = tracks
        .iter()
        .position(|t| std::ptr::eq(t, t) && false) // dummy — find by re-scanning
        .unwrap_or(0);

    // Re-scan: find the folder by scanning for our folder name + FolderParent
    // (indices have shifted, so we re-scan)
    let mut insert_at = tracks.len();
    for (i, t) in tracks.iter().enumerate() {
        if t.folder
            .as_ref()
            .map_or(false, |f| f.folder_state == FolderState::FolderParent)
            && i <= folder_idx
        {
            // Found a potential folder — scan for its close
            let mut d = 1i32;
            for j in (i + 1)..tracks.len() {
                let ind = tracks[j].folder.as_ref().map_or(0, |f| f.indentation);
                d += ind;
                if d <= 0 {
                    insert_at = j;
                    break;
                }
            }
            break;
        }
    }

    // Insert extracted tracks before the close track
    for (j, track) in extracted.into_iter().enumerate() {
        tracks.insert(insert_at + j, track);
    }
}

fn make_folder_track(name: &str) -> Track {
    Track {
        name: name.to_string(),
        folder: Some(FolderSettings {
            folder_state: FolderState::FolderParent,
            indentation: 1,
        }),
        ..Track::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str) -> Track {
        Track {
            name: name.to_string(),
            ..Track::default()
        }
    }

    fn folder_trk(name: &str) -> Track {
        Track {
            name: name.to_string(),
            folder: Some(FolderSettings {
                folder_state: FolderState::FolderParent,
                indentation: 1,
            }),
            ..Track::default()
        }
    }

    fn last_in(name: &str, depth: i32) -> Track {
        Track {
            name: name.to_string(),
            folder: Some(FolderSettings {
                folder_state: FolderState::LastInFolder,
                indentation: depth,
            }),
            ..Track::default()
        }
    }

    fn names(tracks: &[Track]) -> Vec<&str> {
        tracks.iter().map(|t| t.name.as_str()).collect()
    }

    fn is_folder_parent(t: &Track) -> bool {
        t.folder
            .as_ref()
            .map_or(false, |f| f.folder_state == FolderState::FolderParent)
    }

    fn is_last_in_folder(t: &Track) -> bool {
        t.folder
            .as_ref()
            .map_or(false, |f| f.folder_state == FolderState::LastInFolder)
    }

    fn indent(t: &Track) -> i32 {
        t.folder.as_ref().map_or(0, |f| f.indentation)
    }

    #[test]
    fn wrap_simple_tracks() {
        let tracks = vec![track("Drums"), track("Bass"), track("Guitar")];
        let result = wrap_in_folder("Band", tracks);

        assert_eq!(names(&result), vec!["Band", "Drums", "Bass", "Guitar"]);
        assert!(is_folder_parent(&result[0]));
        assert!(is_last_in_folder(&result[3]));
        assert_eq!(indent(&result[3]), -1);
    }

    #[test]
    fn wrap_empty() {
        let result = wrap_in_folder("Empty", vec![]);
        assert_eq!(result.len(), 1);
        assert!(is_folder_parent(&result[0]));
    }

    #[test]
    fn wrap_single_track() {
        let result = wrap_in_folder("Solo", vec![track("Vocals")]);
        assert_eq!(names(&result), vec!["Solo", "Vocals"]);
        assert!(is_folder_parent(&result[0]));
        assert!(is_last_in_folder(&result[1]));
        assert_eq!(indent(&result[1]), -1);
    }

    #[test]
    fn wrap_tracks_with_existing_folder_structure() {
        let tracks = vec![folder_trk("Sub"), track("A"), last_in("B", -1)];
        let result = wrap_in_folder("Outer", tracks);

        assert_eq!(names(&result), vec!["Outer", "Sub", "A", "B"]);
        assert!(is_folder_parent(&result[0]));
        // Net inner depth: +1 + 0 + (-1) = 0, so last track closes just our folder
        assert!(is_last_in_folder(&result[3]));
        assert_eq!(indent(&result[3]), -1);
    }

    #[test]
    fn group_by_indices() {
        let mut tracks = vec![
            track("Click"),
            track("Drums"),
            track("Bass"),
            track("Guitar"),
            track("Vocals"),
        ];
        group_into_folder(&mut tracks, "Rhythm", &[1, 2]);

        assert_eq!(
            names(&tracks),
            vec!["Click", "Rhythm", "Drums", "Bass", "Guitar", "Vocals"]
        );
        assert!(is_folder_parent(&tracks[1]));
        assert!(is_last_in_folder(&tracks[3]));
    }

    #[test]
    fn group_by_name_predicate() {
        let mut tracks = vec![
            track("Drums"),
            track("Bass"),
            track("Guitar"),
            track("Vocals"),
            track("Piano"),
        ];
        group_by_names(&mut tracks, "Band", &["Drums", "Bass", "Guitar"]);

        assert_eq!(
            names(&tracks),
            vec!["Band", "Drums", "Bass", "Guitar", "Vocals", "Piano"]
        );
        assert!(is_folder_parent(&tracks[0]));
        assert!(is_last_in_folder(&tracks[3]));
        assert_eq!(indent(&tracks[3]), -1);
    }

    #[test]
    fn group_non_contiguous_tracks() {
        let mut tracks = vec![
            track("Drums"),
            track("Click"),
            track("Bass"),
            track("Guide"),
            track("Guitar"),
        ];
        group_into_folder(&mut tracks, "Utility", &[1, 3]);

        assert_eq!(
            names(&tracks),
            vec!["Drums", "Utility", "Click", "Guide", "Bass", "Guitar"]
        );
        assert!(is_folder_parent(&tracks[1]));
        assert!(is_last_in_folder(&tracks[3]));
    }

    #[test]
    fn move_into_existing() {
        let mut tracks = vec![
            folder_trk("Band"),
            track("Drums"),
            last_in("Bass", -1),
            track("Click"),
            track("Guide"),
        ];

        move_into_existing_folder(&mut tracks, 0, &[3, 4]);

        // Click and Guide should now be inside Band folder, before the close
        assert_eq!(
            names(&tracks),
            vec!["Band", "Drums", "Click", "Guide", "Bass"]
        );
    }
}

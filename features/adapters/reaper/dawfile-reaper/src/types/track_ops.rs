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
// r[impl track-ops.wrap-in-folder]
pub fn wrap_in_folder(folder_name: &str, mut tracks: Vec<Track>) -> Vec<Track> {
    if tracks.is_empty() {
        return vec![make_folder_track(folder_name)];
    }

    // Calculate the net folder depth of the tracks we're wrapping,
    // EXCLUDING the last track (since we'll overwrite its folder settings).
    // This tells us how many folders are still open after all but the last track.
    let net_depth_without_last: i32 = tracks[..tracks.len() - 1]
        .iter()
        .map(|t| t.folder.as_ref().map_or(0, |f| f.indentation))
        .sum();

    // The last track must close: our new folder (-1) plus any unclosed inner folders
    let close_depth = -1 - net_depth_without_last;
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
// r[impl track-ops.group-into-folder]
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
// r[impl track-ops.group-by-predicate]
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
// r[impl track-ops.move-into-existing]
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
        .is_some_and(|f| f.folder_state == FolderState::FolderParent);
    if !is_folder {
        return;
    }

    // Find the folder's closing track by scanning forward for matching depth
    let mut depth = 1i32;
    let mut close_idx = tracks.len();
    for (i, track) in tracks.iter().enumerate().skip(folder_idx + 1) {
        let indent = track.folder.as_ref().map_or(0, |f| f.indentation);
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
    // Re-scan: find the folder by scanning for our folder name + FolderParent
    // (indices have shifted, so we re-scan)
    let mut insert_at = tracks.len();
    for (i, t) in tracks.iter().enumerate() {
        if t.folder
            .as_ref()
            .is_some_and(|f| f.folder_state == FolderState::FolderParent)
            && i <= folder_idx
        {
            // Found a potential folder — scan for its close
            let mut d = 1i32;
            for (j, track) in tracks.iter().enumerate().skip(i + 1) {
                let ind = track.folder.as_ref().map_or(0, |f| f.indentation);
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

// ── FTS Project Hierarchy ─────────────────────────────────────────────────────

/// Well-known guide track names.
const GUIDE_NAMES: &[&str] = &["Click", "Loop", "Count", "Guide"];

/// Well-known Keyflow track names.
const KEYFLOW_NAMES: &[&str] = &["CHORDS", "LINES", "HITS"];

/// Well-known structural folder names to strip (we rebuild these).
const STRUCTURAL_FOLDERS: &[&str] = &[
    "click/guide",
    "click + guide",
    "tracks",
    "keyflow",
    "midi bus",
    "reference",
];

/// Classify a track's role in the FTS hierarchy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackRole {
    /// Click, Loop, Count, Guide
    Guide,
    /// CHORDS, LINES, HITS
    Keyflow,
    /// Structural folder to be stripped (Click+Guide, TRACKS, Keyflow, Reference)
    Structural,
    /// Reference mix track (mp3, full song bounce)
    Mix,
    /// Stem split track (demucs/lalal output)
    StemSplit,
    /// Stem Split folder marker
    StemSplitFolder,
    /// Regular content track
    Content,
}

/// Classify a track into its FTS role.
pub fn classify_track(track: &Track) -> TrackRole {
    let name = &track.name;
    let lower = name.to_lowercase();

    // Structural folders to strip
    let is_folder = track
        .folder
        .as_ref()
        .is_some_and(|f| f.folder_state == FolderState::FolderParent);
    if is_folder && STRUCTURAL_FOLDERS.iter().any(|s| lower == *s) {
        return TrackRole::Structural;
    }

    // Guide tracks
    if GUIDE_NAMES.iter().any(|g| g.to_lowercase() == lower) {
        return TrackRole::Guide;
    }

    // Keyflow tracks
    if KEYFLOW_NAMES.iter().any(|k| k.to_lowercase() == lower) {
        return TrackRole::Keyflow;
    }

    // Stem Split folder
    if lower == "stem split" && is_folder {
        return TrackRole::StemSplitFolder;
    }

    // Stem split tracks — items with stem-like names or tracks inside Stem Split
    if lower == "stem split" {
        return TrackRole::StemSplit;
    }

    // Reference/Mix detection — mp3 files, "mix", "reference", "bounce"
    let is_mix_name = lower.contains(".mp3")
        || lower == "mix"
        || lower == "reference"
        || lower == "bounce"
        || lower == "master";

    // Check if the track has items that look like stems (demucs patterns).
    // Checks both parsed source.file_path AND raw_content FILE lines,
    // since many items only have their file info in raw_content.
    let stem_patterns = [
        "(drums)", "(bass)", "(vocals)", "(guitar)", "(piano)", "(other)",
    ];
    let has_stem_items = track.items.iter().any(|item| {
        // Check parsed sources
        let in_parsed = item.takes.iter().any(|take| {
            if let Some(ref source) = take.source {
                let fp = source.file_path.to_lowercase();
                stem_patterns.iter().any(|p| fp.contains(p))
            } else {
                false
            }
        });
        // Check raw_content FILE lines
        let in_raw = item.raw_content.lines().any(|line| {
            let trimmed = line.trim().to_lowercase();
            trimmed.starts_with("file ") && stem_patterns.iter().any(|p| trimmed.contains(p))
        });
        in_parsed || in_raw
    });

    // Check if items are mp3/reference sources
    let has_mp3_items = track.items.iter().any(|item| {
        let in_parsed = item.takes.iter().any(|take| {
            if let Some(ref source) = take.source {
                let fp = source.file_path.to_lowercase();
                fp.ends_with(".mp3") || fp.contains("mix") || fp.contains("bounce")
            } else {
                false
            }
        });
        let in_raw = item.raw_content.lines().any(|line| {
            let trimmed = line.trim().to_lowercase();
            trimmed.starts_with("file ")
                && (trimmed.ends_with(".mp3\"")
                    || trimmed.contains("mix")
                    || trimmed.contains("bounce"))
        });
        in_parsed || in_raw
    });

    if has_stem_items {
        return TrackRole::StemSplit;
    }

    if is_mix_name || has_mp3_items {
        return TrackRole::Mix;
    }

    // Empty tracks (no items, not a folder) — treat as reference placeholder
    if track.items.is_empty() && !is_folder {
        return TrackRole::Mix;
    }

    TrackRole::Content
}

/// Organize a flat list of tracks into the canonical FTS project hierarchy.
///
/// Produces:
/// ```text
/// Click + Guide/
/// ├── Click, Loop, Count, Guide
/// Keyflow/
/// ├── CHORDS, LINES, HITS
/// TRACKS/
/// ├── (content tracks)
/// Reference/
/// ├── Mix
/// └── Stem Split/
///     ├── Drums, Bass, Vocals, ...
/// ```
///
/// Structural folders from the source project are stripped and rebuilt.
/// Tracks are classified by name and content into the correct category.
// r[impl track-ops.fts-hierarchy]
pub fn organize_into_fts_hierarchy(tracks: Vec<Track>) -> Vec<Track> {
    let mut guide: Vec<Track> = Vec::new();
    let mut keyflow: Vec<Track> = Vec::new();
    let mut content: Vec<Track> = Vec::new();
    let mut mix: Vec<Track> = Vec::new();
    let mut stems: Vec<Track> = Vec::new();

    // Track folder context to detect tracks inside Reference/Stem Split folders
    let mut in_reference = false;
    let mut in_stem_split = false;
    let mut ref_depth = 0i32;
    let mut stem_depth = 0i32;

    for track in tracks {
        let role = classify_track(&track);

        // Track folder context
        let lower = track.name.to_lowercase();
        if lower == "reference"
            && track
                .folder
                .as_ref()
                .is_some_and(|f| f.folder_state == FolderState::FolderParent)
        {
            in_reference = true;
            ref_depth = 1;
            continue; // Skip the folder track itself
        }
        if (lower == "stem split")
            && track
                .folder
                .as_ref()
                .is_some_and(|f| f.folder_state == FolderState::FolderParent)
        {
            in_stem_split = true;
            stem_depth = 1;
            continue;
        }

        // If inside stem split folder, all children are stems
        if in_stem_split {
            let indent = track.folder.as_ref().map_or(0, |f| f.indentation);
            stem_depth += indent;
            if stem_depth <= 0 {
                in_stem_split = false;
            }
            let mut t = track;
            t.folder = None;
            stems.push(t);
            continue;
        }

        // If inside reference folder, classify children
        if in_reference {
            let indent = track.folder.as_ref().map_or(0, |f| f.indentation);
            ref_depth += indent;
            if ref_depth <= 0 {
                in_reference = false;
            }
            let mut t = track;
            t.folder = None;
            match role {
                TrackRole::StemSplit => stems.push(t),
                _ => mix.push(t),
            }
            continue;
        }

        match role {
            TrackRole::Structural => continue,
            TrackRole::Guide => {
                let mut t = track;
                t.folder = None;
                guide.push(t);
            }
            TrackRole::Keyflow => {
                let mut t = track;
                t.folder = None;
                keyflow.push(t);
            }
            TrackRole::Mix => {
                let mut t = track;
                t.folder = None;
                mix.push(t);
            }
            TrackRole::StemSplit | TrackRole::StemSplitFolder => {
                let mut t = track;
                t.folder = None;
                stems.push(t);
            }
            TrackRole::Content => {
                let mut t = track;
                t.folder = None;
                content.push(t);
            }
        }
    }

    // Build the hierarchy
    let mut result: Vec<Track> = Vec::new();

    // Click + Guide folder — always present with standard tracks
    if guide.is_empty() {
        for name in GUIDE_NAMES {
            guide.push(Track {
                name: name.to_string(),
                ..Track::default()
            });
        }
    }
    result.extend(wrap_in_folder("Click + Guide", guide));

    // Keyflow folder — always present with standard tracks
    if keyflow.is_empty() {
        for name in KEYFLOW_NAMES {
            keyflow.push(Track {
                name: name.to_string(),
                ..Track::default()
            });
        }
    }
    result.extend(wrap_in_folder("Keyflow", keyflow));
    // TRACKS folder is always present
    if content.is_empty() {
        // Empty folder — just the folder track itself
        result.push(make_folder_track("TRACKS"));
        result.push(Track {
            name: String::new(),
            folder: Some(FolderSettings {
                folder_state: FolderState::LastInFolder,
                indentation: -1,
            }),
            ..Track::default()
        });
    } else {
        result.extend(wrap_in_folder("TRACKS", content));
    }

    // Reference section
    let mut ref_children: Vec<Track> = Vec::new();
    ref_children.extend(mix);
    if !stems.is_empty() {
        ref_children.extend(wrap_in_folder("Stem Split", stems));
    }
    if !ref_children.is_empty() {
        result.extend(wrap_in_folder("Reference", ref_children));
    }

    result
}

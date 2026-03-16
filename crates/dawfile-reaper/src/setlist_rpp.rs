//! Setlist RPP generation — concatenate multiple song projects into a single
//! REAPER project with all songs on a shared timeline.
//!
//! Takes `Vec<ReaperProject>` + song info (names, offsets), produces a merged
//! `ReaperProject` with shared guide tracks, per-song folders, concatenated
//! tempo, and offset markers/regions.

use std::path::{Path, PathBuf};

use crate::types::item::Item;
use crate::types::marker_region::{MarkerRegion, MarkerRegionCollection};
use crate::types::project::ReaperProject;
use crate::types::time_tempo::{TempoTimeEnvelope, TempoTimePoint};
use crate::types::track::{FolderSettings, FolderState, Track};

/// Information about a song for concatenation.
pub struct SongInfo {
    /// Song name — used as the folder name in TRACKS/.
    pub name: String,
    /// Time offset: all item positions shift by this amount.
    pub global_start_seconds: f64,
    /// Duration of the song in seconds.
    pub duration_seconds: f64,
}

/// Build `SongInfo` entries from song names and durations, with a configurable
/// gap (in seconds) inserted between each song.
///
/// The gap provides breathing room between songs in the combined timeline
/// for clean transitions and visual separation.
pub fn build_song_infos(
    songs: &[(&str, f64)],
    gap_seconds: f64,
) -> Vec<SongInfo> {
    let mut result = Vec::with_capacity(songs.len());
    let mut offset = 0.0;

    for (i, (name, duration)) in songs.iter().enumerate() {
        result.push(SongInfo {
            name: name.to_string(),
            global_start_seconds: offset,
            duration_seconds: *duration,
        });
        offset += duration;
        if i < songs.len() - 1 {
            offset += gap_seconds; // gap between songs
        }
    }

    result
}

/// Compute the gap duration in seconds for N measures at a given tempo/time sig.
///
/// For example, 2 measures at 120 BPM in 4/4 = 2 * (4 beats * 60/120) = 4 seconds.
pub fn measures_to_seconds(measures: u32, bpm: f64, beats_per_measure: u32) -> f64 {
    let beat_duration = 60.0 / bpm;
    measures as f64 * beats_per_measure as f64 * beat_duration
}

// ── Track Concatenation (US-004) ─────────────────────────────────────────────

// ── RPL File Parsing ─────────────────────────────────────────────────────────

/// Parse an `.RPL` file (REAPER Project List).
///
/// Each non-empty line is a path to an RPP file. Relative paths are resolved
/// against the RPL file's parent directory.
pub fn parse_rpl(rpl_path: &Path) -> std::io::Result<Vec<PathBuf>> {
    let content = std::fs::read_to_string(rpl_path)?;
    let parent = rpl_path.parent().unwrap_or(Path::new("."));
    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let path = PathBuf::from(line.trim());
            if path.is_absolute() {
                path
            } else {
                parent.join(path)
            }
        })
        .collect())
}

/// Extract a song name from an RPP file path.
///
/// Strips the extension and any trailing bracketed content
/// (e.g., `"Belief - John Mayer [Battle SP26].RPP"` → `"Belief - John Mayer"`).
pub fn song_name_from_path(path: &Path) -> String {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let name = stem.split('[').next().unwrap_or(&stem).trim();
    name.to_string()
}

// ── Track Concatenation (US-004) ─────────────────────────────────────────────

// ── Song Bounds Resolution ───────────────────────────────────────────────────

/// Resolved bounds for a song — the full range to include in the combined setlist.
#[derive(Debug, Clone)]
pub struct SongBounds {
    /// Start of the song's allocated range (seconds, relative to project start).
    pub start: f64,
    /// End of the song's allocated range.
    pub end: f64,
    /// Position of the last marker/region endpoint in the song.
    pub last_marker_position: f64,
}

/// Resolve the full bounds of a song from its markers.
///
/// Priority (outermost wins):
/// 1. PREROLL → POSTROLL
/// 2. COUNT-IN → POSTROLL (or =END if no POSTROLL)
/// 3. =START → =END
/// 4. SONGSTART → SONGEND
/// 5. First section region → last section region end
/// 6. 0 → last marker position
pub fn resolve_song_bounds(project: &ReaperProject) -> SongBounds {
    let markers = &project.markers_regions.all;

    let mut preroll: Option<f64> = None;
    let mut postroll: Option<f64> = None;
    let mut count_in: Option<f64> = None;
    let mut eq_start: Option<f64> = None;
    let mut eq_end: Option<f64> = None;
    let mut songstart: Option<f64> = None;
    let mut songend: Option<f64> = None;
    let mut last_pos: f64 = 0.0;
    let mut first_section_start: Option<f64> = None;
    let mut last_section_end: Option<f64> = None;

    for m in markers {
        let name_upper = m.name.to_uppercase();

        let end_pos = m.end_position.unwrap_or(m.position);
        if end_pos > last_pos { last_pos = end_pos; }
        if m.position > last_pos { last_pos = m.position; }

        match name_upper.as_str() {
            "PREROLL" | "=PREROLL" => preroll = Some(m.position),
            "POSTROLL" | "=POSTROLL" => postroll = Some(m.position),
            "COUNT-IN" | "COUNT IN" | "COUNTIN" => count_in = Some(m.position),
            "=START" => eq_start = Some(m.position),
            "=END" => eq_end = Some(m.position),
            "SONGSTART" => songstart = Some(m.position),
            "SONGEND" => songend = Some(m.position),
            _ => {
                if m.is_region() && !m.name.is_empty() {
                    if first_section_start.is_none() || m.position < first_section_start.unwrap() {
                        first_section_start = Some(m.position);
                    }
                    if let Some(end) = m.end_position {
                        if last_section_end.is_none() || end > last_section_end.unwrap() {
                            last_section_end = Some(end);
                        }
                    }
                }
            }
        }
    }

    let start = preroll
        .or(count_in)
        .or(eq_start)
        .or(songstart)
        .or(first_section_start)
        .unwrap_or(0.0);

    let end = postroll
        .or(eq_end)
        .or(songend)
        .or(last_section_end)
        .unwrap_or(last_pos);

    SongBounds {
        start,
        end: end.max(start + 0.1),
        last_marker_position: last_pos,
    }
}

/// Build `SongInfo` entries from parsed projects using resolved bounds + gap.
pub fn build_song_infos_from_projects(
    projects: &[ReaperProject],
    names: &[&str],
    gap_seconds: f64,
) -> Vec<SongInfo> {
    assert_eq!(projects.len(), names.len());
    let mut result = Vec::with_capacity(projects.len());
    let mut offset = 0.0;

    for (i, (project, name)) in projects.iter().zip(names.iter()).enumerate() {
        let bounds = resolve_song_bounds(project);
        let duration = bounds.end - bounds.start;

        result.push(SongInfo {
            name: name.to_string(),
            global_start_seconds: offset,
            duration_seconds: duration,
        });

        offset += duration;
        if i < projects.len() - 1 {
            offset += gap_seconds;
        }
    }

    result
}

// ── Track Concatenation (US-004) ─────────────────────────────────────────────

/// Well-known guide track names that get merged across songs.
const GUIDE_TRACK_NAMES: &[&str] = &["Click", "Loop", "Count", "Guide"];

/// Concatenate tracks from multiple projects into a single track list.
///
/// Guide tracks (Click, Loop, Count, Guide) are merged — items from all songs
/// placed on shared tracks at correct time offsets. Content tracks appear
/// under `TRACKS/{Song Name}/` folder hierarchy.
pub fn concatenate_tracks(projects: &[ReaperProject], songs: &[SongInfo]) -> Vec<Track> {
    assert_eq!(projects.len(), songs.len());

    // Accumulate guide track items across all songs
    let mut guide_items: Vec<Vec<Item>> = vec![vec![]; GUIDE_TRACK_NAMES.len()];

    // Collect per-song content tracks
    struct SongContent {
        tracks: Vec<Track>,
    }
    let mut song_contents: Vec<SongContent> = Vec::new();

    for (project, song) in projects.iter().zip(songs.iter()) {
        let offset = song.global_start_seconds;
        let mut content = SongContent { tracks: vec![] };

        for track in &project.tracks {
            let name_lower = track.name.to_lowercase();

            // Merge guide track items onto shared tracks
            if let Some(idx) = GUIDE_TRACK_NAMES
                .iter()
                .position(|g| g.to_lowercase() == name_lower)
            {
                for item in &track.items {
                    let mut offset_item = item.clone();
                    offset_item.position += offset;
                    guide_items[idx].push(offset_item);
                }
                continue;
            }

            // Skip structural folders from source projects
            if is_structural_folder(&name_lower, track) {
                continue;
            }

            // Content track — clone with offset items
            let mut cloned = clone_track_with_offset(track, offset);
            cloned.track_id = None; // New GUID on import
            content.tracks.push(cloned);
        }

        song_contents.push(content);
    }

    // Build the output track list
    let mut result: Vec<Track> = Vec::new();

    // Click/Guide folder
    result.push(make_folder_track("Click/Guide"));
    for (i, name) in GUIDE_TRACK_NAMES.iter().enumerate() {
        let mut t = empty_track(name);
        t.items = std::mem::take(&mut guide_items[i]);
        if i == GUIDE_TRACK_NAMES.len() - 1 {
            // Last guide track closes the folder
            t.folder = Some(FolderSettings {
                folder_state: FolderState::LastInFolder,
                indentation: -1,
            });
        }
        result.push(t);
    }

    // TRACKS folder with per-song subfolders
    result.push(make_folder_track("TRACKS"));

    for (i, (content, song)) in song_contents.iter().zip(songs.iter()).enumerate() {
        let is_last_song = i == songs.len() - 1;

        // Song subfolder
        result.push(make_folder_track(&song.name));

        if content.tracks.is_empty() {
            // Empty song — placeholder closes the folder
            let mut placeholder = empty_track("(empty)");
            placeholder.folder = Some(FolderSettings {
                folder_state: FolderState::LastInFolder,
                indentation: if is_last_song { -2 } else { -1 },
            });
            result.push(placeholder);
        } else {
            for (j, track) in content.tracks.iter().enumerate() {
                let mut t = track.clone();
                if j == content.tracks.len() - 1 {
                    // Last track in song closes the song folder
                    // (and TRACKS folder if this is the last song)
                    let depth = if is_last_song { -2 } else { -1 };
                    t.folder = Some(FolderSettings {
                        folder_state: FolderState::LastInFolder,
                        indentation: depth,
                    });
                }
                result.push(t);
            }
        }
    }

    result
}

fn is_structural_folder(name_lower: &str, track: &Track) -> bool {
    let is_folder = track
        .folder
        .as_ref()
        .map_or(false, |f| f.folder_state == FolderState::FolderParent);
    (name_lower == "click/guide" || name_lower == "tracks") && is_folder
}

fn clone_track_with_offset(track: &Track, offset_seconds: f64) -> Track {
    let mut cloned = track.clone();
    for item in &mut cloned.items {
        item.position += offset_seconds;
    }
    cloned
}

fn empty_track(name: &str) -> Track {
    Track {
        name: name.to_string(),
        ..Track::default()
    }
}

fn make_folder_track(name: &str) -> Track {
    Track {
        name: name.to_string(),
        folder: Some(FolderSettings {
            folder_state: FolderState::FolderParent,
            indentation: 0,
        }),
        ..Track::default()
    }
}

// ── Tempo Envelope Concatenation (US-005) ────────────────────────────────────

/// Concatenate tempo envelopes from multiple projects.
///
/// Each song's tempo points are offset by `global_start_seconds`. A square-shape
/// boundary point is inserted at each song transition for clean tempo changes.
pub fn concatenate_tempo_envelopes(
    projects: &[ReaperProject],
    songs: &[SongInfo],
) -> TempoTimeEnvelope {
    assert_eq!(projects.len(), songs.len());

    let mut points: Vec<TempoTimePoint> = Vec::new();

    for (project, song) in projects.iter().zip(songs.iter()) {
        let offset = song.global_start_seconds;

        if let Some(ref envelope) = project.tempo_envelope {
            let mut last_tempo = envelope.default_tempo;

            for point in &envelope.points {
                let mut p = point.clone();
                p.position += offset;
                p.shape = 1; // Force square (instant) for combined setlists
                last_tempo = p.tempo;
                points.push(p);
            }

            // Insert a trailing tempo marker at the song's last marker position.
            // This "freezes" the tempo at the end of the song, preventing any
            // curve/interpolation from bleeding into the gap or next song.
            let bounds = resolve_song_bounds(project);
            let trailing_pos = bounds.last_marker_position + offset;
            if trailing_pos > points.last().map(|p| p.position).unwrap_or(0.0) {
                let mut trailing = TempoTimePoint::default();
                trailing.position = trailing_pos;
                trailing.tempo = last_tempo;
                trailing.shape = 1;
                points.push(trailing);
            }
        }
    }

    let (default_tempo, default_ts) = projects
        .first()
        .and_then(|p| p.tempo_envelope.as_ref())
        .map(|e| (e.default_tempo, e.default_time_signature))
        .unwrap_or((120.0, (4, 4)));

    TempoTimeEnvelope {
        points,
        default_tempo,
        default_time_signature: default_ts,
    }
}

// ── Region/Marker Generation (US-006) ────────────────────────────────────────

/// Generate regions and markers for the setlist project.
///
/// Creates a full-song region for each song, plus offset copies of each
/// song's internal markers/regions with re-numbered IDs.
pub fn generate_markers_regions(
    projects: &[ReaperProject],
    songs: &[SongInfo],
) -> MarkerRegionCollection {
    assert_eq!(projects.len(), songs.len());

    let mut all: Vec<MarkerRegion> = Vec::new();
    let mut next_id: i32 = 1;

    for (project, song) in projects.iter().zip(songs.iter()) {
        let offset = song.global_start_seconds;
        let bounds = resolve_song_bounds(project);

        // SONG lane region — spans the full song bounds + a tiny bit past the last marker.
        // The extra 0.1s ensures the region visually covers the trailing tempo marker.
        let song_region_end = offset + (bounds.last_marker_position - bounds.start) + 0.1;
        all.push(MarkerRegion {
            id: next_id,
            position: offset,
            name: song.name.clone(),
            color: 0,
            flags: 0,
            locked: 0,
            guid: String::new(),
            additional: 0,
            end_position: Some(song_region_end.max(offset + song.duration_seconds)),
            lane: Some(3), // SONG lane (index 3)
            beat_position: None,
        });
        next_id += 1;

        // Offset copies of internal markers/regions with lane classification
        for mr in &project.markers_regions.all {
            let mut cloned = mr.clone();
            cloned.position += offset;
            if let Some(ref mut end) = cloned.end_position {
                *end += offset;
            }
            cloned.id = next_id;
            cloned.guid = String::new();

            // Classify into the correct ruler lane if not already set
            if cloned.lane.is_none() || cloned.lane == Some(0) {
                cloned.lane = Some(classify_lane(&cloned.name, cloned.is_region()) as i32);
            }

            all.push(cloned);
            next_id += 1;
        }
    }

    let markers: Vec<MarkerRegion> = all.iter().filter(|m| m.is_marker()).cloned().collect();
    let regions: Vec<MarkerRegion> = all.iter().filter(|m| m.is_region()).cloned().collect();

    MarkerRegionCollection {
        all,
        markers,
        regions,
    }
}

// ── Full Project Concatenation ───────────────────────────────────────────────

/// Concatenate multiple REAPER projects into a single setlist project.
pub fn concatenate_projects(projects: &[ReaperProject], songs: &[SongInfo]) -> ReaperProject {
    assert!(!projects.is_empty());
    assert_eq!(projects.len(), songs.len());

    let mut combined = projects[0].clone();
    combined.tracks = concatenate_tracks(projects, songs);
    combined.tempo_envelope = Some(concatenate_tempo_envelopes(projects, songs));
    combined.markers_regions = generate_markers_regions(projects, songs);
    combined.items.clear();
    combined
}

// ── Shell Copy Generation ────────────────────────────────────────────────────

/// Generate a shell copy of a setlist project for a specific role.
///
/// A shell copy preserves the timeline structure (tempo, markers, regions,
/// ruler lanes) and the Click/Guide tracks, but strips all content tracks.
/// A placeholder folder for the role's own tracks is added.
///
/// This enables role-specific REAPER instances (Vocals, Guitar, Keys, etc.)
/// to share the same timeline and click track while having their own
/// independent track setup.
pub fn generate_shell_copy(master: &ReaperProject, role: &str) -> ReaperProject {
    let mut shell = master.clone();

    // Strip content tracks — keep only Click/Guide tracks
    let mut kept_tracks: Vec<Track> = Vec::new();
    let mut in_guide_folder = false;
    let mut guide_depth: i32 = 0;

    for track in &master.tracks {
        let name_lower = track.name.to_lowercase();
        let is_folder_start = track.folder.as_ref()
            .map_or(false, |f| f.folder_state == FolderState::FolderParent);
        let is_folder_end = track.folder.as_ref()
            .map_or(false, |f| f.folder_state == FolderState::LastInFolder);

        // Track the Click/Guide folder hierarchy
        if name_lower == "click/guide" && is_folder_start {
            in_guide_folder = true;
            guide_depth = 1;
            kept_tracks.push(track.clone());
            continue;
        }

        if in_guide_folder {
            kept_tracks.push(track.clone());
            if is_folder_start {
                guide_depth += 1;
            }
            if is_folder_end {
                guide_depth += track.folder.as_ref().map_or(0, |f| f.indentation);
                if guide_depth <= 0 {
                    in_guide_folder = false;
                }
            }
            continue;
        }

        // Also keep individual Click/Loop/Count/Guide tracks at the top level
        // (in case they're not inside a Click/Guide folder)
        if GUIDE_TRACK_NAMES.iter().any(|g| g.to_lowercase() == name_lower)
            && !is_folder_start
        {
            kept_tracks.push(track.clone());
            continue;
        }

        // Skip everything else (TRACKS folder, song folders, content tracks)
    }

    // Add a role folder for the performer's own tracks
    let mut role_folder = Track {
        name: role.to_string(),
        folder: Some(FolderSettings {
            folder_state: FolderState::FolderParent,
            indentation: 0,
        }),
        ..Track::default()
    };

    // Add a placeholder child track inside the role folder
    let mut placeholder = Track {
        name: format!("{} (add tracks here)", role),
        folder: Some(FolderSettings {
            folder_state: FolderState::LastInFolder,
            indentation: -1,
        }),
        ..Track::default()
    };

    kept_tracks.push(role_folder);
    kept_tracks.push(placeholder);

    shell.tracks = kept_tracks;

    // Clear items from the top-level items list (they're inside tracks)
    shell.items.clear();

    shell
}

/// Generate shell copies for multiple roles from a master setlist.
///
/// Returns a vec of (role_name, project) pairs.
pub fn generate_role_setlists(
    master: &ReaperProject,
    roles: &[&str],
) -> Vec<(String, ReaperProject)> {
    roles
        .iter()
        .map(|role| {
            let shell = generate_shell_copy(master, role);
            (role.to_string(), shell)
        })
        .collect()
}

/// Standard FTS roles for setlist shell copies.
pub const STANDARD_ROLES: &[&str] = &[
    "Vocals",
    "Guitar",
    "Guitar 2",
    "Keys",
    "Keys 2",
    "Bass",
    "Drums",
];

/// Write all role setlists to a directory.
///
/// Each file is named `{role} - {setlist_name}.RPP`.
/// Returns the paths of the written files.
pub fn write_role_setlists(
    master: &ReaperProject,
    roles: &[&str],
    setlist_name: &str,
    output_dir: &Path,
) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(output_dir)?;

    let role_projects = generate_role_setlists(master, roles);
    let mut paths = Vec::new();

    for (role, project) in &role_projects {
        let filename = format!("{} - {}.RPP", role, setlist_name);
        let path = output_dir.join(&filename);
        let rpp_text = project_to_rpp_text(project);
        std::fs::write(&path, &rpp_text)?;
        paths.push(path);
    }

    // Also write the master as "Tracks - {name}.RPP"
    let master_filename = format!("Tracks - {}.RPP", setlist_name);
    let master_path = output_dir.join(&master_filename);
    let master_text = project_to_rpp_text(master);
    std::fs::write(&master_path, &master_text)?;
    paths.insert(0, master_path);

    Ok(paths)
}

// ── RPP Serialization ────────────────────────────────────────────────────────

/// Write a combined `ReaperProject` to RPP text.
///
/// This produces a minimal but valid RPP that REAPER can open.
/// Track items, markers, regions, and tempo envelope are included.
pub fn project_to_rpp_text(project: &ReaperProject) -> String {
    let mut out = String::new();
    let tempo = project.tempo_envelope.as_ref()
        .map(|e| e.default_tempo)
        .unwrap_or(120.0);
    let (ts_num, ts_denom) = project.tempo_envelope.as_ref()
        .map(|e| e.default_time_signature)
        .unwrap_or((4, 4));

    out.push_str(&format!("<REAPER_PROJECT 0.1 \"7.0/generated\" 0\n"));
    out.push_str("  RIPPLE 0 0\n");
    out.push_str("  GROUPOVERRIDE 0 0 0 0\n");
    out.push_str("  AUTOXFADE 129\n");
    out.push_str("  ENVATTACH 3\n");
    out.push_str("  MIXERUIFLAGS 11 48\n");
    out.push_str("  PEAKGAIN 1\n");
    out.push_str("  FEEDBACK 0\n");
    out.push_str("  PANLAW 1\n");
    out.push_str("  PROJOFFS 0 0 0\n");
    out.push_str("  MAXPROJLEN 0 0\n");
    out.push_str("  GRID 3199 8 1 8 1 0 0 0\n");
    out.push_str("  TIMEMODE 1 5 -1 30 0 0 -1\n");
    out.push_str("  PANMODE 3\n");
    out.push_str("  CURSOR 0\n");
    out.push_str("  ZOOM 20 0 0\n");
    out.push_str("  VZOOMEX 6 0\n");
    out.push_str("  USE_REC_CFG 0\n");
    out.push_str("  RECMODE 1\n");
    out.push_str("  LOOP 0\n");
    out.push_str("  LOOPGRAN 0 4\n");
    out.push_str("  RECORD_PATH \"\" \"\"\n");
    out.push_str("  RENDER_FILE \"\"\n");
    out.push_str("  RENDER_PATTERN \"\"\n");
    out.push_str("  RENDER_FMT 0 2 0\n");
    out.push_str("  RENDER_1X 0\n");
    out.push_str("  RENDER_RANGE 1 0 0 18 1000\n");
    out.push_str("  SAMPLERATE 48000 0 0\n");
    out.push_str("  GLOBAL_AUTO -1\n");
    out.push_str(&format!("  TEMPO {} {} {} 0\n", tempo, ts_num, ts_denom));
    out.push_str("  PLAYRATE 1 0 0.25 4\n");
    out.push_str("  SELECTION 0 0\n");
    out.push_str("  SELECTION2 0 0\n");
    out.push_str("  MASTERTRACKHEIGHT 0 0\n");
    out.push_str("  MASTERPEAKCOL 16576\n");
    out.push_str("  MASTERMUTESOLO 0\n");
    out.push_str("  MASTERTRACKVIEW 0 0.6667 0.5 0.5 -1 -1 -1 0 0 0 -1 -1 0\n");
    out.push_str("  MASTERHWOUT 0 0 1 0 0 0 0 -1\n");
    out.push_str("  MASTER_NCH 2 2\n");
    out.push_str("  MASTER_VOLUME 1 0 -1 -1 1\n");
    out.push_str("  MASTER_PANMODE 6\n");
    out.push_str("  MASTER_FX 1\n");
    out.push_str("  MASTER_SEL 0\n");

    // Tempo envelope
    if let Some(ref env) = project.tempo_envelope {
        out.push_str("  <TEMPOENVEX\n");
        out.push_str("    ACT 1 -1\n");
        out.push_str("    VIS 1 0 1\n");
        out.push_str("    LANEHEIGHT 0 0\n");
        out.push_str("    ARM 0\n");
        out.push_str("    DEFSHAPE 1 -1 -1\n");
        for pt in &env.points {
            let ts_str = pt.time_signature_encoded
                .map(|ts| format!(" {}", ts))
                .unwrap_or_default();
            out.push_str(&format!("    PT {:.12} {:.10} {}{}\n",
                pt.position, pt.tempo, pt.shape, ts_str));
        }
        out.push_str("  >\n");
    }

    // Ruler lane definitions (FTS standard layout)
    // These must come before markers so REAPER knows the lane names
    out.push_str("  RULERHEIGHT 120 84\n");
    out.push_str("  RULERLANE 1 8 SECTIONS 0 -1\n");  // flag 8 = default region lane
    out.push_str("  RULERLANE 2 0 MARKS 0 -1\n");
    out.push_str("  RULERLANE 3 4 SONG 0 -1\n");      // flag 4 = default marker lane
    out.push_str("  RULERLANE 4 0 START/END 0 -1\n");
    out.push_str("  RULERLANE 5 0 KEY 0 -1\n");
    out.push_str("  RULERLANE 6 0 MODE 0 -1\n");
    out.push_str("  RULERLANE 7 0 CHORDS 0 -1\n");
    out.push_str("  RULERLANE 8 0 NOTES 0 -1\n");

    // Markers and regions
    // The last number on each MARKER line is the ruler lane index (0 = default)
    for mr in &project.markers_regions.all {
        let lane = mr.lane.unwrap_or(0);
        if mr.is_region() {
            // Region: two MARKER lines with same ID
            out.push_str(&format!("  MARKER {} {} {:?} 1 0 1 R {{}} 0 {}\n",
                mr.id, mr.position, mr.name, lane));
            out.push_str(&format!("  MARKER {} {} \"\" 1 0 1 R {{}} 0 {}\n",
                mr.id, mr.end_position.unwrap_or(mr.position), lane));
        } else {
            out.push_str(&format!("  MARKER {} {} {:?} 0 0 1 B {{}} 0 {}\n",
                mr.id, mr.position, mr.name, lane));
        }
    }

    // Tracks
    for track in &project.tracks {
        write_track_rpp(&mut out, track, 1);
    }

    out.push_str(">\n");
    out
}

fn write_track_rpp(out: &mut String, track: &Track, indent: usize) {
    let prefix = "  ".repeat(indent);
    out.push_str(&format!("{}<TRACK {{}}\n", prefix));
    out.push_str(&format!("{}  NAME {:?}\n", prefix, track.name));
    out.push_str(&format!("{}  PEAKCOL 16576\n", prefix));
    out.push_str(&format!("{}  BEAT -1\n", prefix));
    out.push_str(&format!("{}  AUTOMODE 0\n", prefix));

    if let Some(ref vp) = track.volpan {
        out.push_str(&format!("{}  VOLPAN {} {} -1 -1 1\n", prefix, vp.volume, vp.pan));
    } else {
        out.push_str(&format!("{}  VOLPAN 1 0 -1 -1 1\n", prefix));
    }

    out.push_str(&format!("{}  MUTESOLO 0 0 0\n", prefix));
    out.push_str(&format!("{}  IPHASE 0\n", prefix));

    if let Some(ref f) = track.folder {
        let state: i32 = match f.folder_state {
            FolderState::Regular => 0,
            FolderState::FolderParent => 1,
            FolderState::LastInFolder => 2,
            FolderState::Unknown(v) => v,
        };
        out.push_str(&format!("{}  ISBUS {} {}\n", prefix, state, f.indentation));
    } else {
        out.push_str(&format!("{}  ISBUS 0 0\n", prefix));
    }

    out.push_str(&format!("{}  SEL 0\n", prefix));
    out.push_str(&format!("{}  REC 0 0 0 0 0 0 0 0\n", prefix));
    out.push_str(&format!("{}  FX 1\n", prefix));
    out.push_str(&format!("{}  NCHAN 2\n", prefix));

    // Items
    for item in &track.items {
        write_item_rpp(out, item, indent + 1);
    }

    out.push_str(&format!("{}>\n", prefix));
}

fn write_item_rpp(out: &mut String, item: &Item, indent: usize) {
    let prefix = "  ".repeat(indent);
    out.push_str(&format!("{}<ITEM\n", prefix));
    out.push_str(&format!("{}  POSITION {}\n", prefix, item.position));
    out.push_str(&format!("{}  SNAPOFFS 0\n", prefix));
    out.push_str(&format!("{}  LENGTH {}\n", prefix, item.length));
    out.push_str(&format!("{}  LOOP 0\n", prefix));
    out.push_str(&format!("{}  ALLTAKES 0\n", prefix));
    out.push_str(&format!("{}  SEL 0\n", prefix));
    if !item.name.is_empty() {
        out.push_str(&format!("{}  NAME {:?}\n", prefix, item.name));
    }
    out.push_str(&format!("{}>\n", prefix));
}

// ── Lane Classification ──────────────────────────────────────────────────────

/// FTS ruler lane indices (matching session-proto::ruler_lanes::CoreLane).
const LANE_SECTIONS: u32 = 1;
const LANE_MARKS: u32 = 2;
const LANE_START_END: u32 = 4;

/// Classify a marker/region name into the correct FTS ruler lane index.
fn classify_lane(name: &str, is_region: bool) -> u32 {
    if is_region {
        // Most regions are sections
        LANE_SECTIONS
    } else {
        let upper = name.trim().to_uppercase();
        match upper.as_str() {
            "SONGSTART" | "SONGEND" | "COUNT-IN" | "COUNT IN" | "COUNTIN" => LANE_MARKS,
            "=START" | "=END" | "PREROLL" | "=PREROLL" | "POSTROLL" | "=POSTROLL" => LANE_START_END,
            _ if name.starts_with('=') => LANE_START_END,
            _ => 0, // Default lane for unclassified markers
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(position: f64, length: f64) -> Item {
        Item {
            position,
            length,
            ..Item::default()
        }
    }

    fn make_project_with_tracks(tracks: Vec<Track>) -> ReaperProject {
        ReaperProject {
            tracks,
            ..ReaperProject::default()
        }
    }

    fn make_empty_project() -> ReaperProject {
        ReaperProject::default()
    }

    fn make_songs(entries: &[(&str, f64)]) -> Vec<SongInfo> {
        let mut offset = 0.0;
        entries
            .iter()
            .map(|(name, dur)| {
                let s = SongInfo {
                    name: name.to_string(),
                    global_start_seconds: offset,
                    duration_seconds: *dur,
                };
                offset += dur;
                s
            })
            .collect()
    }

    #[test]
    fn guide_tracks_merged_with_offsets() {
        let p1 = make_project_with_tracks(vec![
            {
                let mut t = empty_track("Click");
                t.items = vec![make_item(0.0, 1.0)];
                t
            },
            {
                let mut t = empty_track("Guitar");
                t.items = vec![make_item(0.0, 10.0)];
                t
            },
        ]);
        let p2 = make_project_with_tracks(vec![
            {
                let mut t = empty_track("Click");
                t.items = vec![make_item(0.0, 1.0)];
                t
            },
            {
                let mut t = empty_track("Bass");
                t.items = vec![make_item(0.0, 15.0)];
                t
            },
        ]);

        let songs = make_songs(&[("Song A", 30.0), ("Song B", 45.0)]);
        let tracks = concatenate_tracks(&[p1, p2], &songs);

        let click = tracks.iter().find(|t| t.name == "Click").unwrap();
        assert_eq!(click.items.len(), 2);
        assert_eq!(click.items[0].position, 0.0);
        assert_eq!(click.items[1].position, 30.0);
    }

    #[test]
    fn content_tracks_offset_in_song_folders() {
        let p1 = make_project_with_tracks(vec![{
            let mut t = empty_track("Guitar");
            t.items = vec![make_item(0.0, 10.0)];
            t
        }]);
        let p2 = make_project_with_tracks(vec![{
            let mut t = empty_track("Bass");
            t.items = vec![make_item(0.0, 15.0)];
            t
        }]);

        let songs = make_songs(&[("Song A", 30.0), ("Song B", 45.0)]);
        let tracks = concatenate_tracks(&[p1, p2], &songs);

        let names: Vec<&str> = tracks.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"TRACKS"));
        assert!(names.contains(&"Song A"));
        assert!(names.contains(&"Song B"));

        let bass = tracks.iter().find(|t| t.name == "Bass").unwrap();
        assert_eq!(bass.items[0].position, 30.0);
    }

    #[test]
    fn tempo_points_offset_and_square_boundary() {
        let mut p1 = make_empty_project();
        p1.tempo_envelope = Some(TempoTimeEnvelope {
            points: vec![TempoTimePoint {
                position: 0.0,
                tempo: 120.0,
                shape: 0,
                time_signature_encoded: None,
                selected: false,
                unknown1: 0,
                bezier_tension: 0.0,
                metronome_pattern: String::new(),
                unknown2: 0,
                unknown3: 0,
                unknown4: 0
            }],
            default_tempo: 120.0,
            default_time_signature: (4, 4),
        });

        let mut p2 = make_empty_project();
        p2.tempo_envelope = Some(TempoTimeEnvelope {
            points: vec![TempoTimePoint {
                position: 0.0,
                tempo: 90.0,
                shape: 0,
                time_signature_encoded: None,
                selected: false,
                unknown1: 0,
                bezier_tension: 0.0,
                metronome_pattern: String::new(),
                unknown2: 0,
                unknown3: 0,
                unknown4: 0
            }],
            default_tempo: 90.0,
            default_time_signature: (3, 4),
        });

        let songs = make_songs(&[("A", 30.0), ("B", 45.0)]);
        let envelope = concatenate_tempo_envelopes(&[p1, p2], &songs);

        assert_eq!(envelope.points.len(), 2);
        assert_eq!(envelope.points[0].position, 0.0);
        assert_eq!(envelope.points[0].tempo, 120.0);
        assert_eq!(envelope.points[1].position, 30.0);
        assert_eq!(envelope.points[1].tempo, 90.0);
        assert_eq!(envelope.points[1].shape, 1, "Boundary should be square");
    }

    #[test]
    fn song_regions_generated_at_correct_positions() {
        let p1 = make_empty_project();
        let p2 = make_empty_project();

        let songs = make_songs(&[("Song A", 30.0), ("Song B", 45.0)]);
        let collection = generate_markers_regions(&[p1, p2], &songs);

        assert!(collection.regions.len() >= 2);

        let ra = collection.regions.iter().find(|r| r.name == "Song A").unwrap();
        assert_eq!(ra.position, 0.0);
        assert_eq!(ra.end_position, Some(30.0));

        let rb = collection.regions.iter().find(|r| r.name == "Song B").unwrap();
        assert_eq!(rb.position, 30.0);
        assert_eq!(rb.end_position, Some(75.0));
    }

    #[test]
    fn internal_markers_offset_and_ids_unique() {
        let mut p1 = make_empty_project();
        p1.markers_regions.all.push(MarkerRegion {
            id: 1,
            position: 5.0,
            name: "Chorus".to_string(),
            color: 0,
            flags: 0,
            locked: 0,
            guid: "old".to_string(),
            additional: 0,
            end_position: Some(15.0),
            lane: None,
            beat_position: None,
        });

        let mut p2 = make_empty_project();
        p2.markers_regions.all.push(MarkerRegion {
            id: 1, // Same ID as p1
            position: 2.0,
            name: "Verse".to_string(),
            color: 0,
            flags: 0,
            locked: 0,
            guid: "old2".to_string(),
            additional: 0,
            end_position: None,
            lane: None,
            beat_position: None,
        });

        let songs = make_songs(&[("Song A", 30.0), ("Song B", 45.0)]);
        let collection = generate_markers_regions(&[p1, p2], &songs);

        // Chorus should be at position 5 (Song A offset = 0)
        let chorus = collection.all.iter().find(|m| m.name == "Chorus").unwrap();
        assert_eq!(chorus.position, 5.0);
        assert!(chorus.guid.is_empty());

        // Verse should be at position 32 (Song B offset = 30 + 2)
        let verse = collection.all.iter().find(|m| m.name == "Verse").unwrap();
        assert_eq!(verse.position, 32.0);

        // All IDs unique
        let ids: Vec<i32> = collection.all.iter().map(|m| m.id).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "IDs must be unique");
    }
}

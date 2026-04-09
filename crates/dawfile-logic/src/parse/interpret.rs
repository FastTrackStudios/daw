//! Chunk interpreter — builds a [`LogicSession`] from raw [`LogicChunk`]s.
//!
//! The internal payload format of most chunk types is not yet fully
//! reverse-engineered.  This module extracts what it can from the chunks
//! (primarily from `Envi`/`ivnE` mixer objects which carry track names) and
//! leaves the rest as raw data in [`LogicSession::chunks`].
//!
//! As we learn more about individual chunk payloads, interpretation logic
//! will grow here without changing the public API.

use crate::parse::aufl::parse_aufl;
use crate::parse::aurg::parse_aurg;
use crate::parse::bundle::BundleMeta;
use crate::types::{
    ClipKind, LogicChunk, LogicClip, LogicMarker, LogicSession, LogicSummingGroup, LogicTempoEvent,
    LogicTrack, TrackKind,
};

/// Build a [`LogicSession`] from parsed bundle metadata and the flat chunk list.
pub fn build_session(meta: BundleMeta, chunks: Vec<LogicChunk>) -> LogicSession {
    // Extract mixer channel names from Envi (ivnE) chunks.
    let (mut tracks, summing_groups) = extract_tracks_and_groups(&chunks);

    // Collect audio regions from AuRg chunks and attach them as clips.
    // We don't have track-to-region mapping yet, so we attach all audio clips
    // to the first audio track that matches by name, or append them to a
    // catch-all list if no match is found.
    let audio_regions = collect_audio_regions(&chunks, meta.sample_rate);
    attach_audio_clips(&mut tracks, audio_regions);

    LogicSession {
        creator_version: meta.creator_version,
        variant_name: meta.variant_name,
        sample_rate: meta.sample_rate,
        bpm: meta.bpm,
        time_sig_numerator: meta.time_sig_numerator,
        time_sig_denominator: meta.time_sig_denominator,
        key: meta.key,
        key_gender: meta.key_gender,
        tracks,
        markers: Vec::new(),      // TODO: parse from EvSq chunks
        tempo_events: Vec::new(), // TODO: parse from EvSq chunks
        summing_groups,
        chunks,
    }
}

// ── AuFl / AuRg chunk parsing ────────────────────────────────────────────────

const TAG_AUFL: [u8; 4] = *b"lFuA"; // on-disk tag for AuFl (reversed)
const TAG_AURG: [u8; 4] = *b"gRuA"; // on-disk tag for AuRg (reversed)

/// An audio clip ready to be attached to a track.
struct PendingClip {
    /// Region name (e.g. `"Audio Track 1 #01"`).
    name: String,
    /// Track name inferred from the region name (prefix before ` #NN`).
    track_name_hint: String,
    clip: LogicClip,
}

/// Collect all parsed audio regions from `AuRg` chunks.
///
/// The sample_rate is used to convert frame counts to beat positions using
/// the session BPM stored in the metadata plist.  At this point we don't have
/// the BPM here, so we leave positions in beats as raw samples / sample_rate.
/// Beat conversion happens in the public `LogicClip` later.
fn collect_audio_regions(chunks: &[LogicChunk], sample_rate: u32) -> Vec<PendingClip> {
    // Build a map of filename → AuFl entry for file-path decoration.
    let aufl_map: Vec<_> = chunks
        .iter()
        .filter(|c| c.tag == TAG_AUFL)
        .filter_map(|c| parse_aufl(&c.data))
        .collect();

    let sr = sample_rate.max(1) as f64;

    chunks
        .iter()
        .filter(|c| c.tag == TAG_AURG)
        .filter_map(|c| {
            let region = parse_aurg(&c.data)?;

            // `start_ticks` is the arrangement clock position in Logic's internal
            // tick unit (240 PPQ at session BPM).  We don't have the BPM here so
            // we store ticks directly as "beats" for now — callers can convert
            // knowing the session BPM (LogicSession::bpm) and that 1 beat = 240
            // ticks.  Duration is stored as seconds (frames / sample_rate).
            // TODO: store everything in seconds once BPM is threaded through.
            let position_beats = region.start_ticks as f64; // raw 240-PPQ ticks
            let length_beats = region.duration_frames as f64 / sr; // seconds

            // Try to find the matching audio file by name prefix.
            let file_path = aufl_map
                .iter()
                .find(|f| {
                    region.name.starts_with(
                        &f.filename[..f.filename.rfind('.').unwrap_or(f.filename.len())],
                    )
                })
                .map(|f| {
                    if f.vol_name.is_empty() {
                        f.filename.clone()
                    } else {
                        format!("{}/{}", f.vol_name, f.filename)
                    }
                });

            // Infer track name from region name: "Audio Track 1 #01" → "Audio Track 1"
            let track_name_hint = if let Some(pos) = region.name.rfind(" #") {
                region.name[..pos].to_owned()
            } else {
                region.name.clone()
            };

            Some(PendingClip {
                name: region.name.clone(),
                track_name_hint,
                clip: LogicClip {
                    position_beats,
                    length_beats,
                    kind: ClipKind::Audio { file_path },
                },
            })
        })
        .collect()
}

/// Attach pending clips to matching tracks by name.
///
/// If no track matches, clips are dropped (we don't yet have the track↔region
/// mapping from the Trak/MSeq chunks).
fn attach_audio_clips(tracks: &mut Vec<LogicTrack>, clips: Vec<PendingClip>) {
    for pending in clips {
        if let Some(track) = tracks
            .iter_mut()
            .find(|t| t.name == pending.track_name_hint)
        {
            track.clips.push(pending.clip);
        }
    }
}

// ── Envi chunk parsing ────────────────────────────────────────────────────────
//
// `ivnE` (Envi) chunks represent mixer objects: tracks, buses, aux sends,
// the master, etc.  Each chunk carries a name string that is the user-visible
// label for that mixer channel.
//
// From reverse-engineering with the cigol scripts, the name appears as a
// null-terminated or length-prefixed ASCII string somewhere in the payload.
// We use a heuristic: scan for the longest printable-ASCII run that looks
// like a name (no control chars, not all whitespace).

const TAG_ENVI: [u8; 4] = *b"ivnE";

fn extract_tracks_and_groups(chunks: &[LogicChunk]) -> (Vec<LogicTrack>, Vec<LogicSummingGroup>) {
    let mut tracks = Vec::new();
    let mut summing_groups = Vec::new();

    for chunk in chunks {
        if chunk.tag != TAG_ENVI {
            continue;
        }

        let name = extract_name_from_envi(&chunk.data);
        let kind = classify_envi_channel(&name);

        match kind {
            EnviKind::SummingGroup { child_hint } => {
                summing_groups.push(LogicSummingGroup {
                    name,
                    member_names: child_hint.into_iter().collect(),
                });
            }
            EnviKind::SystemChannel => {
                // Skip internal channels (Click, Stereo Out, Master, etc.)
                // unless we later decide to expose them.
            }
            EnviKind::Track(track_kind) => {
                tracks.push(LogicTrack {
                    name,
                    kind: track_kind,
                    channel: 0,     // TODO: parse channel index from payload
                    fader_db: None, // TODO: parse from payload using cigol formula
                    muted: false,
                    soloed: false,
                    parent_group: None, // TODO: link via summing group membership
                    clips: Vec::new(),  // TODO: parse from Trak/EvSq chunks
                });
            }
        }
    }

    tracks.retain(|t| !t.name.is_empty());

    (tracks, summing_groups)
}

#[derive(Debug)]
enum EnviKind {
    Track(TrackKind),
    SummingGroup { child_hint: Option<String> },
    SystemChannel,
}

/// Classify an Envi channel by its name.
fn classify_envi_channel(name: &str) -> EnviKind {
    let lower = name.to_ascii_lowercase();

    // Known system channels
    if matches!(
        lower.as_str(),
        "stereo out"
            | "master"
            | "click"
            | "preview"
            | "not assigned"
            | "midi click"
            | "sequencer input"
            | "physical input"
            | "input view"
            | "input notes"
            | "folder"
    ) {
        return EnviKind::SystemChannel;
    }

    // Summing group heuristics (Logic names them with "Sum" or they have a
    // "Child of" hint embedded in the payload alongside the name)
    if lower.contains("sum") || lower.contains("group") || lower.contains("indented") {
        return EnviKind::SummingGroup { child_hint: None };
    }

    // Track kind by name convention
    if lower.contains("audio") {
        return EnviKind::Track(TrackKind::Audio);
    }
    if lower.contains("midi") {
        return EnviKind::Track(TrackKind::Midi);
    }
    if lower.contains("aux") || lower.contains("bus") || lower.contains("send") {
        return EnviKind::Track(TrackKind::Aux);
    }

    // Default: treat unknown named channels as audio tracks
    EnviKind::Track(TrackKind::Audio)
}

/// Heuristically extract a human-readable name from an `ivnE` payload.
///
/// Logic stores names as Pascal strings (1-byte length prefix) or as
/// null-terminated strings at various offsets.  We scan for the longest
/// printable-ASCII run that plausibly looks like a channel name.
fn extract_name_from_envi(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }

    // Strategy 1: Pascal string at the start (common in Logic environments).
    // First byte = length, followed by that many ASCII bytes.
    {
        let len = data[0] as usize;
        if len > 0 && len < 64 && data.len() > len {
            let candidate = &data[1..=len];
            if candidate.iter().all(|&b| b >= 0x20 && b < 0x7f) {
                return String::from_utf8_lossy(candidate).into_owned();
            }
        }
    }

    // Strategy 2: scan for a null-terminated printable ASCII string of
    // length 3–64 anywhere in the payload.
    let mut best: &[u8] = &[];
    let mut i = 0;
    while i < data.len() {
        if data[i] >= 0x20 && data[i] < 0x7f {
            let start = i;
            while i < data.len() && data[i] >= 0x20 && data[i] < 0x7f {
                i += 1;
            }
            let run = &data[start..i];
            if run.len() >= 3 && run.len() > best.len() && run.len() <= 64 {
                best = run;
            }
        } else {
            i += 1;
        }
    }

    String::from_utf8_lossy(best).into_owned()
}

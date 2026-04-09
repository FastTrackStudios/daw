//! Audio track parsing and region-to-track assignment.
//!
//! Tracks are parsed from block 0x1015 (audio track list). Region-to-track
//! assignments come from blocks 0x1012 (old, PT 5-7) or 0x1054 (new, PT 8+).
//!
//! ## Playlist model
//!
//! In Pro Tools, each track has one active playlist plus zero or more alternate
//! (inactive) playlists. In the file format:
//!
//! - The top-level `0x1054` block holds the **active** playlist for every track.
//!   Each child `0x1052` corresponds to one track channel and carries the
//!   playlist name and its region placements.
//! - `0x2428` / `0x2429` blocks wrap additional `0x1054` blocks for each
//!   **alternate** playlist. These are empty in most sessions.

use crate::block::Block;
use crate::content_type::ContentType;
use crate::cursor::Cursor;
use crate::types::{AudioRegion, NO_REGION, Playlist, Track, TrackRegion};

/// Parse audio tracks and their region assignments (active + alternate playlists).
pub fn parse_audio_tracks(
    blocks: &[Block],
    cursor: &Cursor<'_>,
    regions: &[AudioRegion],
    version: u16,
    rate_factor: f64,
) -> Vec<Track> {
    // Step 1: Parse track definitions from 0x1015
    let mut tracks = parse_track_definitions(blocks, cursor);

    // Step 2: Assign active playlist regions to tracks
    if version < 8 {
        assign_regions_old(blocks, cursor, regions, &mut tracks);
    } else {
        assign_regions_new(blocks, cursor, regions, &mut tracks, rate_factor);
        // Step 2b: Parse alternate playlists from 0x2428 / 0x2429 containers
        assign_alternate_playlists(blocks, cursor, regions, &mut tracks, rate_factor);
    }

    // Step 3: Remove tracks with no active regions
    tracks.retain(|t| !t.regions.is_empty());

    // Step 4: Sort by index and renumber
    tracks.sort_by_key(|t| t.index);
    for (i, track) in tracks.iter_mut().enumerate() {
        track.index = i as u16;
    }

    tracks
}

// ── Track definitions ──────────────────────────────────────────────────────

/// Parse track definitions from the AudioTrackList block (0x1015).
fn parse_track_definitions(blocks: &[Block], cursor: &Cursor<'_>) -> Vec<Track> {
    let mut tracks = Vec::new();

    let track_list = match find_block_recursive(blocks, ContentType::AudioTrackList) {
        Some(b) => b,
        None => return tracks,
    };

    for child in track_list.find_children(ContentType::AudioTrackInfo) {
        let data = cursor.data();
        let name_offset = child.offset + 2;
        if name_offset + 4 >= data.len() {
            continue;
        }

        let (name, str_consumed) = cursor.length_prefixed_string(name_offset);

        // Number of channels is a u32 after name + 1 separator byte.
        // str_consumed already includes the 4-byte length prefix, so +1 for the separator.
        let nch_offset = name_offset + str_consumed + 1;
        if nch_offset + 4 >= data.len() {
            continue;
        }
        let nch = cursor.u32_at(nch_offset) as usize;

        // Read channel map indices (nch × u16)
        let ch_offset = nch_offset + 4;
        for ch in 0..nch {
            let idx_offset = ch_offset + ch * 2;
            if idx_offset + 2 > data.len() {
                break;
            }
            let track_index = cursor.u16_at(idx_offset);

            tracks.push(Track {
                name: name.clone(),
                index: track_index,
                playlist_name: String::new(), // filled in by assign_regions_*
                regions: Vec::new(),
                alternate_playlists: Vec::new(),
            });
        }
    }

    tracks
}

// ── Active playlist assignment ─────────────────────────────────────────────

/// Assign regions to tracks using the old format (block 0x1012, PT 5-7).
///
/// In the old format there are no named playlists, so `playlist_name` is left
/// as an empty string.
fn assign_regions_old(
    blocks: &[Block],
    cursor: &Cursor<'_>,
    regions: &[AudioRegion],
    tracks: &mut [Track],
) {
    let map_block = match find_block_recursive(blocks, ContentType::AudioRegionTrackMap) {
        Some(b) => b,
        None => return,
    };

    let entries = map_block.find_all(ContentType::RegionTrackEntry);
    let mut track_idx = 0;

    for entry in entries {
        if track_idx >= tracks.len() {
            break;
        }

        let data = cursor.data();
        let raw_offset = entry.offset + 4;
        if raw_offset + 4 > data.len() {
            continue;
        }

        let raw_index = cursor.u32_at(raw_offset) as u16;

        if raw_index == NO_REGION {
            track_idx += 1;
            continue;
        }

        if let Some(region) = regions.iter().find(|r| r.index == raw_index) {
            tracks[track_idx].regions.push(TrackRegion {
                region_index: raw_index,
                start_pos: region.start_pos,
            });
        }

        track_idx += 1;
    }
}

/// Assign regions to tracks from a single 0x1054 block.
///
/// Returns the playlist name for each track slot (indexed by position).
/// Each 0x1052 block in the 0x1054 corresponds to one track slot; `track_idx`
/// advances once per 0x1052 (matching ptformat's `count++` per track).
fn assign_from_map_block(
    map_block: &Block,
    cursor: &Cursor<'_>,
    regions: &[AudioRegion],
    tracks: &mut [Track],
    rate_factor: f64,
    dest: AssignDest,
) {
    let map_entries = map_block.find_all(ContentType::AudioRegionTrackMapEntriesNew);
    let data = cursor.data();
    let mut track_idx = 0;

    for map_entry in &map_entries {
        if track_idx >= tracks.len() {
            break;
        }

        // Read the playlist name from the 0x1052 block (at offset + 2)
        let playlist_name = {
            let no = map_entry.offset + 2;
            if no + 4 < data.len() {
                let (n, _) = cursor.length_prefixed_string(no);
                n
            } else {
                String::new()
            }
        };

        let track_entries = map_entry.find_all(ContentType::AudioRegionTrackEntryNew);
        let mut slot_regions = Vec::new();

        for track_entry in &track_entries {
            // Skip fade regions (byte at offset+46 == 0x01)
            if track_entry.offset + 47 <= data.len()
                && cursor.u8_at(track_entry.offset + 46) == 0x01
            {
                continue;
            }

            let sub_entries = track_entry.find_all(ContentType::AudioRegionTrackSubEntryNew);
            for sub_entry in &sub_entries {
                let raw_offset = sub_entry.offset + 4;
                if raw_offset + 4 > data.len() {
                    continue;
                }
                let raw_index = cursor.u32_at(raw_offset) as u16;
                if raw_index == NO_REGION {
                    continue;
                }

                // Start position (u32 LE at offset+9, matches ptformat's u_endian_read4)
                let start_offset = sub_entry.offset + 9;
                let start = if start_offset + 4 <= data.len() {
                    let raw_start = cursor.u32_at(start_offset) as u64;
                    (raw_start as f64 * rate_factor) as u64
                } else if let Some(r) = regions.iter().find(|r| r.index == raw_index) {
                    r.start_pos
                } else {
                    0
                };

                slot_regions.push(TrackRegion {
                    region_index: raw_index,
                    start_pos: start,
                });
            }
        }

        match dest {
            AssignDest::Active => {
                tracks[track_idx].playlist_name = playlist_name;
                tracks[track_idx].regions.extend(slot_regions);
            }
            AssignDest::Alternate => {
                if !slot_regions.is_empty() || !playlist_name.is_empty() {
                    tracks[track_idx].alternate_playlists.push(Playlist {
                        name: playlist_name,
                        regions: slot_regions,
                    });
                }
            }
        }

        track_idx += 1;
    }
}

#[derive(Clone, Copy)]
enum AssignDest {
    Active,
    Alternate,
}

/// Assign regions from the top-level 0x1054 block (the active playlist).
fn assign_regions_new(
    blocks: &[Block],
    cursor: &Cursor<'_>,
    regions: &[AudioRegion],
    tracks: &mut [Track],
    rate_factor: f64,
) {
    // The top-level 0x1054 block (not wrapped in 0x2428/0x2429) is the active playlist.
    let map_block = match find_top_level_block(blocks, ContentType::AudioRegionTrackMapNew) {
        Some(b) => b,
        None => return,
    };
    assign_from_map_block(
        map_block,
        cursor,
        regions,
        tracks,
        rate_factor,
        AssignDest::Active,
    );
}

/// Parse alternate playlists from 0x2428 / 0x2429 container blocks.
///
/// Each container wraps a 0x1054 block with the same channel-slot layout as the
/// active playlist. Empty slots produce no `Playlist` entry for that track.
fn assign_alternate_playlists(
    blocks: &[Block],
    cursor: &Cursor<'_>,
    regions: &[AudioRegion],
    tracks: &mut [Track],
    rate_factor: f64,
) {
    for block in blocks {
        if block.content_type == Some(ContentType::AlternatePlaylistMap)
            || block.content_type == Some(ContentType::AlternatePlaylistMapAlt)
        {
            // Find the 0x1054 inside this container
            if let Some(map_block) = block.find_child(ContentType::AudioRegionTrackMapNew) {
                assign_from_map_block(
                    map_block,
                    cursor,
                    regions,
                    tracks,
                    rate_factor,
                    AssignDest::Alternate,
                );
            }
        }
        // Recurse into children (containers can be nested)
        assign_alternate_playlists(&block.children, cursor, regions, tracks, rate_factor);
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Find the first top-level block (not nested inside another known block) with
/// the given content type. This ensures we get the active 0x1054 rather than
/// one buried inside a 0x2428 / 0x2429 alternate-playlist container.
fn find_top_level_block<'a>(blocks: &'a [Block], ct: ContentType) -> Option<&'a Block> {
    for block in blocks {
        if block.content_type == Some(ct) {
            return Some(block);
        }
    }
    None
}

fn find_block_recursive<'a>(blocks: &'a [Block], ct: ContentType) -> Option<&'a Block> {
    for block in blocks {
        if block.content_type == Some(ct) {
            return Some(block);
        }
        if let Some(found) = find_block_recursive(&block.children, ct) {
            return Some(found);
        }
    }
    None
}

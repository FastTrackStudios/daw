//! Audio track parsing and region-to-track assignment.
//!
//! Tracks are parsed from block 0x1015 (audio track list). Region-to-track
//! assignments come from blocks 0x1012 (old) or 0x1054 (v8+).

use crate::block::Block;
use crate::content_type::ContentType;
use crate::cursor::Cursor;
use crate::types::{AudioRegion, NO_REGION, Track, TrackRegion};

/// Parse audio tracks and their region assignments.
pub fn parse_audio_tracks(
    blocks: &[Block],
    cursor: &Cursor<'_>,
    regions: &[AudioRegion],
    version: u16,
    rate_factor: f64,
) -> Vec<Track> {
    // Step 1: Parse track definitions from 0x1015
    let mut tracks = parse_track_definitions(blocks, cursor);

    // Step 2: Assign regions to tracks
    if version < 8 {
        assign_regions_old(blocks, cursor, regions, &mut tracks);
    } else {
        assign_regions_new(blocks, cursor, regions, &mut tracks, rate_factor);
    }

    // Step 3: Remove tracks with no assigned regions
    tracks.retain(|t| !t.regions.is_empty());

    // Step 4: Sort by index and renumber
    tracks.sort_by_key(|t| t.index);
    for (i, track) in tracks.iter_mut().enumerate() {
        track.index = i as u16;
    }

    tracks
}

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

        // Number of channels is a u32 after name + 1 separator byte
        // (str_consumed already includes the 4-byte length prefix, so only +1 for the separator)
        let nch_offset = name_offset + str_consumed + 1;
        if nch_offset + 4 >= data.len() {
            continue;
        }
        let nch = cursor.u32_at(nch_offset) as usize;

        // Read channel map indices (nch x u16)
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
                playlist: 0,
                regions: Vec::new(),
            });
        }
    }

    tracks
}

/// Assign regions to tracks using the old format (block 0x1012, PT 5-7).
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

/// Assign regions to tracks using the new format (block 0x1054, PT 8+).
fn assign_regions_new(
    blocks: &[Block],
    cursor: &Cursor<'_>,
    regions: &[AudioRegion],
    tracks: &mut [Track],
    rate_factor: f64,
) {
    let map_block = match find_block_recursive(blocks, ContentType::AudioRegionTrackMapNew) {
        Some(b) => b,
        None => return,
    };

    // Navigate: 0x1054 > 0x1052 > 0x1050 > 0x104f
    //
    // Each 0x1052 block corresponds to one track slot (ptformat increments its
    // track counter once per 0x1052). Within a 0x1052, each 0x1050 is one
    // region placement on that same track. We must advance track_idx once per
    // 0x1052, not once per 0x1050.
    let map_entries = map_block.find_all(ContentType::AudioRegionTrackMapEntriesNew);
    let mut track_idx = 0;

    for map_entry in &map_entries {
        if track_idx >= tracks.len() {
            break;
        }

        let track_entries = map_entry.find_all(ContentType::AudioRegionTrackEntryNew);
        let data = cursor.data();

        for track_entry in &track_entries {
            // Check if this is a fade region (byte at offset+46 == 0x01)
            if track_entry.offset + 47 <= data.len()
                && cursor.u8_at(track_entry.offset + 46) == 0x01
            {
                continue; // Skip fade regions — do not affect track_idx
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

                // Read start position at offset + 9 (u32 LE, matches ptformat's u_endian_read4)
                let start_offset = sub_entry.offset + 9;
                let start = if start_offset + 4 <= data.len() {
                    let raw_start = cursor.u32_at(start_offset) as u64;
                    (raw_start as f64 * rate_factor) as u64
                } else if let Some(region) = regions.iter().find(|r| r.index == raw_index) {
                    region.start_pos
                } else {
                    0
                };

                tracks[track_idx].regions.push(TrackRegion {
                    region_index: raw_index,
                    start_pos: start,
                });
            }
        }

        // Advance once per 0x1052 (track slot), matching ptformat's count++
        track_idx += 1;
    }
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

//! MIDI parsing: events, regions, and track assignments.
//!
//! MIDI data is stored as event chunks (block 0x2000), which are then
//! mapped to regions (0x2002/0x2634) and finally to tracks (0x1058).

use crate::block::Block;
use crate::content_type::ContentType;
use crate::cursor::{self, Cursor};
use crate::types::{MidiEvent, MidiRegion, NO_REGION, Track, TrackKind, TrackRegion, ZERO_TICKS};

/// Magic marker that precedes MIDI event data within a 0x2000 block.
const MIDI_MAGIC: &[u8] = b"MdNLB";

/// A raw MIDI event chunk before region assignment.
#[derive(Debug, Clone)]
struct MidiChunk {
    events: Vec<MidiEvent>,
    zero_ticks: u64,
    max_pos: u64,
}

/// Parse all MIDI data: events, regions, and tracks.
pub fn parse_midi(
    blocks: &[Block],
    cursor: &Cursor<'_>,
    version: u16,
    rate_factor: f64,
) -> (Vec<MidiRegion>, Vec<Track>) {
    // Pass 1: Parse raw MIDI event chunks
    let chunks = parse_midi_chunks(blocks, cursor);

    // Pass 2: Map chunks to MIDI regions
    let regions = parse_midi_regions(blocks, cursor, &chunks, version, rate_factor);

    // Pass 3: Parse MIDI tracks and assign regions
    let tracks = parse_midi_tracks(blocks, cursor, &regions, rate_factor);

    (regions, tracks)
}

/// Parse raw MIDI event chunks from 0x2000 blocks.
fn parse_midi_chunks(blocks: &[Block], cursor: &Cursor<'_>) -> Vec<MidiChunk> {
    let mut chunks = Vec::new();
    let data = cursor.data();

    let midi_blocks = find_all_recursive(blocks, ContentType::MidiEventsBlock);

    for block in midi_blocks {
        // Scan for ALL MdNLB magic markers within the block.
        // A single 0x2000 block can contain multiple MIDI chunks.
        let block_end = (block.offset + block.block_size as usize).min(data.len());
        let mut search_start = block.offset;

        while let Some(magic_pos) = find_magic(data, search_start, block_end) {
            // Advance search past this magic for the next iteration
            search_start = magic_pos + MIDI_MAGIC.len();

            // n_events at magic + 11
            let n_events_offset = magic_pos + 11;
            if n_events_offset + 4 > data.len() {
                break;
            }
            let n_events = cursor.u32_at(n_events_offset) as usize;

            // zero_ticks at magic + 15 (5 bytes, LE)
            let zt_offset = magic_pos + 15;
            if zt_offset + 5 > data.len() {
                break;
            }
            let zero_ticks = cursor.u40_le(zt_offset);

            // Events start at magic + 20, each is 35 bytes
            let events_start = magic_pos + 20;
            let mut events = Vec::with_capacity(n_events);
            let mut max_pos: u64 = 0;

            for i in 0..n_events {
                let ev_offset = events_start + i * 35;
                if ev_offset + 35 > data.len() {
                    break;
                }

                let midi_pos = cursor.u40_le(ev_offset);
                let relative_pos = midi_pos.saturating_sub(zero_ticks);

                let note = cursor.u8_at(ev_offset + 8);
                let duration = cursor.u40_le(ev_offset + 9);
                let velocity = cursor.u8_at(ev_offset + 17);

                if relative_pos > max_pos {
                    max_pos = relative_pos;
                }

                events.push(MidiEvent {
                    position: relative_pos,
                    duration,
                    note,
                    velocity,
                });
            }

            chunks.push(MidiChunk {
                events,
                zero_ticks,
                max_pos,
            });
        }
    }

    chunks
}

/// Parse MIDI regions from region map blocks.
fn parse_midi_regions(
    blocks: &[Block],
    cursor: &Cursor<'_>,
    chunks: &[MidiChunk],
    version: u16,
    _rate_factor: f64,
) -> Vec<MidiRegion> {
    let mut regions = Vec::new();

    // Choose block types based on version
    let (map_ct, region_ct) = if version < 10 {
        (ContentType::MidiRegionMapOld, ContentType::MidiRegionOld)
    } else {
        (ContentType::MidiRegionMapNew, ContentType::MidiRegionNew)
    };

    let region_map = match find_block_recursive(blocks, map_ct) {
        Some(b) => b,
        None => return regions,
    };

    let region_blocks = region_map.find_all(region_ct);

    for (idx, block) in region_blocks.iter().enumerate() {
        let data = cursor.data();

        // For PT 10+, the region data is inside a CompoundRegionGroup (0x2628) child.
        // For PT 5-9, the data is directly in the region block.
        let data_block = if version >= 10 {
            match block.find_child(ContentType::CompoundRegionGroup) {
                Some(child) => child,
                None => continue,
            }
        } else {
            block
        };

        // Region name at data_block.offset + 2
        let name_offset = data_block.offset + 2;
        if name_offset + 4 >= data.len() {
            continue;
        }
        let (name, str_consumed) = cursor.length_prefixed_string(name_offset);

        // The chunk index is a u32 right after the end of the data block
        // (at data_block.offset + data_block.block_size)
        let chunk_idx_offset = data_block.offset + data_block.block_size as usize;
        let chunk_idx = if chunk_idx_offset + 4 <= data.len() {
            cursor.u32_at(chunk_idx_offset) as usize
        } else {
            idx
        };

        // Look up the MIDI chunk
        let events = if chunk_idx < chunks.len() {
            chunks[chunk_idx].events.clone()
        } else {
            Vec::new()
        };

        let region_length = if chunk_idx < chunks.len() {
            chunks[chunk_idx].max_pos
        } else {
            0
        };

        regions.push(MidiRegion {
            name,
            index: idx as u16,
            start_pos: ZERO_TICKS,
            sample_offset: 0,
            length: region_length,
            events,
        });
    }

    regions
}

/// Parse MIDI tracks and assign regions.
fn parse_midi_tracks(
    blocks: &[Block],
    cursor: &Cursor<'_>,
    regions: &[MidiRegion],
    rate_factor: f64,
) -> Vec<Track> {
    let mut tracks = Vec::new();

    // Parse track definitions from 0x2519
    let track_list = match find_block_recursive(blocks, ContentType::MidiTrackList) {
        Some(b) => b,
        None => return tracks,
    };

    for child in track_list.find_children(ContentType::MidiTrackInfo) {
        let data = cursor.data();
        let name_offset = child.offset + 4;
        if name_offset + 4 >= data.len() {
            continue;
        }

        let (name, _str_consumed) = cursor.length_prefixed_string(name_offset);

        tracks.push(Track {
            name,
            kind: TrackKind::Midi,
            index: tracks.len() as u16,
            playlist_name: String::new(),
            regions: Vec::new(),
            alternate_playlists: Vec::new(),
        });
    }

    // Assign regions to tracks from 0x1058
    let map_block = match find_block_recursive(blocks, ContentType::MidiRegionTrackMap) {
        Some(b) => b,
        None => return tracks,
    };

    let sub_entries = map_block.find_all(ContentType::AudioRegionTrackSubEntryNew);
    let mut track_idx = 0;

    for entry in sub_entries {
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

        // Read start position (u40 at offset + 9)
        let start_offset = entry.offset + 9;
        let start = if start_offset + 5 <= data.len() {
            let raw_start = cursor.u40_le(start_offset);
            let relative = if raw_start >= ZERO_TICKS {
                raw_start - ZERO_TICKS
            } else {
                ZERO_TICKS - raw_start
            };
            (relative as f64 * rate_factor) as u64
        } else {
            0
        };

        if track_idx < tracks.len() {
            tracks[track_idx].regions.push(TrackRegion {
                region_index: raw_index,
                start_pos: start,
            });
        }

        track_idx += 1;
    }

    // Remove tracks with no regions
    tracks.retain(|t| !t.regions.is_empty());

    tracks
}

/// Find the MdNLB magic bytes within a range.
fn find_magic(data: &[u8], start: usize, end: usize) -> Option<usize> {
    let end = end.min(data.len());
    if end < start + MIDI_MAGIC.len() {
        return None;
    }
    data[start..end - MIDI_MAGIC.len() + 1]
        .windows(MIDI_MAGIC.len())
        .position(|w| w == MIDI_MAGIC)
        .map(|p| start + p)
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

fn find_all_recursive<'a>(blocks: &'a [Block], ct: ContentType) -> Vec<&'a Block> {
    let mut result = Vec::new();
    for block in blocks {
        if block.content_type == Some(ct) {
            result.push(block);
        }
        result.extend(find_all_recursive(&block.children, ct));
    }
    result
}

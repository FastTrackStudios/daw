//! Top-level parse orchestration.
//!
//! The main parse flow:
//! 1. Decrypt the file
//! 2. Detect the Pro Tools version
//! 3. Parse the block tree
//! 4. Extract session metadata (sample rate)
//! 5. Parse tempo and meter maps
//! 6. Extract audio file references
//! 7. Extract regions and region-to-track assignments
//! 8. Extract MIDI data

pub mod audio;
pub mod meter;
pub mod midi;
pub mod regions;
pub mod tempo;
pub mod tracks;
pub mod version;

use crate::block::{self, Block};
use crate::content_type::ContentType;
use crate::cursor::Cursor;
use crate::decrypt;
use crate::error::{PtError, PtResult};
use crate::types::{ProToolsSession, TempoEvent};

/// Parse a Pro Tools session from raw file bytes.
///
/// The `target_sample_rate` is the rate to convert positions to. If it matches
/// the session rate, no conversion is applied.
pub fn parse_session(data: &mut Vec<u8>, target_sample_rate: u32) -> PtResult<ProToolsSession> {
    // Step 1: Decrypt
    let xor_type = decrypt::decrypt(data)?;

    // Step 2: Detect endianness and version
    let is_bigendian = data.get(0x11).copied().unwrap_or(0) != 0;
    let cursor = Cursor::new(data, is_bigendian);

    // Step 3: Parse block tree
    let blocks = block::parse_blocks(data, is_bigendian);

    // Step 4: Detect version
    let version = version::parse_version(&cursor, &blocks, xor_type)?;
    if !(5..=12).contains(&version) {
        return Err(PtError::UnsupportedVersion(version));
    }

    // Step 5: Extract sample rate
    let session_sample_rate = parse_sample_rate(&blocks, &cursor).unwrap_or(48000);
    let rate_factor = if session_sample_rate > 0 && target_sample_rate > 0 {
        target_sample_rate as f64 / session_sample_rate as f64
    } else {
        1.0
    };

    // Step 6: Parse tempo map (needed for all tick→sample conversions below)
    let tempo_segments = tempo::parse_tempo_map(&blocks, &cursor, target_sample_rate);

    // Build the public TempoEvent list from the internal segments.
    let bpm = tempo_segments.first().map(|s| s.bpm).unwrap_or(120.0);
    let tempo_events: Vec<TempoEvent> = tempo_segments
        .iter()
        .map(|s| TempoEvent {
            tick_start: s.tick_start,
            sample_start: s.sample_start,
            bpm: s.bpm,
            ticks_per_beat: s.ticks_per_beat,
        })
        .collect();

    // Step 7: Parse meter map and markers
    let meter_events =
        meter::parse_meter_events(&blocks, &cursor, &tempo_segments, target_sample_rate);
    let markers = meter::parse_markers(&blocks, &cursor, &tempo_segments, target_sample_rate);

    // Step 8: Parse audio files
    let audio_files = audio::parse_audio_files(&blocks, &cursor, version);

    // Step 9: Parse regions
    let audio_regions = regions::parse_audio_regions(&blocks, &cursor, version, rate_factor);

    // Step 10: Parse tracks and region-to-track assignments
    let audio_tracks = tracks::parse_audio_tracks(
        &blocks,
        &cursor,
        &audio_regions,
        version,
        rate_factor,
        &tempo_segments,
        target_sample_rate,
    );

    // Step 11: Parse MIDI
    let (midi_regions, midi_tracks) = midi::parse_midi(&blocks, &cursor, version, rate_factor);

    Ok(ProToolsSession {
        version,
        session_sample_rate,
        bpm,
        tempo_events,
        meter_events,
        markers,
        audio_files,
        audio_regions,
        audio_tracks,
        midi_regions,
        midi_tracks,
    })
}

/// Find the session sample rate from block type 0x1028.
fn parse_sample_rate(blocks: &[Block], cursor: &Cursor<'_>) -> Option<u32> {
    fn find_recursive(blocks: &[Block], ct: ContentType) -> Option<&Block> {
        for block in blocks {
            if block.content_type == Some(ct) {
                return Some(block);
            }
            if let Some(found) = find_recursive(&block.children, ct) {
                return Some(found);
            }
        }
        None
    }

    let block = find_recursive(blocks, ContentType::SessionSampleRate)?;
    // Sample rate is a u32 at offset + 4
    // (offset+0,1 = content_type, offset+2,3 = flags, offset+4 = sample rate)
    Some(cursor.u32_at(block.offset + 4))
}

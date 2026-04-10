//! Top-level parse orchestration.
//!
//! The main parse flow:
//! 1. Decrypt the file
//! 2. Detect the Pro Tools version
//! 3. Parse the block tree
//! 4. Extract session metadata (sample rate)
//! 5. Extract audio file references
//! 6. Extract regions and region-to-track assignments
//! 7. Extract MIDI data

pub mod audio;
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
use crate::types::ProToolsSession;

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

    // Step 6: Parse audio files
    let audio_files = audio::parse_audio_files(&blocks, &cursor, version);

    // Step 7: Parse regions
    let audio_regions = regions::parse_audio_regions(&blocks, &cursor, version, rate_factor);

    // Step 8: Parse tempo map for tick→sample conversion
    let tempo_map = tempo::parse_tempo_map(&blocks, &cursor, target_sample_rate);

    // Step 9: Parse tracks and region-to-track assignments
    let audio_tracks = tracks::parse_audio_tracks(
        &blocks,
        &cursor,
        &audio_regions,
        version,
        rate_factor,
        &tempo_map,
        target_sample_rate,
    );

    // Step 10: Parse MIDI
    let (midi_regions, midi_tracks) = midi::parse_midi(&blocks, &cursor, version, rate_factor);

    Ok(ProToolsSession {
        version,
        session_sample_rate,
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

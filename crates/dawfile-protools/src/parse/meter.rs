//! Meter map and marker parsing.
//!
//! ## Meter block (0x2029)
//!
//! Layout:
//! ```text
//! [0-4]   "Meter"  (5 bytes)
//! [5-6]   u16      (always 02 00)
//! [7-10]  u32      payload size
//! [11-14] u32      entry count  N
//! [15 .. 15+N*36]  primary entries (36 bytes each):
//!     [0-7]   u64  absolute tick position
//!     [8-11]  u32  bar number (1-based)
//!     [12-15] u32  numerator
//!     [16-19] u32  denominator
//!     [20-35] 16 bytes of other fields (ignored)
//! [..]    N * 16 bytes secondary back-reference entries (ignored)
//! [..]    10-byte trailing footer
//! ```
//!
//! ## Markers (0x271a / 0x2619)
//!
//! Pro Tools "Memory Locations" are children of the 0x271a block (type
//! `MarkerList`).  The first child (type 0x2619) named `"Markers"` is the
//! container; additional 0x2619 children within it would be the individual
//! marker entries.  None of the current fixture files contain user-defined
//! memory locations, so this returns an empty Vec for now.

use crate::block::Block;
use crate::content_type::ContentType;
use crate::cursor::Cursor;
use crate::parse::tempo::{TempoSegment, tick_to_sample};
use crate::types::{Marker, MeterEvent, ZERO_TICKS};

/// Byte size of each primary meter entry.
const ENTRY_SIZE: usize = 36;
/// Byte offset of the entry count within the "Meter" block.
const COUNT_OFFSET: usize = 11;
/// Byte offset of the first primary entry.
const FIRST_ENTRY_OFFSET: usize = 15;

/// Parse the meter (time-signature) map from the first `0x2029` block.
///
/// Returns a sorted list of meter events. Empty if the session has no meter
/// block or the block contains no entries.
pub fn parse_meter_events(
    blocks: &[Block],
    cursor: &Cursor<'_>,
    tempo_map: &[TempoSegment],
    target_sample_rate: u32,
) -> Vec<MeterEvent> {
    let meter_block = match find_block_recursive(blocks, ContentType::MeterBlock) {
        Some(b) => b,
        None => return Vec::new(),
    };

    let data = cursor.data();
    let block_start = meter_block.offset + 2;
    let block_end = (block_start + meter_block.block_size as usize).min(data.len());

    if block_start + COUNT_OFFSET + 4 > block_end {
        return Vec::new();
    }

    // Verify the "Meter" magic at the start.
    if data[block_start..].get(..5) != Some(b"Meter") {
        return Vec::new();
    }

    let count_bytes: [u8; 4] = data[block_start + COUNT_OFFSET..block_start + COUNT_OFFSET + 4]
        .try_into()
        .unwrap();
    let count = u32::from_le_bytes(count_bytes) as usize;

    if count == 0 {
        return Vec::new();
    }

    let mut events = Vec::with_capacity(count);

    for i in 0..count {
        let entry_start = block_start + FIRST_ENTRY_OFFSET + i * ENTRY_SIZE;
        if entry_start + ENTRY_SIZE > block_end {
            break;
        }

        // u64 absolute tick position
        let tick_bytes: [u8; 8] = data[entry_start..entry_start + 8].try_into().unwrap();
        let tick_abs = u64::from_le_bytes(tick_bytes);
        let tick_start = tick_abs.saturating_sub(ZERO_TICKS);

        // u32 bar number
        let measure_bytes: [u8; 4] = data[entry_start + 8..entry_start + 12].try_into().unwrap();
        let measure = u32::from_le_bytes(measure_bytes);

        // u32 numerator
        let numer_bytes: [u8; 4] = data[entry_start + 12..entry_start + 16].try_into().unwrap();
        let numerator = u32::from_le_bytes(numer_bytes);

        // u32 denominator
        let denom_bytes: [u8; 4] = data[entry_start + 16..entry_start + 20].try_into().unwrap();
        let denominator = u32::from_le_bytes(denom_bytes);

        if numerator == 0 || denominator == 0 {
            continue;
        }

        let sample_start = tick_to_sample(tick_start, tempo_map, target_sample_rate);

        events.push(MeterEvent {
            tick_start,
            sample_start,
            measure,
            numerator,
            denominator,
        });
    }

    events
}

/// Parse user-defined markers from the `0x271a` (MarkerList) block.
///
/// Pro Tools stores markers as "Memory Locations". None of the current
/// fixture files contain user-defined memory locations, so this always
/// returns an empty Vec until the format is confirmed against a fixture
/// that has real markers.
pub fn parse_markers(
    _blocks: &[Block],
    _cursor: &Cursor<'_>,
    _tempo_map: &[TempoSegment],
    _target_sample_rate: u32,
) -> Vec<Marker> {
    Vec::new()
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

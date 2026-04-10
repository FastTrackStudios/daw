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
/// ## Block 0x2619 — MarkerEntry layout
///
/// ```text
/// [0..1]         u16   content_type = 0x2619
/// [2..5]         u32   name_len
/// [6..6+N]       [u8]  name (N = name_len bytes)
/// [6+N]          u8    marker_class:
///                        0x00 = system / built-in (Tempo, Meter, Key Signature, Chord Symbols)
///                        0x01 = user-defined memory location
/// [6+N+1]        u8    (unknown, 0x00)
/// [6+N+2]        u8    (unknown, 0x01)
/// [6+N+3..6+N+6] [u8;4] zeros
/// [6+N+7..6+N+10]  u32 = 42 (field-length constant)
/// [6+N+11..6+N+18] [u8;8] unique record identifier (UID, not a position)
/// [6+N+19..6+N+22] [u8;4] zeros
/// [6+N+23..6+N+26] u32 = 42
/// [6+N+27..6+N+34] [u8;8] same UID (duplicate cross-reference)
/// [6+N+35..6+N+42] [u8;8] zeros
/// [6+N+43..6+N+46] u32  sequential_number (1-based order in the session)
/// ...                    child block (0x4301, all zeros — position encoding TBD)
/// ```
///
/// The tick/sample position encoding in 0x2619 blocks has not been fully
/// reverse-engineered; `sample_pos` and `tick_pos` are reported as 0.
pub fn parse_markers(
    blocks: &[Block],
    cursor: &Cursor<'_>,
    _tempo_map: &[TempoSegment],
    _target_sample_rate: u32,
) -> Vec<Marker> {
    let user_marker_container = match find_block_recursive(blocks, ContentType::UserMarkerContainer)
    {
        Some(b) => b,
        None => return Vec::new(),
    };

    let data = cursor.data();
    let mut markers = Vec::new();
    let mut number: u32 = 0;

    for child in &user_marker_container.children {
        if child.content_type != Some(ContentType::MarkerEntry) {
            continue;
        }

        let base = child.offset; // points at content_type field

        // Need at least 6 bytes for content_type(2) + name_len(4)
        if base + 6 > data.len() {
            continue;
        }

        let name_len = u32::from_le_bytes(data[base + 2..base + 6].try_into().unwrap()) as usize;
        if name_len == 0 || base + 6 + name_len > data.len() {
            continue;
        }

        let name = String::from_utf8_lossy(&data[base + 6..base + 6 + name_len]).into_owned();

        // Marker class byte: 0x00 = system/built-in, 0x01 = user-defined
        let marker_class = data.get(base + 6 + name_len).copied().unwrap_or(0);
        if marker_class != 0x01 {
            // Skip system markers (Tempo, Meter, Key Signature, Chord Symbols)
            continue;
        }

        number += 1;

        // Try to read the sequential number from the fixed offset.
        // Formula: offset = 6(ct) + name_len + 43(fixed fields before number)
        let num_offset = base + 6 + name_len + 43;
        let seq_number = if num_offset + 4 <= data.len() {
            u32::from_le_bytes(data[num_offset..num_offset + 4].try_into().unwrap())
        } else {
            number
        };

        markers.push(Marker {
            name,
            number: if seq_number > 0 { seq_number } else { number },
            // Position encoding in 0x2619 blocks is not yet decoded.
            tick_pos: 0,
            sample_pos: 0,
        });
    }

    markers
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

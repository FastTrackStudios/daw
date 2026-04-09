//! Audio region parsing.
//!
//! Regions are parsed from block types 0x100b (PT 5-9) or 0x262a (PT 10+).
//! Each region has a name, index, and three-point position data (start,
//! sample offset, length) encoded with variable-width integers.

use crate::block::Block;
use crate::content_type::ContentType;
use crate::cursor::{self, Cursor};
use crate::types::AudioRegion;

/// Parse all audio regions from the block tree.
pub fn parse_audio_regions(
    blocks: &[Block],
    cursor: &Cursor<'_>,
    version: u16,
    rate_factor: f64,
) -> Vec<AudioRegion> {
    let mut regions = Vec::new();

    // Choose the region list block type based on version
    let (list_ct, region_ct) = if version < 10 {
        (ContentType::AudioRegionListOld, ContentType::AudioRegionOld)
    } else {
        (ContentType::AudioRegionListNew, ContentType::AudioRegionNew)
    };

    let region_list = match find_block_recursive(blocks, list_ct) {
        Some(b) => b,
        None => return regions,
    };

    let region_blocks = region_list.find_all(region_ct);

    for (idx, block) in region_blocks.iter().enumerate() {
        if let Some(region) = parse_single_region(block, cursor, idx as u16, rate_factor) {
            regions.push(region);
        }
    }

    regions
}

/// Parse a single audio region from its block.
fn parse_single_region(
    block: &Block,
    cursor: &Cursor<'_>,
    index: u16,
    rate_factor: f64,
) -> Option<AudioRegion> {
    let data = cursor.data();

    // Region name is a length-prefixed string at offset + 11
    let name_offset = block.offset + 11;
    if name_offset + 4 >= data.len() {
        return None;
    }

    let (name, str_consumed) = cursor.length_prefixed_string(name_offset);

    // Three-point data starts immediately after the name (str_consumed already includes the 4-byte prefix)
    let three_point_offset = name_offset + str_consumed;
    if three_point_offset + 20 >= data.len() {
        return None;
    }

    let (start, sample_offset, length) = cursor::parse_three_point(cursor, three_point_offset);

    // Audio file index is a u32 at (block.offset + block.block_size)
    // This is right at the end of the block's payload
    let findex_offset = block.offset + block.block_size as usize;
    let audio_file_index = if findex_offset + 4 <= data.len() {
        cursor.u32_at(findex_offset) as u16
    } else {
        0
    };

    Some(AudioRegion {
        name,
        index,
        start_pos: (start as f64 * rate_factor) as u64,
        sample_offset: (sample_offset as f64 * rate_factor) as u64,
        length: (length as f64 * rate_factor) as u64,
        audio_file_index,
    })
}

/// Recursively find a block by content type.
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

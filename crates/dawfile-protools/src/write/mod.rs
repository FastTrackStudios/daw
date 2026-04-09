//! Round-trip write support for Pro Tools session files.
//!
//! The strategy: read a .ptx file, make targeted modifications to the
//! decrypted byte buffer, then re-encrypt and write. Unmodified blocks
//! pass through byte-for-byte identical.
//!
//! # Modification tiers
//!
//! ## Tier 1: Fixed-size in-place (safe, tested)
//! - Sample rate, region positions, numeric fields
//! - Track names of the **same byte length** (padded/truncated)
//!
//! ## Tier 2: Variable-length splice (experimental)
//! - Track names of **different lengths** — splices bytes and updates
//!   all ancestor block sizes. The block tree is rebuilt after each splice.
//! - Region names (same mechanism)
//!
//! ## Tier 3: Structural changes (not yet supported)
//! - Adding/removing blocks, tracks, regions
//! - Changing cross-reference indices

pub mod splice;

use crate::content_type::ContentType;
use crate::raw_block::RawSession;

/// Write a modified session back to disk.
///
/// Encrypts the decrypted buffer and writes it to the given path.
pub fn write_session(
    session: &RawSession,
    path: impl AsRef<std::path::Path>,
) -> crate::PtResult<()> {
    let encrypted = session.encrypt();
    std::fs::write(path, &encrypted)?;
    Ok(())
}

/// Overwrite the session sample rate.
///
/// Finds the 0x1028 block and writes the new rate at offset+4.
pub fn set_sample_rate(session: &mut RawSession, sample_rate: u32) -> bool {
    if let Some(block) = session.find_block(ContentType::SessionSampleRate) {
        let offset = block.start + 7; // offset = pos + 7
        if offset + 8 <= session.data.len() {
            session.write_u32(offset + 4, sample_rate);
            return true;
        }
    }
    false
}

/// Overwrite a track name in-place.
///
/// The new name must be the **exact same byte length** as the original,
/// because changing the length would shift all subsequent data and break
/// block size fields. Pad with spaces or truncate as needed.
///
/// Returns `true` if the track was found and the name was replaced.
pub fn set_track_name_inplace(
    session: &mut RawSession,
    track_index: usize,
    new_name: &str,
) -> bool {
    // Find track name blocks (0x1014)
    let track_blocks = collect_content_type(&session.blocks, ContentType::AudioTrackInfo);

    if track_index >= track_blocks.len() {
        return false;
    }

    let block_start = track_blocks[track_index];
    let offset = block_start + 7; // content_type field
    let name_offset = offset + 2; // name starts at offset + 2

    let cursor = session.cursor();
    if name_offset + 4 >= cursor.len() {
        return false;
    }

    let str_len = cursor.u32_at(name_offset) as usize;
    let str_data_start = name_offset + 4;
    let str_data_end = str_data_start + str_len;

    if str_data_end > session.data.len() {
        return false;
    }

    // Build padded/truncated name
    let name_bytes = new_name.as_bytes();
    let mut padded = vec![0x20u8; str_len]; // pad with spaces
    let copy_len = name_bytes.len().min(str_len);
    padded[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

    // Write the name bytes (length field stays the same)
    session.data[str_data_start..str_data_end].copy_from_slice(&padded);

    true
}

/// Rename a track, allowing a different-length name.
///
/// This is a **Tier 2** operation: it splices the string bytes and updates
/// all ancestor block sizes. The block tree is rebuilt afterward.
///
/// Returns the byte-level size delta, or `None` if the track wasn't found.
pub fn rename_track(session: &mut RawSession, track_index: usize, new_name: &str) -> Option<i64> {
    // Find the Nth AudioTrackInfo (0x1014) block
    let track_starts = collect_content_type(&session.blocks, ContentType::AudioTrackInfo);

    if track_index >= track_starts.len() {
        return None;
    }

    // The string is at block.start + 7 (content_type) + 2 = start + 9
    let string_offset = track_starts[track_index] + 9;

    Some(splice::replace_string(session, string_offset, new_name))
}

/// Rename a region, allowing a different-length name.
///
/// Works for both old-format (0x1008) and new-format (0x2629) audio regions.
/// The `region_index` is the position in the region list.
pub fn rename_region(
    session: &mut RawSession,
    region_index: usize,
    new_name: &str,
    version: u16,
) -> Option<i64> {
    let region_ct = if version < 10 {
        ContentType::AudioRegionOld
    } else {
        ContentType::AudioRegionNew
    };

    let region_starts = collect_content_type(&session.blocks, region_ct);
    if region_index >= region_starts.len() {
        return None;
    }

    // Region name is at block.start + 7 (content_type) + 11
    let string_offset = region_starts[region_index] + 7 + 11;

    Some(splice::replace_string(session, string_offset, new_name))
}

/// Collect the `start` positions of all blocks with a given content type.
fn collect_content_type(blocks: &[crate::raw_block::RawBlock], ct: ContentType) -> Vec<usize> {
    let mut result = Vec::new();
    collect_ct_recursive(blocks, ct, &mut result);
    result
}

fn collect_ct_recursive(
    blocks: &[crate::raw_block::RawBlock],
    ct: ContentType,
    out: &mut Vec<usize>,
) {
    for block in blocks {
        if block.content_type == Some(ct) {
            out.push(block.start);
        }
        collect_ct_recursive(&block.children, ct, out);
    }
}

//! Round-trip tests: read → write → read should produce identical results.
//!
//! These tests verify that:
//! 1. decrypt → encrypt produces the original file bytes
//! 2. parse_raw preserves the block tree structure
//! 3. In-place modifications (track names, sample rate) survive round-trip

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture_path(name: &str) -> String {
    format!("{FIXTURES}/{name}")
}

// =============================================================================
// Core round-trip: decrypt → encrypt = identity
// =============================================================================

#[test]
fn round_trip_pt12_region_test() {
    round_trip_identity("RegionTest.ptx");
}

#[test]
fn round_trip_pt12_test_ptx() {
    round_trip_identity("TestPTX.ptx");
}

#[test]
fn round_trip_pt8_playlists() {
    round_trip_identity("goodplaylists2.ptf");
}

#[test]
fn round_trip_pt5_damien() {
    round_trip_identity("Damien_monos.pts");
}

/// Verify that decrypt → encrypt produces byte-identical output.
fn round_trip_identity(filename: &str) {
    let original = std::fs::read(fixture_path(filename))
        .unwrap_or_else(|e| panic!("failed to read {filename}: {e}"));

    let session = dawfile_protools::parse_raw(original.clone())
        .unwrap_or_else(|e| panic!("failed to parse {filename}: {e}"));

    let re_encrypted = session.encrypt();

    assert_eq!(
        original.len(),
        re_encrypted.len(),
        "{filename}: length mismatch"
    );

    // Find first differing byte for diagnostics
    for (i, (a, b)) in original.iter().zip(re_encrypted.iter()).enumerate() {
        assert_eq!(
            a, b,
            "{filename}: byte mismatch at offset 0x{i:04x} (original=0x{a:02x}, re-encrypted=0x{b:02x})"
        );
    }
}

// =============================================================================
// Raw block tree preserves structure
// =============================================================================

#[test]
fn raw_blocks_match_parsed_blocks() {
    let original = std::fs::read(fixture_path("RegionTest.ptx")).unwrap();
    let session = dawfile_protools::parse_raw(original.clone()).unwrap();

    // Also parse with the regular parser to compare block counts
    let mut data_copy = original;
    let _ = dawfile_protools::decrypt::decrypt(&mut data_copy).unwrap();
    let is_be = data_copy[0x11] != 0;
    let regular_blocks = dawfile_protools::block::parse_blocks(&data_copy, is_be);

    assert_eq!(
        session.blocks.len(),
        regular_blocks.len(),
        "top-level block count should match"
    );

    // Verify content types match
    for (raw, parsed) in session.blocks.iter().zip(regular_blocks.iter()) {
        assert_eq!(
            raw.content_type_raw, parsed.content_type_raw,
            "content type mismatch at offset 0x{:04x}",
            raw.start
        );
    }
}

// =============================================================================
// In-place modification: sample rate
// =============================================================================

#[test]
fn modify_sample_rate_round_trip() {
    let original = std::fs::read(fixture_path("RegionTest.ptx")).unwrap();
    let mut session = dawfile_protools::parse_raw(original).unwrap();

    // Original sample rate is 44100
    let cursor = session.cursor();
    let sr_block = session
        .find_block(dawfile_protools::content_type::ContentType::SessionSampleRate)
        .expect("should find sample rate block");
    let original_sr = cursor.u32_at(sr_block.start + 7 + 4);
    assert_eq!(original_sr, 44100);

    // Change to 48000
    assert!(dawfile_protools::write::set_sample_rate(
        &mut session,
        48000
    ));

    // Encrypt, then re-parse and verify
    let encrypted = session.encrypt();
    let re_parsed = dawfile_protools::parse_raw(encrypted).unwrap();
    let cursor2 = re_parsed.cursor();
    let sr_block2 = re_parsed
        .find_block(dawfile_protools::content_type::ContentType::SessionSampleRate)
        .unwrap();
    let new_sr = cursor2.u32_at(sr_block2.start + 7 + 4);
    assert_eq!(new_sr, 48000);
}

// =============================================================================
// In-place modification: track name
// =============================================================================

#[test]
fn modify_track_name_round_trip() {
    let original = std::fs::read(fixture_path("RegionTest.ptx")).unwrap();
    let mut session = dawfile_protools::parse_raw(original).unwrap();

    // Read original track name
    let cursor = session.cursor();
    let track_blocks: Vec<_> = {
        let mut out = Vec::new();
        collect_ct(
            &session.blocks,
            dawfile_protools::content_type::ContentType::AudioTrackInfo,
            &mut out,
        );
        out
    };
    assert!(!track_blocks.is_empty(), "should have audio tracks");

    let first_offset = track_blocks[0] + 7; // content_type field
    let (original_name, _) = cursor.length_prefixed_string(first_offset + 2);
    assert_eq!(original_name, "Track_Name");

    // Rename to same length
    assert!(dawfile_protools::write::set_track_name_inplace(
        &mut session,
        0,
        "New__Track"
    ));

    // Verify in decrypted buffer
    let cursor = session.cursor();
    let (new_name, _) = cursor.length_prefixed_string(first_offset + 2);
    assert_eq!(new_name, "New__Track");

    // Encrypt → decrypt → verify survives round-trip
    let encrypted = session.encrypt();
    let re_parsed = dawfile_protools::parse_raw(encrypted).unwrap();
    let cursor2 = re_parsed.cursor();
    let track_blocks2: Vec<_> = {
        let mut out = Vec::new();
        collect_ct(
            &re_parsed.blocks,
            dawfile_protools::content_type::ContentType::AudioTrackInfo,
            &mut out,
        );
        out
    };
    let (rt_name, _) = cursor2.length_prefixed_string(track_blocks2[0] + 7 + 2);
    assert_eq!(rt_name, "New__Track");
}

// =============================================================================
// Tier 2: Variable-length splice (rename with different length)
// =============================================================================

#[test]
fn rename_track_shorter() {
    let original = std::fs::read(fixture_path("RegionTest.ptx")).unwrap();
    let original_len = original.len();
    let mut session = dawfile_protools::parse_raw(original).unwrap();

    // Original: "Track_Name" (10 bytes)
    let cursor = session.cursor();
    let tracks = get_track_starts(&session);
    let (name, _) = cursor.length_prefixed_string(tracks[0] + 9);
    assert_eq!(name, "Track_Name");

    // Rename to shorter: "Tk" (2 bytes) — delta = -8
    let delta = dawfile_protools::write::rename_track(&mut session, 0, "Tk");
    assert_eq!(delta, Some(-8));
    assert_eq!(session.data.len(), original_len - 8);

    // Verify the name changed
    let tracks2 = get_track_starts(&session);
    let cursor2 = session.cursor();
    let (new_name, _) = cursor2.length_prefixed_string(tracks2[0] + 9);
    assert_eq!(new_name, "Tk");

    // Round-trip: encrypt → decrypt → verify
    let encrypted = session.encrypt();
    let re_parsed = dawfile_protools::parse_raw(encrypted).unwrap();
    let tracks3 = get_track_starts(&re_parsed);
    let cursor3 = re_parsed.cursor();
    let (rt_name, _) = cursor3.length_prefixed_string(tracks3[0] + 9);
    assert_eq!(rt_name, "Tk");

    // Block tree should survive splice
    assert!(
        !re_parsed.blocks.is_empty(),
        "block tree should survive splice"
    );
}

#[test]
fn rename_track_longer() {
    let original = std::fs::read(fixture_path("RegionTest.ptx")).unwrap();
    let original_len = original.len();
    let mut session = dawfile_protools::parse_raw(original).unwrap();

    // "Track_Name" (10) → "Extended_Track_Name_Here" (24) — delta = +14
    let delta = dawfile_protools::write::rename_track(&mut session, 0, "Extended_Track_Name_Here");
    assert_eq!(delta, Some(14));
    assert_eq!(session.data.len(), original_len + 14);

    // Verify
    let tracks = get_track_starts(&session);
    let cursor = session.cursor();
    let (name, _) = cursor.length_prefixed_string(tracks[0] + 9);
    assert_eq!(name, "Extended_Track_Name_Here");

    // Round-trip
    let encrypted = session.encrypt();
    let re_parsed = dawfile_protools::parse_raw(encrypted).unwrap();
    let tracks2 = get_track_starts(&re_parsed);
    let cursor2 = re_parsed.cursor();
    let (rt_name, _) = cursor2.length_prefixed_string(tracks2[0] + 9);
    assert_eq!(rt_name, "Extended_Track_Name_Here");
}

#[test]
fn rename_track_preserves_other_data() {
    let original = std::fs::read(fixture_path("RegionTest.ptx")).unwrap();
    let mut session = dawfile_protools::parse_raw(original).unwrap();

    // Read original sample rate before rename
    let cursor = session.cursor();
    let sr_block = session
        .find_block(dawfile_protools::content_type::ContentType::SessionSampleRate)
        .unwrap();
    let sr_before = cursor.u32_at(sr_block.start + 7 + 4);

    // Rename track (changes file size, shifts blocks)
    dawfile_protools::write::rename_track(&mut session, 0, "Short");

    // Sample rate should still be correct after the splice
    let sr_block2 = session
        .find_block(dawfile_protools::content_type::ContentType::SessionSampleRate)
        .unwrap();
    let cursor2 = session.cursor();
    let sr_after = cursor2.u32_at(sr_block2.start + 7 + 4);
    assert_eq!(sr_before, sr_after, "sample rate should survive splice");

    // WavList should still exist
    let wav_list = session
        .find_block(dawfile_protools::content_type::ContentType::WavList)
        .expect("WavList should survive splice");
    assert!(!wav_list.children.is_empty());
}

fn get_track_starts(session: &dawfile_protools::RawSession) -> Vec<usize> {
    let mut out = Vec::new();
    collect_ct(
        &session.blocks,
        dawfile_protools::content_type::ContentType::AudioTrackInfo,
        &mut out,
    );
    out
}

fn collect_ct(
    blocks: &[dawfile_protools::raw_block::RawBlock],
    ct: dawfile_protools::content_type::ContentType,
    out: &mut Vec<usize>,
) {
    for block in blocks {
        if block.content_type == Some(ct) {
            out.push(block.start);
        }
        collect_ct(&block.children, ct, out);
    }
}

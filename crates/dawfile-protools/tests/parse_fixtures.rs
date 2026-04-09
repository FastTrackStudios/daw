//! Integration tests that parse the ptformat test fixtures.
//!
//! These tests validate our Rust parser against the expected output
//! from the reference C++ ptformat library.

use dawfile_protools::read_session;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture_path(name: &str) -> String {
    format!("{FIXTURES}/{name}")
}

#[test]
fn parse_pt12_region_test() {
    // RegionTest.ptx — PT12, 44100Hz session, target 48000Hz
    let session =
        read_session(fixture_path("RegionTest.ptx"), 48000).expect("should parse RegionTest.ptx");

    assert_eq!(session.version, 12);
    assert_eq!(session.session_sample_rate, 44100);

    // Expected: 1 wav, 3 regions, 6 active track regions
    assert_eq!(session.audio_files.len(), 1, "should have 1 wav file");
    assert_eq!(session.audio_files[0].filename, "region_name_WAV.wav");

    assert_eq!(
        session.audio_regions.len(),
        3,
        "should have 3 audio regions"
    );
    assert_eq!(session.audio_regions[0].name, "region_name_region");
    assert_eq!(session.audio_regions[1].name, "region_name_region-01");
    assert_eq!(session.audio_regions[2].name, "region_name_region-03");

    // 6 active region assignments across tracks
    let total_active: usize = session.audio_tracks.iter().map(|t| t.regions.len()).sum();
    assert_eq!(total_active, 6, "should have 6 active region assignments");
}

#[test]
fn parse_pt12_midi_test() {
    // TestPTX.ptx — PT12, 48000Hz, has MIDI
    let session =
        read_session(fixture_path("TestPTX.ptx"), 48000).expect("should parse TestPTX.ptx");

    assert_eq!(session.version, 12);
    assert_eq!(session.session_sample_rate, 48000);

    // Expected: 4 wavs, 4 audio regions, 3 active audio tracks
    assert_eq!(session.audio_files.len(), 4, "should have 4 wav files");
    assert_eq!(
        session.audio_regions.len(),
        4,
        "should have 4 audio regions"
    );

    // Expected: 3 MIDI regions
    assert_eq!(session.midi_regions.len(), 3, "should have 3 MIDI regions");

    if !session.midi_regions.is_empty() {
        let mr0 = &session.midi_regions[0];
        assert_eq!(mr0.name, "MIDI 1-01");
        assert_eq!(mr0.events.len(), 9, "MIDI 1-01 should have 9 events");

        let mr1 = &session.midi_regions[1];
        assert_eq!(mr1.name, "MIDI 2-01");
        assert_eq!(mr1.events.len(), 16, "MIDI 2-01 should have 16 events");

        let mr2 = &session.midi_regions[2];
        assert_eq!(mr2.name, "MIDI 3-01");
        assert_eq!(mr2.events.len(), 4, "MIDI 3-01 should have 4 events");
    }
}

#[test]
fn parse_pt8_playlists() {
    // goodplaylists2.ptf — PT8, 48000Hz
    let session = read_session(fixture_path("goodplaylists2.ptf"), 48000)
        .expect("should parse goodplaylists2.ptf");

    assert_eq!(session.version, 8);
    assert_eq!(session.session_sample_rate, 48000);

    // Expected: 3 wavs, 6 regions
    assert_eq!(session.audio_files.len(), 3, "should have 3 wav files");
    assert_eq!(
        session.audio_regions.len(),
        6,
        "should have 6 audio regions"
    );

    // Expected: 13 active region assignments across 4 tracks
    let total_active: usize = session.audio_tracks.iter().map(|t| t.regions.len()).sum();
    assert_eq!(total_active, 13, "should have 13 active region assignments");
}

#[test]
fn parse_pt5_damien_monos() {
    // Damien_monos.pts — PT5, 48000Hz
    let session = read_session(fixture_path("Damien_monos.pts"), 48000)
        .expect("should parse Damien_monos.pts");

    assert_eq!(session.version, 5);
    assert_eq!(session.session_sample_rate, 48000);

    // Expected: 8 wavs, 8 regions, 8 tracks
    assert_eq!(session.audio_files.len(), 8, "should have 8 wav files");
    assert_eq!(
        session.audio_regions.len(),
        8,
        "should have 8 audio regions"
    );
    assert_eq!(session.audio_tracks.len(), 8, "should have 8 audio tracks");
}

#[test]
fn parse_pt5_fa_16_48() {
    let session =
        read_session(fixture_path("Fa_16_48.pts"), 48000).expect("should parse Fa_16_48.pts");

    assert_eq!(session.version, 5);
}

#[test]
fn parse_pt5_for_ardour() {
    let session =
        read_session(fixture_path("forArdour.pts"), 48000).expect("should parse forArdour.pts");

    assert_eq!(session.version, 5);
}

#[test]
fn parse_pt8_midi() {
    let session =
        read_session(fixture_path("midi345x.ptf"), 48000).expect("should parse midi345x.ptf");

    assert!(session.version >= 5);
}

#[test]
fn feature_support_is_read_only() {
    let support = dawfile_protools::feature_support();
    assert!(support.can_read(daw_proto::Capability::Tracks));
    assert!(support.can_read(daw_proto::Capability::Items));
    assert!(!support.can_write(daw_proto::Capability::Tracks));
    assert!(!support.can_write(daw_proto::Capability::Items));
}

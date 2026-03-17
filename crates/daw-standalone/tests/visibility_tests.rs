//! Tests for track visibility in daw-standalone
//!
//! Verifies that the visibility fields and service methods work correctly
//! through the StandaloneTrack implementation.

use daw_proto::{ProjectContext, TrackRef, TrackService};
use daw_standalone::StandaloneTrack;

fn current() -> ProjectContext {
    ProjectContext::Current
}

// =============================================================================
// Basic visibility field tests
// =============================================================================

#[tokio::test]
async fn tracks_default_to_visible() {
    let track_svc = StandaloneTrack::new();
    let tracks = track_svc.get_tracks(current()).await;

    for track in &tracks {
        assert!(
            track.visible_in_tcp,
            "Track '{}' should be visible in TCP by default",
            track.name
        );
        assert!(
            track.visible_in_mixer,
            "Track '{}' should be visible in mixer by default",
            track.name
        );
    }
}

#[tokio::test]
async fn hide_track_in_tcp() {
    let track_svc = StandaloneTrack::new();

    // Get first track
    let tracks = track_svc.get_tracks(current()).await;
    let guid = tracks[0].guid.clone();

    // Hide it
    track_svc
        .set_visible_in_tcp(current(), TrackRef::Guid(guid.clone()), false)
        .await;

    // Verify hidden
    let track = track_svc
        .get_track(current(), TrackRef::Guid(guid))
        .await
        .unwrap();
    assert!(!track.visible_in_tcp);
    assert!(
        track.visible_in_mixer,
        "Mixer visibility should be unaffected"
    );
}

#[tokio::test]
async fn hide_track_in_mixer() {
    let track_svc = StandaloneTrack::new();

    let tracks = track_svc.get_tracks(current()).await;
    let guid = tracks[0].guid.clone();

    track_svc
        .set_visible_in_mixer(current(), TrackRef::Guid(guid.clone()), false)
        .await;

    let track = track_svc
        .get_track(current(), TrackRef::Guid(guid))
        .await
        .unwrap();
    assert!(track.visible_in_tcp, "TCP visibility should be unaffected");
    assert!(!track.visible_in_mixer);
}

#[tokio::test]
async fn toggle_visibility_roundtrip() {
    let track_svc = StandaloneTrack::new();

    let tracks = track_svc.get_tracks(current()).await;
    let guid = tracks[1].guid.clone();
    let track_ref = TrackRef::Guid(guid.clone());

    // Start visible
    let track = track_svc
        .get_track(current(), track_ref.clone())
        .await
        .unwrap();
    assert!(track.visible_in_tcp);

    // Hide
    track_svc
        .set_visible_in_tcp(current(), track_ref.clone(), false)
        .await;
    let track = track_svc
        .get_track(current(), track_ref.clone())
        .await
        .unwrap();
    assert!(!track.visible_in_tcp);

    // Show again
    track_svc
        .set_visible_in_tcp(current(), track_ref.clone(), true)
        .await;
    let track = track_svc
        .get_track(current(), track_ref)
        .await
        .unwrap();
    assert!(track.visible_in_tcp);
}

// =============================================================================
// Batch visibility tests (simulating visibility manager behavior)
// =============================================================================

#[tokio::test]
async fn hide_multiple_tracks_independently() {
    let track_svc = StandaloneTrack::new();

    let tracks = track_svc.get_tracks(current()).await;
    assert!(tracks.len() >= 4, "Need at least 4 default tracks");

    // Hide tracks 0 and 2
    track_svc
        .set_visible_in_tcp(
            current(),
            TrackRef::Guid(tracks[0].guid.clone()),
            false,
        )
        .await;
    track_svc
        .set_visible_in_tcp(
            current(),
            TrackRef::Guid(tracks[2].guid.clone()),
            false,
        )
        .await;

    // Verify: 0 hidden, 1 visible, 2 hidden, 3 visible
    let updated = track_svc.get_tracks(current()).await;
    assert!(!updated[0].visible_in_tcp);
    assert!(updated[1].visible_in_tcp);
    assert!(!updated[2].visible_in_tcp);
    assert!(updated[3].visible_in_tcp);
}

#[tokio::test]
async fn show_all_restores_visibility() {
    let track_svc = StandaloneTrack::new();

    let tracks = track_svc.get_tracks(current()).await;

    // Hide all tracks
    for track in &tracks {
        track_svc
            .set_visible_in_tcp(current(), TrackRef::Guid(track.guid.clone()), false)
            .await;
    }

    // Verify all hidden
    let hidden = track_svc.get_tracks(current()).await;
    for track in &hidden {
        assert!(
            !track.visible_in_tcp,
            "Track '{}' should be hidden",
            track.name
        );
    }

    // Show all
    for track in &tracks {
        track_svc
            .set_visible_in_tcp(current(), TrackRef::Guid(track.guid.clone()), true)
            .await;
    }

    // Verify all visible again
    let shown = track_svc.get_tracks(current()).await;
    for track in &shown {
        assert!(
            track.visible_in_tcp,
            "Track '{}' should be visible after show_all",
            track.name
        );
    }
}

// =============================================================================
// State field propagation (verify to_track() includes all fields)
// =============================================================================

#[tokio::test]
async fn to_track_propagates_all_state() {
    let track_svc = StandaloneTrack::new();

    let tracks = track_svc.get_tracks(current()).await;
    let guid = tracks[0].guid.clone();
    let track_ref = TrackRef::Guid(guid.clone());

    // Set various states
    track_svc
        .set_muted(current(), track_ref.clone(), true)
        .await;
    track_svc
        .set_soloed(current(), track_ref.clone(), true)
        .await;
    track_svc
        .set_visible_in_tcp(current(), track_ref.clone(), false)
        .await;
    track_svc
        .set_volume(current(), track_ref.clone(), 0.5)
        .await;

    // Verify all fields come back correctly from get_track
    let track = track_svc
        .get_track(current(), track_ref)
        .await
        .unwrap();
    assert!(track.muted, "muted should propagate");
    assert!(track.soloed, "soloed should propagate");
    assert!(!track.visible_in_tcp, "visible_in_tcp should propagate");
    assert!(
        track.visible_in_mixer,
        "visible_in_mixer should be unchanged"
    );
    assert!(
        (track.volume - 0.5).abs() < f64::EPSILON,
        "volume should propagate"
    );
}

// =============================================================================
// Track lookup by index (visibility manager uses index-based access)
// =============================================================================

#[tokio::test]
async fn visibility_by_index() {
    let track_svc = StandaloneTrack::new();

    // Hide track at index 2
    track_svc
        .set_visible_in_tcp(current(), TrackRef::Index(2), false)
        .await;

    // Verify by index
    let track = track_svc
        .get_track(current(), TrackRef::Index(2))
        .await
        .unwrap();
    assert!(!track.visible_in_tcp);

    // Other tracks unaffected
    let track0 = track_svc
        .get_track(current(), TrackRef::Index(0))
        .await
        .unwrap();
    assert!(track0.visible_in_tcp);
}

// =============================================================================
// Add track (dynamic tracks maintain visibility defaults)
// =============================================================================

#[tokio::test]
async fn added_tracks_default_to_visible() {
    let track_svc = StandaloneTrack::new();

    let guid = track_svc.add_track("New Track").await;
    let track = track_svc
        .get_track(current(), TrackRef::Guid(guid))
        .await
        .unwrap();

    assert!(track.visible_in_tcp);
    assert!(track.visible_in_mixer);
    assert_eq!(track.name, "New Track");
}

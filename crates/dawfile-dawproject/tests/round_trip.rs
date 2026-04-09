//! Round-trip tests: write a project, read it back, assert key fields match.

use dawfile_dawproject::{
    Application, Arrangement, AutomationLane, AutomationPoint, Channel, ChannelRole, Clip,
    ClipContent, ContentType, DawProject, Interpolation, Lane, LaneContent, Note, ProjectMetadata,
    Send, TimeUnit, Track, Transport, feature_support,
};

fn minimal_project() -> DawProject {
    DawProject {
        version: "1.0".to_string(),
        application: Some(Application {
            name: "FastTrackStudio".to_string(),
            version: "0.1.0".to_string(),
        }),
        metadata: Some(ProjectMetadata {
            title: Some("Test Session".to_string()),
            artist: Some("Test Artist".to_string()),
            ..Default::default()
        }),
        transport: Transport {
            tempo: 132.5,
            numerator: 3,
            denominator: 8,
        },
        tracks: vec![
            Track {
                id: "track-1".to_string(),
                name: "Bass".to_string(),
                color: Some("#FF4400".to_string()),
                content_type: ContentType::Notes,
                channel: Some(Channel {
                    id: "ch-1".to_string(),
                    role: ChannelRole::Regular,
                    audio_channels: 2,
                    volume: 0.8,
                    pan: -0.25,
                    muted: false,
                    sends: vec![Send {
                        target: "ch-master".to_string(),
                        volume: 1.0,
                        pre_fader: false,
                    }],
                    devices: vec![],
                }),
                children: vec![],
            },
            Track {
                id: "track-master".to_string(),
                name: "Master".to_string(),
                color: None,
                content_type: ContentType::Audio,
                channel: Some(Channel {
                    id: "ch-master".to_string(),
                    role: ChannelRole::Master,
                    audio_channels: 2,
                    volume: 1.0,
                    pan: 0.0,
                    muted: false,
                    sends: vec![],
                    devices: vec![],
                }),
                children: vec![],
            },
        ],
        arrangement: Some(Arrangement {
            id: "arr-1".to_string(),
            time_unit: TimeUnit::Beats,
            lanes: vec![
                Lane {
                    id: "lane-1".to_string(),
                    track: "track-1".to_string(),
                    time_unit: None,
                    content: LaneContent::Clips(vec![Clip {
                        id: "clip-1".to_string(),
                        time: 0.0,
                        duration: 4.0,
                        time_unit: None,
                        content_time_unit: Some(TimeUnit::Beats),
                        name: Some("Intro".to_string()),
                        color: None,
                        fade_in: None,
                        fade_out: None,
                        loop_settings: None,
                        content: ClipContent::Notes(vec![
                            Note {
                                time: 0.0,
                                duration: 0.5,
                                channel: 0,
                                key: 40,
                                velocity: 0.8,
                                release_velocity: None,
                            },
                            Note {
                                time: 1.0,
                                duration: 0.5,
                                channel: 0,
                                key: 43,
                                velocity: 0.7,
                                release_velocity: Some(0.5),
                            },
                        ]),
                    }]),
                },
                Lane {
                    id: "lane-auto".to_string(),
                    track: "ch-master".to_string(),
                    time_unit: None,
                    content: LaneContent::Automation(AutomationLane {
                        id: "auto-1".to_string(),
                        target: "ch-1/Volume".to_string(),
                        points: vec![
                            AutomationPoint {
                                time: 0.0,
                                value: 0.8,
                                interpolation: Interpolation::Linear,
                            },
                            AutomationPoint {
                                time: 4.0,
                                value: 1.0,
                                interpolation: Interpolation::Hold,
                            },
                        ],
                    }),
                },
            ],
        }),
        scenes: vec![],
    }
}

#[test]
fn round_trip_minimal() {
    let original = minimal_project();

    let bytes =
        dawfile_dawproject::serialize_project_bytes(&original).expect("serialize should succeed");

    assert!(!bytes.is_empty(), "output bytes should be non-empty");

    let restored = dawfile_dawproject::parse_project_bytes(&bytes)
        .expect("parse round-tripped bytes should succeed");

    // Transport
    assert_eq!(restored.version, "1.0");
    assert!((restored.transport.tempo - 132.5).abs() < 0.001);
    assert_eq!(restored.transport.numerator, 3);
    assert_eq!(restored.transport.denominator, 8);

    // Application
    let app = restored.application.unwrap();
    assert_eq!(app.name, "FastTrackStudio");

    // Metadata
    let meta = restored.metadata.unwrap();
    assert_eq!(meta.title.as_deref(), Some("Test Session"));
    assert_eq!(meta.artist.as_deref(), Some("Test Artist"));

    // Tracks
    assert_eq!(restored.tracks.len(), 2);
    let bass = &restored.tracks[0];
    assert_eq!(bass.id, "track-1");
    assert_eq!(bass.name, "Bass");
    assert_eq!(bass.color.as_deref(), Some("#FF4400"));
    let ch = bass.channel.as_ref().unwrap();
    assert!((ch.volume - 0.8).abs() < 0.001);
    assert!((ch.pan - (-0.25)).abs() < 0.001);
    assert_eq!(ch.sends.len(), 1);
    assert_eq!(ch.sends[0].target, "ch-master");

    // Arrangement
    let arr = restored.arrangement.unwrap();
    assert_eq!(arr.id, "arr-1");
    assert_eq!(arr.time_unit, TimeUnit::Beats);
    assert_eq!(arr.lanes.len(), 2);

    // Clips lane
    if let LaneContent::Clips(clips) = &arr.lanes[0].content {
        assert_eq!(clips.len(), 1);
        let clip = &clips[0];
        assert_eq!(clip.name.as_deref(), Some("Intro"));
        assert!((clip.duration - 4.0).abs() < 0.001);
        if let ClipContent::Notes(notes) = &clip.content {
            assert_eq!(notes.len(), 2);
            assert_eq!(notes[0].key, 40);
            assert_eq!(notes[1].key, 43);
            assert!(notes[1].release_velocity.is_some());
        } else {
            panic!("expected Notes content");
        }
    } else {
        panic!("expected Clips lane");
    }

    // Automation lane
    if let LaneContent::Automation(auto_lane) = &arr.lanes[1].content {
        assert_eq!(auto_lane.target, "ch-1/Volume");
        assert_eq!(auto_lane.points.len(), 2);
        assert_eq!(auto_lane.points[0].interpolation, Interpolation::Linear);
        assert_eq!(auto_lane.points[1].interpolation, Interpolation::Hold);
    } else {
        panic!("expected Automation lane");
    }
}

#[test]
fn feature_support_is_read_write() {
    use daw_proto::capability::Capability;

    let support = feature_support();
    // Write support was added — verify key capabilities
    assert!(support.can_read(Capability::Tracks));
    assert!(support.can_write(Capability::Tracks));
    assert!(support.can_read(Capability::Midi));
    assert!(support.can_write(Capability::Automation));
}

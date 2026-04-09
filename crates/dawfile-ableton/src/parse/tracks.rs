//! Track parsing for all Ableton track types.
//!
//! Tracks live under `LiveSet.Tracks` and are differentiated by element name:
//! `<MidiTrack>`, `<AudioTrack>`, `<GroupTrack>`, `<ReturnTrack>`.
//!
//! Return tracks live under `LiveSet.ReturnTracks` (not inside `Tracks`).
//! The master track is `LiveSet.MasterTrack` (or `MainTrack` in v12+).

use super::automation;
use super::clips;
use super::devices;
use super::xml_helpers::*;
use crate::types::*;
use roxmltree::Node;

/// Parse all tracks from the `<Tracks>` element.
pub fn parse_tracks(
    tracks_node: Node<'_, '_>,
    version: &AbletonVersion,
) -> (Vec<AudioTrack>, Vec<MidiTrack>, Vec<GroupTrack>) {
    let mut audio_tracks = Vec::new();
    let mut midi_tracks = Vec::new();
    let mut group_tracks = Vec::new();

    for track in tracks_node.children() {
        match track.tag_name().name() {
            "AudioTrack" => {
                if let Some(t) = parse_audio_track(track, version) {
                    audio_tracks.push(t);
                }
            }
            "MidiTrack" => {
                if let Some(t) = parse_midi_track(track, version) {
                    midi_tracks.push(t);
                }
            }
            "GroupTrack" => {
                group_tracks.push(GroupTrack {
                    common: parse_track_common(track, version),
                });
            }
            _ => {}
        }
    }

    (audio_tracks, midi_tracks, group_tracks)
}

/// Parse return tracks from the `<ReturnTracks>` element (sibling of `<Tracks>`).
pub fn parse_return_tracks(
    return_tracks_node: Node<'_, '_>,
    version: &AbletonVersion,
) -> Vec<ReturnTrack> {
    let mut tracks = Vec::new();
    for track in return_tracks_node.children() {
        if track.has_tag_name("ReturnTrack") {
            tracks.push(ReturnTrack {
                common: parse_track_common(track, version),
            });
        }
    }
    tracks
}

/// Parse the master track.
pub fn parse_master_track(node: Node<'_, '_>, version: &AbletonVersion) -> MasterTrack {
    let mixer = parse_mixer(node);
    let audio_output = descend(node, "DeviceChain.AudioOutputRouting")
        .and_then(|r| child_value(r, "Target"))
        .unwrap_or("")
        .to_string();
    let devices = child(node, "DeviceChain")
        .map(|dc| devices::parse_devices(dc, version))
        .unwrap_or_default();

    MasterTrack {
        mixer,
        audio_output,
        devices,
    }
}

fn parse_track_common(track: Node<'_, '_>, version: &AbletonVersion) -> TrackCommon {
    let id = id_attr(track);

    let (user_name, effective_name, annotation) = child(track, "Name")
        .map(|n| {
            (
                child_value(n, "UserName").unwrap_or("").to_string(),
                child_value(n, "EffectiveName").unwrap_or("").to_string(),
                child_value(n, "Annotation").unwrap_or("").to_string(),
            )
        })
        .unwrap_or_default();

    let color = child_i32(track, "Color")
        .or_else(|| child_i32(track, "ColorIndex"))
        .unwrap_or(0);

    let group_id = child_i32(track, "TrackGroupId").unwrap_or(-1);

    let folded = if version.at_least(10, 0) {
        child_bool(track, "TrackUnfolded")
            .map(|v| !v)
            .unwrap_or(false)
    } else {
        descend(track, "DeviceChain.Mixer")
            .and_then(|m| child_bool(m, "IsFolded"))
            .unwrap_or(false)
    };

    let mixer = parse_mixer(track);

    let devices = child(track, "DeviceChain")
        .map(|dc| devices::parse_devices(dc, version))
        .unwrap_or_default();

    let automation_envelopes = automation::parse_track_automation(track);

    TrackCommon {
        id,
        user_name,
        effective_name,
        annotation,
        color,
        group_id,
        folded,
        mixer,
        devices,
        automation_envelopes,
    }
}

fn parse_audio_track(track: Node<'_, '_>, version: &AbletonVersion) -> Option<AudioTrack> {
    let common = parse_track_common(track, version);

    let arrangement_clips = descend(
        track,
        "DeviceChain.MainSequencer.Sample.ArrangerAutomation.Events",
    )
    .map(|events| clips::parse_audio_clips(events, version))
    .unwrap_or_default();

    let session_clips = descend(track, "DeviceChain.MainSequencer.ClipSlotList")
        .map(|slots| clips::parse_session_audio_clips(slots, version))
        .unwrap_or_default();

    let audio_input = descend(track, "DeviceChain.AudioInputRouting")
        .and_then(|r| child_value(r, "Target"))
        .unwrap_or("")
        .to_string();

    let audio_output = descend(track, "DeviceChain.AudioOutputRouting")
        .and_then(|r| child_value(r, "Target"))
        .unwrap_or("")
        .to_string();

    let monitoring = descend(track, "DeviceChain.MainSequencer")
        .and_then(|s| child_i32(s, "MonitoringEnum"))
        .unwrap_or(0);

    Some(AudioTrack {
        common,
        arrangement_clips,
        session_clips,
        audio_input,
        audio_output,
        monitoring,
    })
}

fn parse_midi_track(track: Node<'_, '_>, version: &AbletonVersion) -> Option<MidiTrack> {
    let common = parse_track_common(track, version);

    let arrangement_clips = descend(
        track,
        "DeviceChain.MainSequencer.ClipTimeable.ArrangerAutomation.Events",
    )
    .map(|events| clips::parse_midi_clips(events, version))
    .unwrap_or_default();

    let session_clips = descend(track, "DeviceChain.MainSequencer.ClipSlotList")
        .map(|slots| clips::parse_session_midi_clips(slots, version))
        .unwrap_or_default();

    let midi_input = descend(track, "DeviceChain.MidiInputRouting")
        .and_then(|r| child_value(r, "Target"))
        .unwrap_or("")
        .to_string();

    let audio_output = descend(track, "DeviceChain.AudioOutputRouting")
        .and_then(|r| child_value(r, "Target"))
        .unwrap_or("")
        .to_string();

    let monitoring = descend(track, "DeviceChain.MainSequencer")
        .and_then(|s| child_i32(s, "MonitoringEnum"))
        .unwrap_or(0);

    Some(MidiTrack {
        common,
        arrangement_clips,
        session_clips,
        midi_input,
        audio_output,
        monitoring,
    })
}

fn parse_mixer(track: Node<'_, '_>) -> MixerState {
    let mixer_node = match descend(track, "DeviceChain.Mixer") {
        Some(m) => m,
        None => return MixerState::default(),
    };

    let volume = child(mixer_node, "Volume")
        .and_then(|v| child_f64(v, "Manual"))
        .unwrap_or(1.0);

    let pan = child(mixer_node, "Pan")
        .and_then(|v| child_f64(v, "Manual"))
        .unwrap_or(0.0);

    let speaker_on = child(mixer_node, "Speaker")
        .and_then(|s| child_bool(s, "Manual"))
        .unwrap_or(true);

    let crossfade_state = child(mixer_node, "CrossFadeState")
        .and_then(|c| child_i32(c, "Manual"))
        .unwrap_or(0);

    let sends = child(mixer_node, "Sends")
        .map(|sends_node| {
            sends_node
                .children()
                .filter(|n| n.has_tag_name("TrackSendHolder"))
                .filter_map(|holder| {
                    let send_node = child(holder, "Send")?;
                    let level = child_f64(send_node, "Manual").unwrap_or(0.0);
                    let enabled = child(holder, "Active")
                        .and_then(|a| child_bool(a, "Manual"))
                        .unwrap_or(true);
                    Some(SendLevel { level, enabled })
                })
                .collect()
        })
        .unwrap_or_default();

    MixerState {
        volume,
        pan,
        sends,
        solo: false,
        speaker_on,
        crossfade_state,
    }
}

//! Write support for Ableton Live set files.
//!
//! Generates .als files (gzipped XML) from `AbletonLiveSet` types.
//! The output targets Ableton Live 12 format and is compatible with Live 11+.
//!
//! # Approach
//!
//! This writer generates complete .als XML from our domain types rather than
//! doing round-trip modification. This means:
//! - You can create .als files from any source (Pro Tools import, REAPER, etc.)
//! - Unknown/unsupported Ableton elements are not preserved
//! - Ableton fills in defaults for any missing elements when it opens the file
//!
//! # Usage
//!
//! ```no_run
//! use dawfile_ableton::write::write_live_set;
//!
//! let set = dawfile_ableton::read_live_set("input.als").unwrap();
//! // ... modify set ...
//! write_live_set(&set, "output.als").unwrap();
//! ```

pub mod xml_writer;

use crate::error::{AbletonError, AbletonResult};
use crate::types::*;
use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::Write;
use std::path::Path;
use xml_writer::AbletonXmlWriter;

/// Write an Ableton Live set to a file (.als).
pub fn write_live_set(set: &AbletonLiveSet, path: impl AsRef<Path>) -> AbletonResult<()> {
    let data = write_live_set_bytes(set)?;
    std::fs::write(path.as_ref(), data)?;
    Ok(())
}

/// Serialize an Ableton Live set to gzipped XML bytes.
pub fn write_live_set_bytes(set: &AbletonLiveSet) -> AbletonResult<Vec<u8>> {
    let xml = serialize_to_xml(set)?;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(xml.as_bytes())
        .map_err(AbletonError::Io)?;
    encoder.finish().map_err(AbletonError::Io)
}

/// Serialize to raw XML string (useful for debugging).
pub fn serialize_to_xml(set: &AbletonLiveSet) -> AbletonResult<String> {
    let mut buf = Vec::new();
    let mut w = AbletonXmlWriter::new(&mut buf);

    w.write_declaration()
        .map_err(|e| AbletonError::Xml(e.to_string()))?;

    write_root(&mut w, set).map_err(|e| AbletonError::Xml(e.to_string()))?;

    drop(w);
    let xml = String::from_utf8(buf).map_err(|e| AbletonError::Xml(e.to_string()))?;
    // Ableton expects a trailing newline
    Ok(xml + "\n")
}

fn write_root(w: &mut AbletonXmlWriter<&mut Vec<u8>>, set: &AbletonLiveSet) -> std::io::Result<()> {
    let minor_version = format!(
        "{}.{}.{}",
        set.version.major, set.version.minor, set.version.patch
    );
    let creator = if set.version.creator.is_empty() {
        format!("Ableton Live {}.{}", set.version.major, set.version.minor)
    } else {
        set.version.creator.clone()
    };

    w.start_with_attrs(
        "Ableton",
        &[
            ("MajorVersion", "5"),
            ("MinorVersion", &minor_version),
            ("SchemaChangeCount", "3"),
            ("Creator", &creator),
            ("Revision", ""),
        ],
    )?;

    w.start("LiveSet")?;
    write_tracks(w, set)?;
    write_master_track(w, set)?;
    write_pre_hear_track(w)?;
    write_locators(w, &set.locators)?;
    write_scenes(w, &set.scenes)?;
    write_transport(w, &set.transport)?;
    w.end("LiveSet")?;

    w.end("Ableton")
}

fn write_tracks(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    set: &AbletonLiveSet,
) -> std::io::Result<()> {
    w.start("Tracks")?;

    for track in &set.audio_tracks {
        write_audio_track(w, track)?;
    }
    for track in &set.midi_tracks {
        write_midi_track(w, track)?;
    }
    for track in &set.group_tracks {
        write_group_track(w, track)?;
    }

    w.end("Tracks")?;

    // Return tracks are a separate element
    w.start("ReturnTracks")?;
    for track in &set.return_tracks {
        write_return_track(w, track)?;
    }
    w.end("ReturnTracks")
}

fn write_track_common(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    common: &TrackCommon,
) -> std::io::Result<()> {
    w.value_int("LomId", 0)?;
    w.value_int("LomIdView", 0)?;

    w.start("Name")?;
    w.value_element("EffectiveName", &common.effective_name)?;
    w.value_element("UserName", &common.user_name)?;
    w.value_element("Annotation", &common.annotation)?;
    w.value_element("MemorizedFirstClipName", "")?;
    w.end("Name")?;

    w.value_int("Color", common.color as i64)?;
    w.value_int("TrackGroupId", common.group_id as i64)?;
    w.value_bool("TrackUnfolded", !common.folded)?;

    Ok(())
}

fn write_device_chain_start(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    mixer: &MixerState,
) -> std::io::Result<()> {
    w.start("DeviceChain")?;
    write_mixer(w, mixer)?;
    Ok(())
}

fn write_mixer(w: &mut AbletonXmlWriter<&mut Vec<u8>>, mixer: &MixerState) -> std::io::Result<()> {
    w.start("Mixer")?;
    w.value_int("LomId", 0)?;
    w.value_int("LomIdView", 0)?;
    w.value_bool("IsExpanded", true)?;

    w.start("On")?;
    w.value_bool("Manual", true)?;
    w.automation_target("AutomationTarget")?;
    w.end("On")?;

    // Volume
    w.start("Volume")?;
    w.value_float("Manual", mixer.volume)?;
    w.automation_target("AutomationTarget")?;
    w.start("MidiControllerRange")?;
    w.value_float("Min", 0.0003162277660168)?;
    w.value_float("Max", 1.99526231496888)?;
    w.end("MidiControllerRange")?;
    w.end("Volume")?;

    // Pan
    w.start("Pan")?;
    w.value_float("Manual", mixer.pan)?;
    w.automation_target("AutomationTarget")?;
    w.start("MidiControllerRange")?;
    w.value_float("Min", -1.0)?;
    w.value_float("Max", 1.0)?;
    w.end("MidiControllerRange")?;
    w.end("Pan")?;

    // Speaker
    w.start("Speaker")?;
    w.value_bool("Manual", mixer.speaker_on)?;
    w.automation_target("AutomationTarget")?;
    w.end("Speaker")?;

    // CrossFadeState
    w.start("CrossFadeState")?;
    w.value_int("Manual", mixer.crossfade_state as i64)?;
    w.automation_target("AutomationTarget")?;
    w.end("CrossFadeState")?;

    // Sends
    w.start("Sends")?;
    for (i, send) in mixer.sends.iter().enumerate() {
        w.start_with_id("TrackSendHolder", i as i32)?;
        w.start("Send")?;
        w.value_float("Manual", send.level)?;
        w.automation_target("AutomationTarget")?;
        w.end("Send")?;
        w.start("Active")?;
        w.value_bool("Manual", send.enabled)?;
        w.end("Active")?;
        w.end("TrackSendHolder")?;
    }
    w.end("Sends")?;

    w.end("Mixer")
}

fn write_routing(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    tag: &str,
    target: &str,
) -> std::io::Result<()> {
    w.start(tag)?;
    w.value_element("Target", target)?;
    w.value_element("UpperDisplayString", "")?;
    w.value_element("LowerDisplayString", "")?;
    w.start("MpeSettings")?;
    w.value_int("ZoneType", 0)?;
    w.value_int("FirstNoteChannel", 1)?;
    w.value_int("LastNoteChannel", 15)?;
    w.end("MpeSettings")?;
    w.end(tag)
}

fn write_audio_track(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    track: &AudioTrack,
) -> std::io::Result<()> {
    w.start_with_id("AudioTrack", track.common.id)?;
    write_track_common(w, &track.common)?;

    write_device_chain_start(w, &track.common.mixer)?;
    write_routing(w, "AudioInputRouting", &track.audio_input)?;
    write_routing(w, "AudioOutputRouting", &track.audio_output)?;

    // MainSequencer
    w.start("MainSequencer")?;
    w.value_int("LomId", 0)?;

    // ClipSlotList (session clips)
    w.start("ClipSlotList")?;
    write_session_audio_clips(w, &track.session_clips)?;
    w.end("ClipSlotList")?;

    w.value_int("MonitoringEnum", track.monitoring as i64)?;

    // Arrangement audio clips
    w.start("Sample")?;
    w.start("ArrangerAutomation")?;
    w.start("Events")?;
    for clip in &track.arrangement_clips {
        write_audio_clip(w, clip)?;
    }
    w.end("Events")?;
    w.end("ArrangerAutomation")?;
    w.end("Sample")?;

    w.end("MainSequencer")?;
    w.end("DeviceChain")?;
    w.end("AudioTrack")
}

fn write_midi_track(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    track: &MidiTrack,
) -> std::io::Result<()> {
    w.start_with_id("MidiTrack", track.common.id)?;
    write_track_common(w, &track.common)?;

    write_device_chain_start(w, &track.common.mixer)?;
    write_routing(w, "MidiInputRouting", &track.midi_input)?;
    write_routing(w, "AudioOutputRouting", &track.audio_output)?;

    // MainSequencer
    w.start("MainSequencer")?;
    w.value_int("LomId", 0)?;

    // ClipSlotList (session clips)
    w.start("ClipSlotList")?;
    write_session_midi_clips(w, &track.session_clips)?;
    w.end("ClipSlotList")?;

    w.value_int("MonitoringEnum", track.monitoring as i64)?;

    // Arrangement MIDI clips
    w.start("ClipTimeable")?;
    w.start("ArrangerAutomation")?;
    w.start("Events")?;
    for clip in &track.arrangement_clips {
        write_midi_clip(w, clip)?;
    }
    w.end("Events")?;
    w.end("ArrangerAutomation")?;
    w.end("ClipTimeable")?;

    w.end("MainSequencer")?;
    w.end("DeviceChain")?;
    w.end("MidiTrack")
}

fn write_group_track(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    track: &GroupTrack,
) -> std::io::Result<()> {
    w.start_with_id("GroupTrack", track.common.id)?;
    write_track_common(w, &track.common)?;
    write_device_chain_start(w, &track.common.mixer)?;
    w.end("DeviceChain")?;
    w.end("GroupTrack")
}

fn write_return_track(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    track: &ReturnTrack,
) -> std::io::Result<()> {
    w.start_with_id("ReturnTrack", track.common.id)?;
    write_track_common(w, &track.common)?;
    write_device_chain_start(w, &track.common.mixer)?;
    w.end("DeviceChain")?;
    w.end("ReturnTrack")
}

// ─── Clips ──────────────────────────────────────────────────────────────────

fn write_clip_common(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    common: &ClipCommon,
) -> std::io::Result<()> {
    w.value_int("LomId", 0)?;
    w.value_int("LomIdView", 0)?;
    w.value_float("CurrentStart", common.current_start)?;
    w.value_float("CurrentEnd", common.current_end)?;

    if let Some(ref loop_s) = common.loop_settings {
        w.start("Loop")?;
        w.value_float("LoopStart", loop_s.loop_start)?;
        w.value_float("LoopEnd", loop_s.loop_end)?;
        w.value_float("StartRelative", loop_s.start_relative)?;
        w.value_bool("LoopOn", loop_s.loop_on)?;
        w.value_float("OutMarker", loop_s.loop_end)?;
        w.value_float("HiddenLoopStart", 0.0)?;
        w.value_float("HiddenLoopEnd", 4.0)?;
        w.end("Loop")?;
    }

    w.value_element("Name", &common.name)?;
    w.value_int("Color", common.color as i64)?;
    w.value_bool("Disabled", common.disabled)?;

    if let Some(ref fa) = common.follow_action {
        w.start("FollowAction")?;
        w.value_float("FollowTime", fa.follow_time)?;
        w.value_bool("IsLinked", fa.is_linked)?;
        w.value_int("LoopIterations", fa.loop_iterations as i64)?;
        w.value_int("FollowActionA", fa.action_a as i64)?;
        w.value_int("FollowActionB", fa.action_b as i64)?;
        w.value_int("FollowChanceA", fa.chance_a as i64)?;
        w.value_int("FollowChanceB", fa.chance_b as i64)?;
        w.value_bool("FollowActionEnabled", fa.enabled)?;
        w.end("FollowAction")?;
    }

    Ok(())
}

fn write_audio_clip(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    clip: &AudioClip,
) -> std::io::Result<()> {
    let time_str = format_float(clip.common.time);
    w.start_with_attrs(
        "AudioClip",
        &[("Id", &clip.common.id.to_string()), ("Time", &time_str)],
    )?;

    write_clip_common(w, &clip.common)?;

    if let Some(ref sr) = clip.sample_ref {
        write_sample_ref(w, sr)?;
    }

    w.value_int("WarpMode", clip.warp_mode as i64)?;
    w.value_bool("IsWarped", clip.is_warped)?;
    w.value_float("PitchCoarse", clip.pitch_coarse)?;
    w.value_float("PitchFine", clip.pitch_fine)?;
    w.value_float("SampleVolume", clip.sample_volume)?;

    // Warp markers
    if !clip.warp_markers.is_empty() {
        w.start("WarpMarkers")?;
        for marker in &clip.warp_markers {
            w.empty_with_attrs(
                "WarpMarker",
                &[
                    ("SecTime", &format_float(marker.sec_time)),
                    ("BeatTime", &format_float(marker.beat_time)),
                ],
            )?;
        }
        w.end("WarpMarkers")?;
    }

    // Fades
    if let Some(ref fades) = clip.fades {
        w.start("Fades")?;
        w.value_float("FadeInLength", fades.fade_in_length)?;
        w.value_float("FadeOutLength", fades.fade_out_length)?;
        w.value_float("FadeInCurveSkew", fades.fade_in_curve_skew)?;
        w.value_float("FadeInCurveSlope", fades.fade_in_curve_slope)?;
        w.value_float("FadeOutCurveSkew", fades.fade_out_curve_skew)?;
        w.value_float("FadeOutCurveSlope", fades.fade_out_curve_slope)?;
        w.end("Fades")?;
    }

    w.end("AudioClip")
}

fn write_midi_clip(w: &mut AbletonXmlWriter<&mut Vec<u8>>, clip: &MidiClip) -> std::io::Result<()> {
    let time_str = format_float(clip.common.time);
    w.start_with_attrs(
        "MidiClip",
        &[("Id", &clip.common.id.to_string()), ("Time", &time_str)],
    )?;

    write_clip_common(w, &clip.common)?;

    // Notes > KeyTracks
    w.start("Notes")?;
    w.start("KeyTracks")?;
    for (i, kt) in clip.key_tracks.iter().enumerate() {
        w.start_with_id("KeyTrack", i as i32)?;
        w.start("Notes")?;
        for note in &kt.notes {
            let note_id = w.next_id().to_string();
            w.empty_with_attrs(
                "MidiNoteEvent",
                &[
                    ("Time", &format_float(note.time)),
                    ("Duration", &format_float(note.duration)),
                    ("Velocity", &note.velocity.to_string()),
                    ("VelocityDeviation", &note.velocity_deviation.to_string()),
                    ("IsEnabled", if note.is_enabled { "true" } else { "false" }),
                    ("Probability", &format_float(note.probability)),
                    ("NoteId", &note_id),
                ],
            )?;
        }
        w.end("Notes")?;
        w.value_int("MidiKey", kt.midi_key as i64)?;
        w.end("KeyTrack")?;
    }
    w.end("KeyTracks")?;
    w.empty("PerNoteEventStore")?;
    w.end("Notes")?;

    // Scale info
    if let Some(ref ks) = clip.scale_info {
        w.start("ScaleInformation")?;
        w.value_int("RootNote", ks.root_note.to_midi() as i64)?;
        w.value_element("Name", &ks.scale)?;
        w.end("ScaleInformation")?;
        w.value_bool("IsInKey", true)?;
    }

    w.end("MidiClip")
}

fn write_sample_ref(w: &mut AbletonXmlWriter<&mut Vec<u8>>, sr: &SampleRef) -> std::io::Result<()> {
    w.start("SampleRef")?;
    w.start("FileRef")?;

    w.value_int("RelativePathType", 1)?;
    w.value_element("RelativePath", sr.relative_path.as_deref().unwrap_or(""))?;
    w.value_element(
        "Path",
        &sr.path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
    )?;
    w.value_int("Type", 1)?;
    w.value_element("LivePackName", sr.live_pack_name.as_deref().unwrap_or(""))?;
    w.value_element("LivePackId", sr.live_pack_id.as_deref().unwrap_or(""))?;
    w.value_int("OriginalFileSize", sr.file_size.unwrap_or(0) as i64)?;
    w.value_int("OriginalCrc", sr.crc.unwrap_or(0) as i64)?;

    w.end("FileRef")?;

    w.value_int("LastModDate", sr.last_mod_date.unwrap_or(0) as i64)?;
    w.empty("SourceContext")?;
    w.value_int("SampleUsageHint", 0)?;
    w.value_int("DefaultDuration", sr.default_duration.unwrap_or(0) as i64)?;
    w.value_int(
        "DefaultSampleRate",
        sr.default_sample_rate.unwrap_or(44100) as i64,
    )?;

    w.end("SampleRef")
}

fn write_session_audio_clips(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    clips: &[SessionClip<AudioClip>],
) -> std::io::Result<()> {
    // Determine how many slots we need
    let max_slot = clips.iter().map(|c| c.slot_index).max().unwrap_or(0);
    let num_slots = if clips.is_empty() { 0 } else { max_slot + 1 };

    for slot_idx in 0..num_slots {
        w.start_with_id("ClipSlot", slot_idx as i32)?;
        if let Some(sc) = clips.iter().find(|c| c.slot_index == slot_idx) {
            w.start("ClipData")?;
            write_audio_clip(w, &sc.clip)?;
            w.end("ClipData")?;
        } else {
            w.start("ClipData")?;
            w.end("ClipData")?;
        }
        w.value_bool("HasStop", true)?;
        w.value_bool("NeedRefreeze", true)?;
        w.end("ClipSlot")?;
    }
    Ok(())
}

fn write_session_midi_clips(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    clips: &[SessionClip<MidiClip>],
) -> std::io::Result<()> {
    let max_slot = clips.iter().map(|c| c.slot_index).max().unwrap_or(0);
    let num_slots = if clips.is_empty() { 0 } else { max_slot + 1 };

    for slot_idx in 0..num_slots {
        w.start_with_id("ClipSlot", slot_idx as i32)?;
        if let Some(sc) = clips.iter().find(|c| c.slot_index == slot_idx) {
            w.start("ClipData")?;
            write_midi_clip(w, &sc.clip)?;
            w.end("ClipData")?;
        } else {
            w.start("ClipData")?;
            w.end("ClipData")?;
        }
        w.value_bool("HasStop", true)?;
        w.value_bool("NeedRefreeze", true)?;
        w.end("ClipSlot")?;
    }
    Ok(())
}

// ─── Master / Transport / Markers ───────────────────────────────────────────

fn write_master_track(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    set: &AbletonLiveSet,
) -> std::io::Result<()> {
    // v12+ uses MainTrack
    let tag = if set.version.at_least(12, 0) {
        "MainTrack"
    } else {
        "MasterTrack"
    };

    w.start(tag)?;
    w.value_int("LomId", 0)?;

    w.start("DeviceChain")?;

    // Mixer with tempo
    w.start("Mixer")?;
    w.value_int("LomId", 0)?;

    // Tempo
    w.start("Tempo")?;
    w.value_float("Manual", set.tempo)?;
    let tempo_target_id = w.automation_target("AutomationTarget")?;

    // Tempo automation (inline ArrangerAutomation)
    w.start("ArrangerAutomation")?;
    w.start("Events")?;
    // Always write the sentinel event
    w.empty_with_attrs(
        "FloatEvent",
        &[
            ("Id", "0"),
            ("Time", "-63072000"),
            ("Value", &format_float(set.tempo)),
        ],
    )?;
    for (i, point) in set.tempo_automation.iter().enumerate() {
        w.empty_with_attrs(
            "FloatEvent",
            &[
                ("Id", &(i + 1).to_string()),
                ("Time", &format_float(point.time)),
                ("Value", &format_float(point.value)),
            ],
        )?;
    }
    w.end("Events")?;
    w.end("ArrangerAutomation")?;
    w.end("Tempo")?;

    // Time signature
    w.start("TimeSignature")?;
    w.start("TimeSignatures")?;
    w.start_with_id("RemoteableTimeSignature", 0)?;
    w.value_int("Numerator", set.time_signature.numerator as i64)?;
    w.value_int("Denominator", set.time_signature.denominator as i64)?;
    w.value_float("Time", 0.0)?;
    w.end("RemoteableTimeSignature")?;
    w.end("TimeSignatures")?;
    w.end("TimeSignature")?;

    // Master mixer volume/pan
    if let Some(ref master) = set.master_track {
        w.start("Volume")?;
        w.value_float("Manual", master.mixer.volume)?;
        w.automation_target("AutomationTarget")?;
        w.end("Volume")?;

        w.start("Pan")?;
        w.value_float("Manual", master.mixer.pan)?;
        w.automation_target("AutomationTarget")?;
        w.end("Pan")?;
    }

    w.end("Mixer")?;

    // Master output routing
    let output = set
        .master_track
        .as_ref()
        .map(|m| m.audio_output.as_str())
        .unwrap_or("AudioOut/External/S0");
    write_routing(w, "AudioOutputRouting", output)?;

    w.end("DeviceChain")?;

    // Tempo automation envelope (v10+ style, cross-reference by pointee ID)
    w.start("AutomationEnvelopes")?;
    w.start("Envelopes")?;
    if !set.tempo_automation.is_empty() {
        w.start_with_id("AutomationEnvelope", 0)?;
        w.start("EnvelopeTarget")?;
        w.value_int("PointeeId", tempo_target_id as i64)?;
        w.end("EnvelopeTarget")?;
        w.start("Automation")?;
        w.start("Events")?;
        for (i, point) in set.tempo_automation.iter().enumerate() {
            w.empty_with_attrs(
                "FloatEvent",
                &[
                    ("Id", &i.to_string()),
                    ("Time", &format_float(point.time)),
                    ("Value", &format_float(point.value)),
                ],
            )?;
        }
        w.end("Events")?;
        w.end("Automation")?;
        w.end("AutomationEnvelope")?;
    }
    w.end("Envelopes")?;
    w.end("AutomationEnvelopes")?;

    w.end(tag)
}

fn write_pre_hear_track(w: &mut AbletonXmlWriter<&mut Vec<u8>>) -> std::io::Result<()> {
    w.start("PreHearTrack")?;
    w.value_int("LomId", 0)?;
    w.start("DeviceChain")?;
    write_routing(w, "AudioOutputRouting", "AudioOut/External/S1")?;
    w.end("DeviceChain")?;
    w.end("PreHearTrack")
}

fn write_locators(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    locators: &[Locator],
) -> std::io::Result<()> {
    w.start("Locators")?;
    w.start("Locators")?;
    for loc in locators {
        w.start("Locator")?;
        w.value_float("Time", loc.time)?;
        w.value_element("Name", &loc.name)?;
        w.end("Locator")?;
    }
    w.end("Locators")?;
    w.end("Locators")
}

fn write_scenes(w: &mut AbletonXmlWriter<&mut Vec<u8>>, scenes: &[Scene]) -> std::io::Result<()> {
    w.start("Scenes")?;
    for scene in scenes {
        w.start_with_id("Scene", scene.id)?;
        w.value_element("Name", &scene.name)?;
        w.value_int("Color", scene.color as i64)?;

        if let Some(tempo) = scene.tempo {
            w.start("Tempo")?;
            w.value_float("Manual", tempo)?;
            w.end("Tempo")?;
            w.value_bool("IsTempoEnabled", true)?;
        } else {
            w.value_bool("IsTempoEnabled", false)?;
        }

        w.end("Scene")?;
    }
    w.end("Scenes")
}

fn write_transport(
    w: &mut AbletonXmlWriter<&mut Vec<u8>>,
    transport: &TransportState,
) -> std::io::Result<()> {
    w.start("Transport")?;
    w.value_bool("LoopOn", transport.loop_on)?;
    w.value_float("LoopStart", transport.loop_start)?;
    w.value_float("LoopLength", transport.loop_length)?;
    w.value_bool("LoopIsSongStart", transport.loop_is_song_start)?;
    w.value_float("CurrentTime", transport.current_time)?;
    w.value_bool("PunchIn", transport.punch_in)?;
    w.value_bool("PunchOut", transport.punch_out)?;
    w.value_int(
        "MetronomeTickDuration",
        transport.metronome_tick_duration as i64,
    )?;
    w.value_int("DrawMode", transport.draw_mode as i64)?;
    w.end("Transport")
}

/// Format a float value the way Ableton expects.
fn format_float(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        let raw = format!("{value}");
        raw.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

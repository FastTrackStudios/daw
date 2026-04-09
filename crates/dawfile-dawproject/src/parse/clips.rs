//! Parse clips and their content (audio, MIDI notes).

use super::xml_helpers::*;
use crate::types::{
    AudioContent, Clip, ClipContent, ClipSlot, LoopSettings, Note, Scene, TimeUnit, Warp,
};
use roxmltree::Node;

/// Parse a `<Clips>` element into a list of clips.
pub fn parse_clips(clips_node: Node<'_, '_>) -> Vec<Clip> {
    children(clips_node, "Clip").map(parse_clip).collect()
}

pub fn parse_clip(node: Node<'_, '_>) -> Clip {
    let id = attr(node, "id").unwrap_or("").to_string();
    let time = attr_f64(node, "time", 0.0);
    let duration = attr_f64(node, "duration", 0.0);
    let time_unit = attr(node, "timeUnit").map(TimeUnit::from_str);
    let content_time_unit = attr(node, "contentTimeUnit").map(TimeUnit::from_str);
    let name = attr(node, "name").map(str::to_string);
    let color = attr(node, "color").map(str::to_string);
    let fade_in = attr(node, "fadeInTime").and_then(|v| v.parse().ok());
    let fade_out = attr(node, "fadeOutTime").and_then(|v| v.parse().ok());

    let loop_settings = parse_loop(node);
    let content = parse_clip_content(node);

    Clip {
        id,
        time,
        duration,
        time_unit,
        content_time_unit,
        name,
        color,
        fade_in,
        fade_out,
        loop_settings,
        content,
    }
}

fn parse_loop(node: Node<'_, '_>) -> Option<LoopSettings> {
    let loop_node = child(node, "Loops")?;
    Some(LoopSettings {
        loop_start: attr_f64(loop_node, "loopStart", 0.0),
        loop_end: attr_f64(loop_node, "loopEnd", 0.0),
        play_start: attr_f64(loop_node, "playStart", 0.0),
    })
}

fn parse_clip_content(node: Node<'_, '_>) -> ClipContent {
    // Check for audio content
    if let Some(audio_node) = child(node, "Audio") {
        return ClipContent::Audio(parse_audio(audio_node));
    }

    // Check for MIDI notes
    if let Some(notes_node) = child(node, "Notes") {
        return ClipContent::Notes(parse_notes(notes_node));
    }

    ClipContent::Empty
}

fn parse_audio(node: Node<'_, '_>) -> AudioContent {
    let path = attr(node, "file").map(str::to_string);
    // "embedded" signals the file lives inside the archive
    let embedded = attr_bool(node, "embedded", false);
    let sample_rate = attr(node, "sampleRate").and_then(|v| v.parse().ok());
    let channels = attr(node, "channels").and_then(|v| v.parse().ok());
    let duration = attr(node, "duration").and_then(|v| v.parse().ok());
    let algorithm = attr(node, "algorithm").map(str::to_string);

    let warps = child(node, "Warps").map(parse_warps).unwrap_or_default();

    AudioContent {
        path,
        embedded,
        sample_rate,
        channels,
        duration,
        algorithm,
        warps,
    }
}

fn parse_warps(node: Node<'_, '_>) -> Vec<Warp> {
    children(node, "Warp")
        .map(|w| Warp {
            time: attr_f64(w, "time", 0.0),
            content_time: attr_f64(w, "contentTime", 0.0),
        })
        .collect()
}

pub fn parse_notes(notes_node: Node<'_, '_>) -> Vec<Note> {
    children(notes_node, "Note")
        .map(|n| Note {
            time: attr_f64(n, "time", 0.0),
            duration: attr_f64(n, "duration", 0.0),
            channel: attr_u8(n, "channel", 0),
            key: attr_u8(n, "key", 60),
            velocity: attr_f64(n, "vel", 1.0),
            release_velocity: attr(n, "rel").and_then(|v| v.parse().ok()),
        })
        .collect()
}

/// Parse scenes from a `<Scenes>` element.
pub fn parse_scenes(scenes_node: Node<'_, '_>) -> Vec<Scene> {
    children(scenes_node, "Scene").map(parse_scene).collect()
}

fn parse_scene(node: Node<'_, '_>) -> Scene {
    let id = attr(node, "id").unwrap_or("").to_string();
    let name = attr(node, "name").map(str::to_string);
    let color = attr(node, "color").map(str::to_string);

    let slots = child(node, "Slots")
        .map(|s| children(s, "ClipSlot").map(parse_clip_slot).collect())
        .unwrap_or_default();

    Scene {
        id,
        name,
        color,
        slots,
    }
}

fn parse_clip_slot(node: Node<'_, '_>) -> ClipSlot {
    let id = attr(node, "id").unwrap_or("").to_string();
    let has_stop = attr_bool(node, "hasStop", false);
    let clip = child(node, "Clip").map(parse_clip);
    ClipSlot { id, has_stop, clip }
}

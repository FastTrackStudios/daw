//! Parse clips and their content (audio, video, MIDI notes).

use super::xml_helpers::*;
use crate::types::{
    AudioContent, Clip, ClipContent, ClipSlot, Fade, FadeCurve, LoopSettings, Marker, Note, Scene,
    TimeUnit, VideoContent, Warp, Warps,
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
    let comment = attr(node, "comment").map(str::to_string);
    let enabled = attr_bool(node, "enable", true);
    let play_start = attr(node, "playStart").and_then(|v| v.parse().ok());
    let play_stop = attr(node, "playStop").and_then(|v| v.parse().ok());
    let reference = attr(node, "reference").map(str::to_string);
    let fade_in = parse_fade(node, "FadeIn", "fadeInTime");
    let fade_out = parse_fade(node, "FadeOut", "fadeOutTime");
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
        comment,
        enabled,
        play_start,
        play_stop,
        reference,
        fade_in,
        fade_out,
        loop_settings,
        content,
    }
}

/// Parse a `<FadeIn>` or `<FadeOut>` child element, falling back to a bare
/// duration attribute (e.g. `fadeInTime`) for compatibility with older exports.
fn parse_fade(node: Node<'_, '_>, child_tag: &str, fallback_attr: &str) -> Option<Fade> {
    if let Some(child_node) = child(node, child_tag) {
        let time = attr_f64(child_node, "time", 0.0);
        let curve = attr(child_node, "curve")
            .map(FadeCurve::from_str)
            .unwrap_or_default();
        return Some(Fade { time, curve });
    }
    // Legacy flat attribute form
    attr(node, fallback_attr)
        .and_then(|v| v.parse::<f64>().ok())
        .map(|time| Fade {
            time,
            curve: FadeCurve::Linear,
        })
}

fn parse_loop(node: Node<'_, '_>) -> Option<LoopSettings> {
    // The spec uses playStart/loopStart/loopEnd as clip-level attributes
    let loop_start = attr(node, "loopStart").and_then(|v| v.parse().ok())?;
    let loop_end = attr(node, "loopEnd").and_then(|v| v.parse().ok())?;
    let play_start = attr(node, "playStart")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    Some(LoopSettings {
        loop_start,
        loop_end,
        play_start,
    })
}

fn parse_clip_content(node: Node<'_, '_>) -> ClipContent {
    if let Some(audio_node) = child(node, "Audio") {
        return ClipContent::Audio(parse_media(audio_node, false));
    }
    if let Some(video_node) = child(node, "Video") {
        return ClipContent::Video(parse_video(video_node));
    }
    if let Some(notes_node) = child(node, "Notes") {
        return ClipContent::Notes(parse_notes(notes_node));
    }
    ClipContent::Empty
}

fn parse_media(node: Node<'_, '_>, _is_video: bool) -> AudioContent {
    let path = attr(node, "file").map(str::to_string);
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

fn parse_video(node: Node<'_, '_>) -> VideoContent {
    let path = attr(node, "file").map(str::to_string);
    let embedded = attr_bool(node, "embedded", false);
    let sample_rate = attr(node, "sampleRate").and_then(|v| v.parse().ok());
    let channels = attr(node, "channels").and_then(|v| v.parse().ok());
    let duration = attr(node, "duration").and_then(|v| v.parse().ok());
    let algorithm = attr(node, "algorithm").map(str::to_string);
    let warps = child(node, "Warps").map(parse_warps).unwrap_or_default();
    VideoContent {
        path,
        embedded,
        sample_rate,
        channels,
        duration,
        algorithm,
        warps,
    }
}

fn parse_warps(node: Node<'_, '_>) -> Warps {
    let content_time_unit = attr(node, "contentTimeUnit").map(TimeUnit::from_str);
    let warps = children(node, "Warp")
        .map(|w| Warp {
            time: attr_f64(w, "time", 0.0),
            content_time: attr_f64(w, "contentTime", 0.0),
        })
        .collect();
    Warps {
        content_time_unit,
        warps,
    }
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
    let comment = attr(node, "comment").map(str::to_string);
    let tempo = attr(node, "tempo").and_then(|v| v.parse().ok());
    let slots = child(node, "Slots")
        .map(|s| children(s, "ClipSlot").map(parse_clip_slot).collect())
        .unwrap_or_default();
    Scene {
        id,
        name,
        color,
        comment,
        tempo,
        slots,
    }
}

fn parse_clip_slot(node: Node<'_, '_>) -> ClipSlot {
    let id = attr(node, "id").unwrap_or("").to_string();
    let has_stop = attr_bool(node, "hasStop", false);
    let time = attr(node, "time").and_then(|v| v.parse().ok());
    let duration = attr(node, "duration").and_then(|v| v.parse().ok());
    let clip = child(node, "Clip").map(parse_clip);
    ClipSlot {
        id,
        has_stop,
        time,
        duration,
        clip,
    }
}

pub fn parse_marker(node: Node<'_, '_>) -> Marker {
    Marker {
        time: attr_f64(node, "time", 0.0),
        name: attr(node, "name").unwrap_or("").to_string(),
        color: attr(node, "color").map(str::to_string),
        comment: attr(node, "comment").map(str::to_string),
    }
}

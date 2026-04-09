//! Parse the track hierarchy from the `<Structure>` element.

use super::{devices, xml_helpers::*};
use crate::types::{Channel, ChannelRole, ContentType, Send, Track};
use roxmltree::Node;

/// Parse all top-level tracks from a `<Structure>` element.
pub fn parse_tracks(structure: Node<'_, '_>) -> Vec<Track> {
    children(structure, "Track").map(parse_track).collect()
}

fn parse_track(node: Node<'_, '_>) -> Track {
    let id = attr(node, "id").unwrap_or("").to_string();
    let name = attr(node, "name").unwrap_or("").to_string();
    let color = attr(node, "color").map(str::to_string);
    let content_type = attr(node, "contentType")
        .map(ContentType::from_str)
        .unwrap_or(ContentType::Unknown);

    let channel = child(node, "Channel").map(parse_channel);

    let children = children(node, "Track").map(parse_track).collect();

    Track {
        id,
        name,
        color,
        content_type,
        channel,
        children,
    }
}

fn parse_channel(node: Node<'_, '_>) -> Channel {
    let id = attr(node, "id").unwrap_or("").to_string();
    let role = attr(node, "role")
        .map(ChannelRole::from_str)
        .unwrap_or(ChannelRole::Regular);
    let audio_channels = attr_u32(node, "audioChannels", 2);

    let volume = child(node, "Volume")
        .map(|v| attr_f64(v, "value", 1.0))
        .unwrap_or(1.0);

    let pan = child(node, "Pan")
        .map(|p| attr_f64(p, "value", 0.0))
        .unwrap_or(0.0);

    let muted = child(node, "Mute")
        .map(|m| attr_bool(m, "value", false))
        .unwrap_or(false);

    let sends = child(node, "Sends").map(parse_sends).unwrap_or_default();

    let devices = child(node, "Devices")
        .map(devices::parse_devices)
        .unwrap_or_default();

    Channel {
        id,
        role,
        audio_channels,
        volume,
        pan,
        muted,
        sends,
        devices,
    }
}

fn parse_sends(sends_node: Node<'_, '_>) -> Vec<Send> {
    children(sends_node, "Send")
        .map(|send| {
            let target = attr(send, "target").unwrap_or("").to_string();
            let volume = attr_f64(send, "volume", 1.0);
            let pre_fader = matches!(attr(send, "type"), Some("pre"));
            Send {
                target,
                volume,
                pre_fader,
            }
        })
        .collect()
}

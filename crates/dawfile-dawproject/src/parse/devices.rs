//! Parse device (plugin) chains from channel elements.

use super::xml_helpers::*;
use crate::types::{Device, DeviceFormat, DeviceState};
use roxmltree::Node;

/// Parse all devices from a `<Devices>` element.
pub fn parse_devices(devices_node: Node<'_, '_>) -> Vec<Device> {
    child_elements(devices_node)
        .filter_map(parse_device)
        .collect()
}

fn parse_device(node: Node<'_, '_>) -> Option<Device> {
    let format = match node.tag_name().name() {
        "Vst2Plugin" => DeviceFormat::Vst2,
        "Vst3Plugin" => DeviceFormat::Vst3,
        "ClapPlugin" => DeviceFormat::Clap,
        "AuPlugin" => DeviceFormat::Au,
        "BuiltinDevice" | "Equalizer" | "Compressor" | "Limiter" | "NoiseGate" => {
            DeviceFormat::Builtin
        }
        // Skip unknown elements (e.g. Sends, automation lanes)
        _ => return None,
    };

    let name = attr(node, "name").unwrap_or("").to_string();
    let enabled = !attr_bool(node, "bypass", false);

    let plugin_path = attr(node, "deviceFile").map(std::path::PathBuf::from);

    // Look for a State child element
    let state = child(node, "State").and_then(|s| {
        if let Some(file) = attr(s, "file") {
            Some(DeviceState::File(file.to_string()))
        } else if let Some(text) = s.text() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                Some(DeviceState::Base64(trimmed.to_string()))
            } else {
                None
            }
        } else {
            None
        }
    });

    Some(Device {
        name,
        format,
        plugin_path,
        enabled,
        state,
    })
}

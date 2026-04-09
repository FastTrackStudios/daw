//! Parse device (plugin) chains from channel elements.

use super::{tracks::parse_device_parameters, xml_helpers::*};
use crate::types::{Device, DeviceFormat, DeviceState};
use roxmltree::Node;

/// Parse all devices from a `<Devices>` element.
pub fn parse_devices(devices_node: Node<'_, '_>) -> Vec<Device> {
    child_elements(devices_node)
        .filter_map(parse_device)
        .collect()
}

fn parse_device(node: Node<'_, '_>) -> Option<Device> {
    let tag = node.tag_name().name();
    let format = match tag {
        "Vst2Plugin" => DeviceFormat::Vst2,
        "Vst3Plugin" => DeviceFormat::Vst3,
        "ClapPlugin" => DeviceFormat::Clap,
        "AuPlugin" => DeviceFormat::Au,
        "BuiltinDevice" => DeviceFormat::Builtin,
        "Equalizer" => DeviceFormat::Equalizer,
        "Compressor" => DeviceFormat::Compressor,
        "Limiter" => DeviceFormat::Limiter,
        "NoiseGate" => DeviceFormat::NoiseGate,
        _ => return None,
    };

    let name = attr(node, "deviceName")
        .or_else(|| attr(node, "name"))
        .unwrap_or("")
        .to_string();
    let enabled = !attr_bool(node, "bypass", false);
    let loaded = attr_bool(node, "loaded", true);
    let device_role = super::tracks::parse_device_role(node);

    // Format-specific plugin identifier
    let plugin_id = attr(node, "pluginId").map(str::to_string);

    let plugin_path = attr(node, "deviceFile").map(std::path::PathBuf::from);

    let parameters = child(node, "Parameters")
        .map(parse_device_parameters)
        .unwrap_or_default();

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
        device_role,
        plugin_id,
        plugin_path,
        enabled,
        loaded,
        parameters,
        state,
    })
}

//! Locator (marker) and scene parsing.
//!
//! Locators live under `LiveSet.Locators.Locators` (yes, double-nested).
//! Each `<Locator>` has `<Time Value="..." />` and `<Name Value="..." />`.
//!
//! Scenes live under `LiveSet.Scenes` as `<Scene>` elements.

use super::xml_helpers::*;
use crate::types::{Locator, Scene, TransportState};
use roxmltree::Node;

/// Parse locators from the `<Locators>` element (the outer one).
pub fn parse_locators(locators_outer: Node<'_, '_>) -> Vec<Locator> {
    // The structure is <Locators><Locators><Locator>...</Locator></Locators></Locators>
    let inner = child(locators_outer, "Locators").unwrap_or(locators_outer);

    inner
        .children()
        .filter(|n| n.has_tag_name("Locator"))
        .filter_map(|loc| {
            let time = child_f64(loc, "Time")?;
            let name = child_value(loc, "Name").unwrap_or("").to_string();
            Some(Locator { time, name })
        })
        .collect()
}

/// Parse scenes from the `<Scenes>` element.
pub fn parse_scenes(scenes_node: Node<'_, '_>) -> Vec<Scene> {
    scenes_node
        .children()
        .filter(|n| n.has_tag_name("Scene"))
        .map(|scene| {
            let id = id_attr(scene);
            let name = child_value(scene, "Name").unwrap_or("").to_string();
            let color = child_i32(scene, "Color").unwrap_or(0);

            // Scene tempo override: <IsTempoEnabled Value="true"/>
            // IsTempoEnabled is a direct child with a Value attribute.
            let tempo_enabled = child_bool(scene, "IsTempoEnabled").unwrap_or(false);
            let tempo = if tempo_enabled {
                child(scene, "Tempo").and_then(|t| child_f64(t, "Manual"))
            } else {
                None
            };

            Scene {
                id,
                name,
                color,
                tempo,
            }
        })
        .collect()
}

/// Parse transport state from the `<Transport>` element.
pub fn parse_transport(transport_node: Node<'_, '_>) -> TransportState {
    TransportState {
        loop_on: child(transport_node, "LoopOn")
            .and_then(|n| {
                child_bool(n, "Value").or_else(|| n.attribute("Value").map(|v| v == "true"))
            })
            .unwrap_or(false),
        loop_start: child(transport_node, "LoopStart")
            .and_then(|n| {
                child_f64(n, "Value").or_else(|| n.attribute("Value").and_then(|v| v.parse().ok()))
            })
            .unwrap_or(0.0),
        loop_length: child(transport_node, "LoopLength")
            .and_then(|n| {
                child_f64(n, "Value").or_else(|| n.attribute("Value").and_then(|v| v.parse().ok()))
            })
            .unwrap_or(16.0),
        loop_is_song_start: child(transport_node, "LoopIsSongStart")
            .and_then(|n| {
                child_bool(n, "Value").or_else(|| n.attribute("Value").map(|v| v == "true"))
            })
            .unwrap_or(false),
        current_time: child(transport_node, "CurrentTime")
            .and_then(|n| {
                child_f64(n, "Value").or_else(|| n.attribute("Value").and_then(|v| v.parse().ok()))
            })
            .unwrap_or(0.0),
        punch_in: child(transport_node, "PunchIn")
            .and_then(|n| {
                child_bool(n, "Value").or_else(|| n.attribute("Value").map(|v| v == "true"))
            })
            .unwrap_or(false),
        punch_out: child(transport_node, "PunchOut")
            .and_then(|n| {
                child_bool(n, "Value").or_else(|| n.attribute("Value").map(|v| v == "true"))
            })
            .unwrap_or(false),
        metronome_tick_duration: child(transport_node, "MetronomeTickDuration")
            .and_then(|n| {
                child_i32(n, "Value").or_else(|| n.attribute("Value").and_then(|v| v.parse().ok()))
            })
            .unwrap_or(0),
        draw_mode: child(transport_node, "DrawMode")
            .and_then(|n| {
                child_i32(n, "Value").or_else(|| n.attribute("Value").and_then(|v| v.parse().ok()))
            })
            .unwrap_or(0),
    }
}

//! Parse the arrangement timeline.

use super::{clips, xml_helpers::*};
use crate::types::{
    Arrangement, AutomationLane, AutomationPoint, Interpolation, Lane, LaneContent, Marker,
    TimeUnit,
};
use roxmltree::Node;

/// Parse an `<Arrangement>` element.
pub fn parse_arrangement(node: Node<'_, '_>) -> Arrangement {
    let id = attr(node, "id").unwrap_or("").to_string();
    let time_unit = attr(node, "timeUnit")
        .map(TimeUnit::from_str)
        .unwrap_or(TimeUnit::Beats);

    let lanes = child(node, "Lanes")
        .map(|lanes_node| parse_lanes(lanes_node, time_unit))
        .unwrap_or_default();

    Arrangement {
        id,
        time_unit,
        lanes,
    }
}

fn parse_lanes(node: Node<'_, '_>, parent_time_unit: TimeUnit) -> Vec<Lane> {
    child_elements(node)
        .filter_map(|child| parse_lane(child, parent_time_unit))
        .collect()
}

fn parse_lane(node: Node<'_, '_>, parent_time_unit: TimeUnit) -> Option<Lane> {
    let tag = node.tag_name().name();
    let id = attr(node, "id").unwrap_or("").to_string();
    let track = attr(node, "track").unwrap_or("").to_string();
    let time_unit = attr(node, "timeUnit").map(TimeUnit::from_str);

    let effective_unit = time_unit.unwrap_or(parent_time_unit);

    let content = match tag {
        "Lanes" => {
            // Nested Lanes element — recurse and collect sub-lanes as clips lane
            // (track-level Lanes contain Clips or AutomationLane elements)
            let sub_lanes = parse_lanes(node, effective_unit);
            // Flatten: return each sub-lane individually by collecting them
            // We represent this as nested lanes stored in a Clips variant
            // using the first Clips sub-lane we find
            if let Some(first) = sub_lanes.into_iter().next() {
                return Some(Lane {
                    id,
                    track,
                    time_unit,
                    content: first.content,
                });
            }
            return None;
        }
        "Clips" => {
            let clip_list = clips::parse_clips(node);
            LaneContent::Clips(clip_list)
        }
        "Notes" => {
            let notes = clips::parse_notes(node);
            LaneContent::Notes(notes)
        }
        "AutomationLane" => {
            let automation = parse_automation_lane(node);
            LaneContent::Automation(automation)
        }
        "Markers" => {
            let markers = parse_markers(node);
            LaneContent::Markers(markers)
        }
        _ => return None,
    };

    Some(Lane {
        id,
        track,
        time_unit,
        content,
    })
}

fn parse_automation_lane(node: Node<'_, '_>) -> AutomationLane {
    let id = attr(node, "id").unwrap_or("").to_string();
    let target = attr(node, "target").unwrap_or("").to_string();

    let points = child(node, "AutomationPoints")
        .map(parse_automation_points)
        .unwrap_or_default();

    AutomationLane { id, target, points }
}

fn parse_automation_points(node: Node<'_, '_>) -> Vec<AutomationPoint> {
    // Points can be RealPoint, BoolPoint, IntegerPoint — all share time/value.
    child_elements(node)
        .filter_map(|p| {
            let time = attr_f64(p, "time", 0.0);
            let value: f64 = attr(p, "value")?.parse().ok()?;
            let interpolation = attr(p, "interpolation")
                .map(Interpolation::from_str)
                .unwrap_or(Interpolation::Hold);
            Some(AutomationPoint {
                time,
                value,
                interpolation,
            })
        })
        .collect()
}

fn parse_markers(node: Node<'_, '_>) -> Vec<Marker> {
    children(node, "Marker")
        .map(|m| Marker {
            time: attr_f64(m, "time", 0.0),
            name: attr(m, "name").unwrap_or("").to_string(),
            color: attr(m, "color").map(str::to_string),
        })
        .collect()
}

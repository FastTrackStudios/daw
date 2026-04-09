//! Per-track and per-clip automation parsing.
//!
//! Track automation lives in `AutomationEnvelopes.Envelopes`:
//! ```xml
//! <AutomationEnvelopes>
//!   <Envelopes>
//!     <AutomationEnvelope Id="0">
//!       <EnvelopeTarget>
//!         <PointeeId Value="42" />
//!       </EnvelopeTarget>
//!       <Automation>
//!         <Events>
//!           <FloatEvent Id="0" Time="0" Value="1.0" />
//!           <BoolEvent Id="1" Time="4" Value="true" />
//!           <EnumEvent Id="2" Time="8" Value="3" />
//!         </Events>
//!       </Automation>
//!     </AutomationEnvelope>
//!   </Envelopes>
//! </AutomationEnvelopes>
//! ```
//!
//! Clip envelopes live in `Envelopes.Envelopes` inside each clip.

use super::xml_helpers::*;
use crate::types::{AutomationEnvelope, AutomationEvent, ClipEnvelope};
use roxmltree::Node;

/// Parse all automation envelopes from a track's `AutomationEnvelopes` node.
pub fn parse_track_automation(track_node: Node<'_, '_>) -> Vec<AutomationEnvelope> {
    let envelopes_container = match descend(track_node, "AutomationEnvelopes.Envelopes") {
        Some(e) => e,
        None => return Vec::new(),
    };

    envelopes_container
        .children()
        .filter(|n| n.has_tag_name("AutomationEnvelope"))
        .filter_map(|envelope| {
            let pointee_id =
                descend(envelope, "EnvelopeTarget").and_then(|et| child_i32(et, "PointeeId"))?;

            let events = descend(envelope, "Automation.Events")
                .map(|events_node| parse_automation_events(events_node))
                .unwrap_or_default();

            if events.is_empty() {
                return None;
            }

            Some(AutomationEnvelope { pointee_id, events })
        })
        .collect()
}

/// Parse clip automation envelopes from a clip node.
pub fn parse_clip_envelopes(clip_node: Node<'_, '_>) -> Vec<ClipEnvelope> {
    // Structure: <Envelopes><Envelopes><ClipEnvelope>...</ClipEnvelope></Envelopes></Envelopes>
    let outer = match child(clip_node, "Envelopes") {
        Some(e) => e,
        None => return Vec::new(),
    };
    let inner = child(outer, "Envelopes").unwrap_or(outer);

    inner
        .children()
        .filter(|n| n.has_tag_name("ClipEnvelope"))
        .filter_map(|envelope| {
            let pointee_id =
                descend(envelope, "EnvelopeTarget").and_then(|et| child_i32(et, "PointeeId"))?;

            let events = descend(envelope, "Automation.Events")
                .map(|events_node| parse_automation_events(events_node))
                .unwrap_or_default();

            Some(ClipEnvelope { pointee_id, events })
        })
        .collect()
}

/// Parse automation events from an `Events` node.
fn parse_automation_events(events_node: Node<'_, '_>) -> Vec<AutomationEvent> {
    let mut events = Vec::new();

    for event in events_node.children() {
        if !event.is_element() {
            continue;
        }

        let time = match event.attribute("Time").and_then(|v| v.parse::<f64>().ok()) {
            Some(t) => t,
            None => continue,
        };

        match event.tag_name().name() {
            "FloatEvent" => {
                if let Some(value) = event.attribute("Value").and_then(|v| v.parse::<f64>().ok()) {
                    events.push(AutomationEvent::Float { time, value });
                }
            }
            "BoolEvent" => {
                if let Some(value) = event.attribute("Value").map(|v| v == "true") {
                    events.push(AutomationEvent::Bool { time, value });
                }
            }
            "EnumEvent" => {
                if let Some(value) = event.attribute("Value").and_then(|v| v.parse::<i32>().ok()) {
                    events.push(AutomationEvent::Enum { time, value });
                }
            }
            _ => {}
        }
    }

    events
}

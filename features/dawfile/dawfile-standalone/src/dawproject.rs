//! DAWproject as an **interop** target.
//!
//! Native is ours (`<name>.daw`); DAWproject is how a session reaches
//! Bitwig, Cubase or Studio One. It was seriously considered as the
//! native format and rejected on one fact: a `.dawproject` is a ZIP,
//! which is the worst possible dedup target — compressed, monolithic,
//! and wholly rewritten on any change, in direct conflict with the
//! object store that makes autosave cheap.
//!
//! As interop it is nearly free, because `dawfile-dawproject` already
//! reads *and* writes and its modelled set lines up with ours.
//!
//! ## What crosses, and what does not
//!
//! Transport, the track hierarchy and mixer state cross both ways.
//!
//! **Editor state does not, and cannot.** DAWproject has no home for it,
//! so a round trip out to Cubase and back silently drops mode
//! corrections, lane layout and the generated envelopes. That is
//! accepted rather than solved (#157), and it is the reason the native
//! format exists at all.
//!
//! **FX chains do not.** They are opaque hash-named blobs on our side
//! and typed devices on theirs; carrying bytes across would produce a
//! file that claims plugin state it cannot honour.
//!
//! **Item volume automation does cross, but flattened.** Scenario 1 of
//! #149 generates four source envelopes — gating, compression,
//! sibilance, de-breathing — composited into the item's one volume
//! envelope. DAWproject has one gain expression per lane and no home
//! for the four, exactly as REAPER's item chunk has one `VOLENV`. So
//! the *composite* crosses and the sources stay in the `.daw`.
//!
//! That is a real limit, and the important part is that it is not
//! silent: a mix handed to Cubase keeps its volume ride, and the four
//! it was built from are simply not there to edit.

use crate::document::{DawDocument, MarkerNode, TrackNode};
use crate::error::DawResult;
use crate::id::EntityId;
use dawfile_dawproject::types as dp;
use daw_proto::automation::EnvelopeType;

/// Convert a `.daw` document into a DAWproject.
pub fn to_dawproject(document: &DawDocument) -> dp::DawProject {
    let first = document.tempo_map.first();
    let transport = dp::Transport {
        tempo: first.map(|t| t.bpm).unwrap_or(120.0),
        numerator: first
            .and_then(|t| t.time_signature.as_ref())
            .map(|ts| ts.numerator as u8)
            .unwrap_or(4),
        denominator: first
            .and_then(|t| t.time_signature.as_ref())
            .map(|ts| ts.denominator as u8)
            .unwrap_or(4),
    };

    let tracks = document
        .tracks
        .iter()
        .map(|node| {
            let mut channel = dp::Channel::default();
            channel.id = format!("ch-{}", node.id);
            channel.volume = node.track.volume;
            channel.pan = node.track.pan;
            channel.muted = node.track.muted;
            channel.solo = node.track.soloed;

            dp::Track {
                id: node.id.to_string(),
                name: node.track.name.clone(),
                // Ours is a packed RGB integer; theirs is a hex string.
                color: node.track.color.map(|c| {
                    format!("#{:02X}{:02X}{:02X}", (c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF)
                }),
                comment: None,
                content_types: Vec::new(),
                loaded: true,
                channel: Some(channel),
                children: Vec::new(),
            }
        })
        .collect();

    dp::DawProject {
        version: "1.0".into(),
        application: None,
        metadata: None,
        transport,
        tracks,
        arrangement: automation_arrangement(document),
        scenes: Vec::new(),
    }
}

/// The name the composite volume envelope goes by on an item.
///
/// REAPER's own chunk name, kept because that is what a round trip
/// through `.rpp` produces — matching on it here means one rule covers
/// documents that came from either side.
const COMPOSITE: &str = "VOLENV";

/// An arrangement carrying each item's composite volume automation.
///
/// `None` when there is nothing to say, rather than an empty
/// arrangement: a file claiming an arrangement with no lanes reads as
/// "this project is empty", which is a different statement from "this
/// project's contents did not cross".
fn automation_arrangement(document: &DawDocument) -> Option<dp::Arrangement> {
    let mut lanes = Vec::new();

    for node in &document.tracks {
        // One lane per *track*, not per item. A track's items are
        // disjoint in time, and their composites concatenate into the
        // one gain curve DAWproject models — two lanes with the same
        // track IDREF and the same Gain expression would be asking the
        // reader to choose.
        let mut points: Vec<dp::AutomationPoint> = Vec::new();

        for item in &node.items {
            let Some(env) = composite_of(item) else {
                continue;
            };
            // Item envelope times are relative to the item, which is
            // how both `.rpp` and our own format store them. The
            // arrangement's are absolute, so they have to be offset —
            // without this every item's ride is stacked at the top of
            // the timeline.
            let origin = item.item.position.as_seconds();
            points.extend(env.points.iter().map(|p| dp::AutomationPoint {
                time: origin + p.time.as_seconds(),
                value: p.value,
                interpolation: dp::Interpolation::Linear,
            }));
        }

        if points.is_empty() {
            continue;
        }
        points.sort_by(|a, b| a.time.total_cmp(&b.time));

        lanes.push(dp::Lane {
            id: format!("auto-{}", node.id),
            track: node.id.to_string(),
            time_unit: Some(dp::TimeUnit::Seconds),
            content: dp::LaneContent::Automation(dp::AutomationPoints {
                id: format!("points-{}", node.id),
                target: dp::AutomationTarget {
                    // Gain, not a device parameter: this is the item's
                    // own volume, which belongs to no plugin.
                    expression: Some(dp::ExpressionType::Gain),
                    ..Default::default()
                },
                unit: Some(dp::AutomationUnit::Linear),
                points,
            }),
        });
    }

    (!lanes.is_empty()).then(|| dp::Arrangement {
        id: "arrangement".into(),
        name: None,
        color: None,
        comment: None,
        time_unit: dp::TimeUnit::Seconds,
        lanes,
        markers: Vec::new(),
        tempo_automation: Vec::new(),
        time_sig_automation: Vec::new(),
    })
}

/// The one envelope on an item that describes how it should sound.
///
/// The composite by name, or — for a document that never went through
/// REAPER — the only volume envelope there is. `None` when several
/// volume envelopes exist and none is named as the composite: there is
/// no way to know which describes the item, and exporting a guess hands
/// the other DAW a mix that is quietly wrong.
fn composite_of(item: &crate::document::ItemNode) -> Option<&crate::document::EnvelopeNode> {
    if let Some(named) = item.envelopes.iter().find(|e| e.envelope.name == COMPOSITE) {
        return (!named.points.is_empty()).then_some(named);
    }
    let mut vols = item
        .envelopes
        .iter()
        .filter(|e| e.envelope.envelope_type == EnvelopeType::Volume);
    let first = vols.next()?;
    if vols.next().is_some() || first.points.is_empty() {
        return None;
    }
    Some(first)
}

/// Convert a DAWproject into a `.daw` document.
///
/// Entities get fresh ids: DAWproject's ids are XML cross-reference
/// strings scoped to one file, not stable identities we may adopt as our
/// own. Adopting them would make two imports of the same file collide.
pub fn from_dawproject(project: &dp::DawProject, name: impl Into<String>) -> DawResult<DawDocument> {
    let mut document = DawDocument::new(name);

    document.tempo_map = vec![daw_proto::tempo_map::TempoPoint {
        position: Default::default(),
        bpm: project.transport.tempo,
        time_signature: Some(daw_proto::TimeSignature {
            numerator: project.transport.numerator as u32,
            denominator: project.transport.denominator as u32,
        }),
        shape: None,
        bezier_tension: None,
        selected: None,
        linear: None,
    }];

    for track in &project.tracks {
        let id = EntityId::new();
        let mut t = daw_proto::track::Track::default();
        t.guid = id.to_string();
        t.name = track.name.clone();
        t.color = track.color.as_deref().and_then(parse_hex_rgb);
        if let Some(ch) = &track.channel {
            t.volume = ch.volume;
            t.pan = ch.pan;
            t.muted = ch.muted;
            t.soloed = ch.solo;
        }
        document.tracks.push(TrackNode {
            id,
            track: t,
            parent: None,
            envelopes: Vec::new(),
            items: Vec::new(),
            fx_chain: None,
            input_fx_chain: None,
        });
    }

    Ok(document)
}

/// `#RRGGBB` to a packed RGB integer. `None` for anything else, rather
/// than a guessed colour.
fn parse_hex_rgb(s: &str) -> Option<u32> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

/// Markers do not have a DAWproject home in this mapping yet; kept as a
/// named gap rather than silently dropped inside a conversion.
pub fn markers_not_carried(document: &DawDocument) -> &[MarkerNode] {
    &document.markers
}

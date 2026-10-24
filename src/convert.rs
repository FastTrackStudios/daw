//! Conversion functions from dawfile-reaper parsed types to daw-proto domain types.
//!
//! Maps parsed RPP structures into the workspace's shared domain model:
//! - [`FxChain`] → [`daw_proto::fx::FxTree`]
//! - [`FxPlugin`] → [`daw_proto::fx::Fx`]
//! - [`FxChainNode`] → [`daw_proto::fx::FxNode`]
//! - [`FxContainer`] → [`daw_proto::fx::FxNode`] (Container kind)
//! - [`PluginType`] → [`daw_proto::fx::FxType`]

use daw_proto::fx::{
    Fx, FxType,
    tree::{FxContainerChannelConfig, FxNode, FxNodeId, FxNodeKind, FxRoutingMode, FxTree},
};
use daw_proto::track::Track as DawTrack;
use std::collections::HashSet;

use crate::types::fx_chain::{FxChain, FxChainNode, FxContainer, FxPlugin, PluginType};
use crate::types::track::{FolderState, Track as ReaperTrack};

// ---------------------------------------------------------------------------
// FxChain → FxTree
// ---------------------------------------------------------------------------

/// Convert a parsed [`FxChain`] into a [`daw_proto::fx::FxTree`].
///
/// Each top-level node is assigned an index-based `FxNodeId`. Plugins use their
/// FXID GUID when available, containers use a synthetic `container:N` path.
pub fn fx_chain_to_tree(chain: &FxChain) -> FxTree {
    let nodes = chain
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| convert_node(node, i, None, &[]))
        .collect();

    FxTree { nodes }
}

/// Convert a daw-proto [`FxTree`] back to a REAPER [`FxChain`].
///
/// This enables round-trip editing flows:
/// `FXCHAIN text -> FxChain -> FxTree -> FxChain -> FXCHAIN text`.
pub fn tree_to_fx_chain(tree: &FxTree) -> FxChain {
    FxChain {
        window_rect: None,
        show: 0,
        last_sel: 0,
        docked: false,
        nodes: tree
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| convert_tree_node(node, i, FxRoutingMode::Serial))
            .collect(),
        raw_content: String::new(),
    }
}

// ---------------------------------------------------------------------------
// FxChainNode → FxNode
// ---------------------------------------------------------------------------

fn convert_node(
    node: &FxChainNode,
    index: usize,
    parent_id: Option<&FxNodeId>,
    path_prefix: &[usize],
) -> FxNode {
    match node {
        FxChainNode::Plugin(plugin) => convert_plugin(plugin, index, parent_id),
        FxChainNode::Container(container) => {
            convert_container(container, index, parent_id, path_prefix)
        }
    }
}

fn convert_plugin(plugin: &FxPlugin, index: usize, parent_id: Option<&FxNodeId>) -> FxNode {
    let id = plugin
        .fxid
        .as_ref()
        .map(|guid| FxNodeId::from_guid(guid.clone()))
        .unwrap_or_else(|| FxNodeId::from_guid(format!("index:{}", index)));

    let fx = Fx {
        guid: plugin.fxid.clone().unwrap_or_default(),
        index: index as u32,
        name: plugin.name.clone(),
        plugin_name: extract_plugin_name(&plugin.name),
        plugin_type: convert_plugin_type(&plugin.plugin_type),
        enabled: !plugin.bypassed,
        offline: plugin.offline,
        window_open: false, // Not available from chunk parsing
        parameter_count: 0, // Not available from chunk parsing without state decode
        preset_name: plugin.preset_name.clone(),
    };

    FxNode {
        id,
        kind: FxNodeKind::Plugin(fx),
        enabled: !plugin.bypassed,
        parent_id: parent_id.cloned(),
    }
}

fn convert_container(
    container: &FxContainer,
    index: usize,
    parent_id: Option<&FxNodeId>,
    path_prefix: &[usize],
) -> FxNode {
    // Build path for container ID: "container:0", "container:0:1", etc.
    let mut path_parts: Vec<String> = path_prefix.iter().map(|p| p.to_string()).collect();
    path_parts.push(index.to_string());
    let path = path_parts.join(":");

    let id = container
        .fxid
        .as_ref()
        .map(|guid| FxNodeId::from_guid(guid.clone()))
        .unwrap_or_else(|| FxNodeId::container(&path));

    // Parse channel config from CONTAINER_CFG if available
    // Format: [type, nch, nch_in, nch_out] — but actual REAPER uses different semantics
    let channel_config = container
        .container_cfg
        .map(|cfg| FxContainerChannelConfig {
            nch: cfg[1] as u32,
            nch_in: cfg[2] as u32,
            nch_out: cfg[3] as u32,
        })
        .unwrap_or_default();

    // Determine routing mode from container children
    // If any child has parallel=true, the container has mixed routing.
    // The container's routing mode in REAPER is per-child, not per-container,
    // but for the tree model we use Serial as the default container routing.
    let routing = FxRoutingMode::Serial;

    // Convert children recursively
    let child_path: Vec<usize> = path_prefix
        .iter()
        .copied()
        .chain(std::iter::once(index))
        .collect();
    let children: Vec<FxNode> = container
        .children
        .iter()
        .enumerate()
        .map(|(i, child)| convert_node(child, i, Some(&id), &child_path))
        .collect();

    FxNode {
        id,
        kind: FxNodeKind::Container {
            name: container.name.clone(),
            children,
            routing,
            channel_config,
        },
        enabled: !container.bypassed,
        parent_id: parent_id.cloned(),
    }
}

fn convert_tree_node(node: &FxNode, index: usize, routing: FxRoutingMode) -> FxChainNode {
    match &node.kind {
        FxNodeKind::Plugin(fx) => FxChainNode::Plugin(FxPlugin {
            name: if fx.name.is_empty() {
                fx.plugin_name.clone()
            } else {
                fx.name.clone()
            },
            custom_name: None,
            plugin_type: convert_fx_type(fx.plugin_type),
            file: String::new(),
            bypassed: !node.enabled,
            offline: fx.offline,
            fxid: if node.id.is_container() {
                None
            } else {
                Some(node.id.as_str().to_string())
            },
            preset_name: fx.preset_name.clone(),
            float_pos: None,
            wak: None,
            parallel: routing == FxRoutingMode::Parallel && index > 0,
            state_data: Vec::new(),
            raw_block: String::new(),
            param_envelopes: Vec::new(),
            params_on_tcp: Vec::new(),
            header_extra: None,
        }),
        FxNodeKind::Container {
            name,
            children,
            routing: child_routing,
            channel_config,
        } => FxChainNode::Container(FxContainer {
            name: name.clone(),
            bypassed: !node.enabled,
            offline: false,
            fxid: if node.id.is_container() {
                None
            } else {
                Some(node.id.as_str().to_string())
            },
            float_pos: None,
            parallel: routing == FxRoutingMode::Parallel && index > 0,
            container_cfg: Some([
                2,
                channel_config.nch as i32,
                channel_config.nch_in as i32,
                channel_config.nch_out as i32,
            ]),
            show: 0,
            last_sel: 0,
            docked: false,
            children: children
                .iter()
                .enumerate()
                .map(|(i, child)| convert_tree_node(child, i, *child_routing))
                .collect(),
            raw_block: String::new(),
        }),
    }
}

// ---------------------------------------------------------------------------
// PluginType → FxType
// ---------------------------------------------------------------------------

/// Convert dawfile [`PluginType`] to daw-proto [`FxType`].
pub fn convert_plugin_type(pt: &PluginType) -> FxType {
    match pt {
        PluginType::Vst => FxType::Vst2,
        PluginType::Vst3 => FxType::Vst3,
        PluginType::Au => FxType::Au,
        PluginType::Js => FxType::Js,
        PluginType::Clap => FxType::Clap,
        PluginType::Video | PluginType::Other(_) => FxType::Unknown,
    }
}

/// Convert daw-proto [`FxType`] to dawfile [`PluginType`].
pub fn convert_fx_type(pt: FxType) -> PluginType {
    match pt {
        FxType::Vst2 => PluginType::Vst,
        FxType::Vst3 => PluginType::Vst3,
        FxType::Au => PluginType::Au,
        FxType::Js => PluginType::Js,
        FxType::Clap => PluginType::Clap,
        FxType::Unknown => PluginType::Other("UNKNOWN".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Reaper Track ↔ daw-proto Track
// ---------------------------------------------------------------------------

/// Convert a parsed REAPER track model to daw-proto [`DawTrack`].
pub fn rpp_track_to_daw_track(track: &ReaperTrack, index: u32) -> DawTrack {
    let guid = track
        .track_id
        .clone()
        .unwrap_or_else(|| format!("track-{index}"));
    let mut out = DawTrack::new(guid, index, track.name.clone());

    out.color = track
        .peak_color
        .and_then(|c| if c < 0 { None } else { Some(c as u32) });

    if let Some(vp) = &track.volpan {
        out.volume = vp.volume;
        out.pan = vp.pan;
    }

    if let Some(ms) = &track.mutesolo {
        out.muted = ms.mute;
        out.soloed = !matches!(ms.solo, crate::types::track::TrackSoloState::NoSolo);
    }

    out.armed = track.record.as_ref().map(|r| r.armed).unwrap_or(false);
    out.folder_depth = track.folder.as_ref().map(|f| f.indentation).unwrap_or(0);
    out.is_folder = matches!(
        track.folder.as_ref().map(|f| f.folder_state),
        Some(FolderState::FolderParent)
    ) || out.folder_depth > 0;

    out.fx_count = track
        .fx_chain
        .as_ref()
        .map(|fx| fx.nodes.len() as u32)
        .unwrap_or(0);
    out.input_fx_count = track
        .input_fx
        .as_ref()
        .map(|fx| fx.nodes.len() as u32)
        .unwrap_or(0);

    out.selected = track.selected;
    out
}

/// Convert an ordered list of parsed REAPER tracks to daw-proto tracks while
/// reconstructing folder parent relationships.
pub fn rpp_tracks_to_daw_tracks(tracks: &[ReaperTrack]) -> Vec<DawTrack> {
    let mut out = Vec::with_capacity(tracks.len());
    let mut folder_stack: Vec<String> = Vec::new();
    let mut seen_guids: HashSet<String> = HashSet::with_capacity(tracks.len());

    for (index, track) in tracks.iter().enumerate() {
        let mut dt = rpp_track_to_daw_track(track, index as u32);
        // Robust GUID handling: repair missing/empty/duplicate IDs.
        if dt.guid.trim().is_empty() || seen_guids.contains(&dt.guid) {
            let base = format!("track-{index}");
            let mut candidate = base.clone();
            let mut n = 2usize;
            while seen_guids.contains(&candidate) {
                candidate = format!("{base}-{n}");
                n += 1;
            }
            dt.guid = candidate;
        }
        seen_guids.insert(dt.guid.clone());

        dt.parent_guid = folder_stack.last().cloned();

        // Update folder stack based on current track's folder state/depth.
        let mut pops_remaining = 0i32;
        if let Some(folder) = &track.folder {
            // Tolerate inconsistent state values by inferring parent from positive indentation.
            if matches!(folder.folder_state, FolderState::FolderParent) || folder.indentation > 0 {
                folder_stack.push(dt.guid.clone());
            }
            if folder.indentation < 0 {
                pops_remaining = -folder.indentation;
            } else if matches!(folder.folder_state, FolderState::LastInFolder) {
                pops_remaining = 1;
            }
        }
        while pops_remaining > 0 && !folder_stack.is_empty() {
            folder_stack.pop();
            pops_remaining -= 1;
        }

        out.push(dt);
    }
    out
}

/// Serialize a daw-proto [`DawTrack`] into a minimal REAPER `<TRACK ...>` chunk.
pub fn daw_track_to_rpp_track_chunk(track: &DawTrack) -> String {
    fn b(v: bool) -> i32 {
        if v { 1 } else { 0 }
    }
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }

    let folder_state = if track.is_folder { 1 } else { 0 };
    let solo = if track.soloed { 1 } else { 0 };
    let peakcol = track.color.unwrap_or(0) as i64;
    let fx_enabled = if track.fx_count > 0 || track.input_fx_count > 0 {
        1
    } else {
        0
    };

    format!(
        "<TRACK\n  NAME \"{name}\"\n  SEL {selected}\n  PEAKCOL {peakcol}\n  VOLPAN {vol:.17} {pan:.17} -1 -1 1\n  MUTESOLO {mute} {solo} 0\n  ISBUS {folder_state} {indent}\n  REC {armed} 0 0 0 0 0 0 0\n  NCHAN 2\n  FX {fx_enabled}\n  TRACKID \"{guid}\"\n>",
        name = esc(&track.name),
        selected = b(track.selected),
        vol = track.volume,
        pan = track.pan,
        mute = b(track.muted),
        solo = solo,
        indent = track.folder_depth,
        armed = b(track.armed),
        guid = esc(&track.guid),
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the clean plugin name from the full display name.
///
/// REAPER display names include the type prefix and vendor:
/// - `"VST: ReaEQ (Cockos)"` → `"ReaEQ"`
/// - `"VSTi: Surge XT (Surge Synth Team)"` → `"Surge XT"`
/// - `"JS: loser/3BandEQ"` → `"3BandEQ"`
/// - `"AU: AUBandpass (Apple)"` → `"AUBandpass"`
fn extract_plugin_name(display_name: &str) -> String {
    // Strip the type prefix (e.g., "VST: ", "VSTi: ", "JS: ", "AU: ", "CLAP: ")
    let name = if let Some(after_colon) = display_name.split_once(": ") {
        after_colon.1.trim()
    } else {
        display_name.trim()
    };

    // Strip the vendor suffix in parentheses (e.g., " (Cockos)")
    if let Some(paren_start) = name.rfind(" (") {
        let before_paren = &name[..paren_start];
        if name.ends_with(')') {
            return before_paren.to_string();
        }
    }

    // For JS plugins, take just the filename part
    if let Some(slash_pos) = name.rfind('/') {
        return name[slash_pos + 1..].to_string();
    }

    name.to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

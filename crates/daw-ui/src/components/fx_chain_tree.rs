//! FX Chain Tree — hierarchical view of FX containers and plugins.
//!
//! Displays the FX chain as a tree with collapsible containers,
//! routing mode indicators, and enable/bypass toggles.

use crate::prelude::*;
use daw_control::{FxNode, FxNodeKind, FxRoutingMode, FxTree};
use std::collections::HashSet;

/// FX Chain Tree panel that polls the DAW for FX tree state.
///
/// Uses the same poll-wait pattern as MixerPanel and TrackControlPanel.
#[component]
pub fn FxChainTree() -> Element {
    let mut tree = use_signal(FxTree::new);
    let mut error_msg = use_signal(|| Option::<String>::None);
    let mut connected = use_signal(|| false);
    let collapsed = use_signal(HashSet::<String>::new);
    let selected = use_signal(|| Option::<String>::None);
    let mut track_guid = use_signal(|| Option::<String>::None);

    // Poll for FX tree
    use_future(move || async move {
        // Poll-wait for DAW
        loop {
            if daw_control::Daw::try_get().is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        let daw = daw_control::Daw::get();
        connected.set(true);

        // Periodic refresh loop
        loop {
            match daw.current_project().await {
                Ok(project) => {
                    // Get first track with FX (or selected track)
                    let tguid = track_guid.read().clone();
                    let target_guid = if let Some(guid) = tguid {
                        Some(guid)
                    } else {
                        // Find first track with FX
                        match project.tracks().all().await {
                            Ok(tracks) => {
                                let mut found = None;
                                for t in &tracks {
                                    let chain = project.tracks().by_guid(&t.guid).await;
                                    if let Ok(Some(th)) = chain {
                                        let fx_chain = th.fx_chain();
                                        if let Ok(count) = fx_chain.count().await {
                                            if count > 0 {
                                                found = Some(t.guid.clone());
                                                break;
                                            }
                                        }
                                    }
                                }
                                found
                            }
                            Err(_) => None,
                        }
                    };

                    if let Some(guid) = target_guid {
                        track_guid.set(Some(guid.clone()));
                        match project.tracks().by_guid(&guid).await {
                            Ok(Some(track_handle)) => {
                                let chain = track_handle.fx_chain();
                                match chain.tree().await {
                                    Ok(fx_tree) => {
                                        tree.set(fx_tree);
                                        error_msg.set(None);
                                    }
                                    Err(e) => {
                                        error_msg.set(Some(format!("FX tree error: {:?}", e)));
                                    }
                                }
                            }
                            Ok(None) => {
                                track_guid.set(None); // Track gone, re-scan next time
                            }
                            Err(e) => {
                                error_msg.set(Some(format!("Track error: {:?}", e)));
                            }
                        }
                    }
                }
                Err(e) => {
                    error_msg.set(Some(format!("Project error: {:?}", e)));
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    });

    let is_connected = *connected.read();
    let fx_tree = tree.read();
    let err = error_msg.read();

    if !is_connected {
        return rsx! {
            div { class: "h-full w-full flex items-center justify-center bg-card text-muted-foreground text-sm",
                "Waiting for DAW connection..."
            }
        };
    }

    if let Some(msg) = err.as_ref() {
        return rsx! {
            div { class: "h-full w-full flex items-center justify-center bg-card text-red-400 text-sm p-4",
                "{msg}"
            }
        };
    }

    let total = fx_tree.total_count();

    rsx! {
        div { class: "h-full w-full flex flex-col bg-card overflow-hidden",
            // Header
            div { class: "px-3 py-2 border-b border-border flex items-center justify-between",
                h2 { class: "text-sm font-semibold text-foreground", "FX Chain" }
                span { class: "text-[10px] text-muted-foreground", "{total} FX" }
            }

            // Tree content
            div { class: "flex-1 overflow-y-auto px-1 py-1",
                if fx_tree.nodes.is_empty() {
                    div { class: "text-center text-muted-foreground text-xs py-8",
                        "No FX in chain"
                    }
                } else {
                    for node in fx_tree.nodes.iter() {
                        FxTreeNode {
                            key: "{node.id.as_str()}",
                            node: node.clone(),
                            depth: 0,
                            collapsed: collapsed.clone(),
                            selected: selected.clone(),
                        }
                    }
                }
            }
        }
    }
}

// ── Tree Node ───────────────────────────────────────────────────────

#[derive(Props, Clone)]
struct FxTreeNodeProps {
    node: FxNode,
    depth: u32,
    collapsed: Signal<HashSet<String>>,
    selected: Signal<Option<String>>,
}

impl PartialEq for FxTreeNodeProps {
    fn eq(&self, other: &Self) -> bool {
        // Compare by node ID and depth — signals are always equal (shared state)
        self.node.id == other.node.id
            && self.depth == other.depth
            && self.node.enabled == other.node.enabled
    }
}

#[component]
fn FxTreeNode(props: FxTreeNodeProps) -> Element {
    let node = &props.node;
    let depth = props.depth;
    let mut collapsed = props.collapsed;
    let mut selected = props.selected;
    let node_id = node.id.as_str().to_string();
    let is_selected = selected.read().as_deref() == Some(node_id.as_str());

    let indent_px = depth * 16;

    match &node.kind {
        FxNodeKind::Container {
            name,
            children,
            routing,
            ..
        } => {
            let is_collapsed = collapsed.read().contains(&node_id);
            let child_count = children.len();
            let routing_label = match routing {
                FxRoutingMode::Serial => "S",
                FxRoutingMode::Parallel => "P",
            };
            let routing_color = match routing {
                FxRoutingMode::Serial => "text-blue-400",
                FxRoutingMode::Parallel => "text-amber-400",
            };
            let enabled_class = if node.enabled {
                "text-foreground"
            } else {
                "text-muted-foreground line-through"
            };
            let selected_bg = if is_selected {
                "bg-accent"
            } else {
                "hover:bg-accent/50"
            };
            let chevron = if is_collapsed { ">" } else { "v" };

            let toggle_id = node_id.clone();
            let select_id = node_id.clone();

            rsx! {
                // Container row
                div {
                    class: "flex items-center gap-1 px-1 py-0.5 rounded cursor-pointer text-xs {selected_bg}",
                    style: "padding-left: {indent_px}px",
                    onclick: move |_| {
                        selected.set(Some(select_id.clone()));
                    },

                    // Collapse toggle
                    button {
                        class: "w-4 h-4 flex items-center justify-center text-muted-foreground hover:text-foreground text-[10px] font-mono",
                        onclick: move |evt| {
                            evt.stop_propagation();
                            let mut set = collapsed.write();
                            if set.contains(&toggle_id) {
                                set.remove(&toggle_id);
                            } else {
                                set.insert(toggle_id.clone());
                            }
                        },
                        "{chevron}"
                    }

                    // Container icon
                    span { class: "text-[10px] {routing_color} font-bold w-4 text-center",
                        "{routing_label}"
                    }

                    // Name
                    span { class: "flex-1 truncate {enabled_class}", "{name}" }

                    // Child count
                    span { class: "text-[10px] text-muted-foreground", "{child_count}" }

                    // Enable indicator
                    span {
                        class: if node.enabled { "w-2 h-2 rounded-full bg-green-500" } else { "w-2 h-2 rounded-full bg-neutral-600" },
                    }
                }

                // Children (if not collapsed)
                if !is_collapsed {
                    for child in children.iter() {
                        FxTreeNode {
                            key: "{child.id.as_str()}",
                            node: child.clone(),
                            depth: depth + 1,
                            collapsed: collapsed.clone(),
                            selected: selected.clone(),
                        }
                    }
                }
            }
        }

        FxNodeKind::Plugin(fx) => {
            let enabled_class = if node.enabled {
                "text-foreground"
            } else {
                "text-muted-foreground line-through"
            };
            let selected_bg = if is_selected {
                "bg-accent"
            } else {
                "hover:bg-accent/50"
            };
            let select_id = node_id.clone();
            let preset = fx.preset_name.as_deref().unwrap_or("");

            rsx! {
                div {
                    class: "flex items-center gap-1 px-1 py-0.5 rounded cursor-pointer text-xs {selected_bg}",
                    style: "padding-left: {indent_px}px",
                    onclick: move |_| {
                        selected.set(Some(select_id.clone()));
                    },

                    // Spacer (no collapse toggle for plugins)
                    span { class: "w-4" }

                    // Plugin type badge
                    span { class: "text-[9px] text-muted-foreground font-mono w-4 text-center",
                        match fx.plugin_type {
                            daw_proto::FxType::Vst3 => "V3",
                            daw_proto::FxType::Vst2 => "V2",
                            daw_proto::FxType::Au => "AU",
                            daw_proto::FxType::Js => "JS",
                            daw_proto::FxType::Clap => "CL",
                            _ => "FX",
                        }
                    }

                    // Name
                    span { class: "flex-1 truncate {enabled_class}", "{fx.name}" }

                    // Preset
                    if !preset.is_empty() {
                        span { class: "text-[9px] text-muted-foreground truncate max-w-[80px]",
                            "{preset}"
                        }
                    }

                    // Enable indicator
                    span {
                        class: if node.enabled { "w-2 h-2 rounded-full bg-green-500" } else { "w-2 h-2 rounded-full bg-neutral-600" },
                    }
                }
            }
        }
    }
}

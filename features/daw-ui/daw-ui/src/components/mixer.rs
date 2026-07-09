//! Mixer Panel — DAW mixer with channel strips resembling REAPER's mixer.
//!
//! Each channel strip shows (top to bottom):
//! - FX container blocks (colored rectangles with container names)
//! - Mute / Solo / FX buttons
//! - Volume fader
//! - dB readout + pan label
//! - Record arm / monitoring buttons
//! - Track name + number

use crate::prelude::*;
use daw_control::{FxNodeKind, FxTree};
use daw_proto::Track;

/// Per-track FX data fetched alongside the track list.
#[derive(Clone, Debug, Default)]
struct TrackFxData {
    tree: FxTree,
}

/// Mixer panel that polls the DAW for track state.
#[component]
pub fn MixerPanel() -> Element {
    let mut tracks = use_signal(Vec::<Track>::new);
    let mut fx_data = use_signal(Vec::<(String, TrackFxData)>::new);
    let mut error_msg = use_signal(|| Option::<String>::None);
    let mut connected = use_signal(|| false);

    // Poll for tracks + FX trees
    use_future(move || async move {
        loop {
            if daw_control::Daw::try_get().is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        let daw = daw_control::Daw::get();
        connected.set(true);

        loop {
            match daw.current_project().await {
                Ok(project) => match project.tracks().all().await {
                    Ok(track_list) => {
                        // Fetch FX tree for each track (for the FX block display)
                        let mut fx_entries = Vec::new();
                        for t in &track_list {
                            let tree = if t.fx_count > 0 {
                                match project.tracks().by_guid(&t.guid).await {
                                    Ok(Some(th)) => th.fx_chain().tree().await.unwrap_or_default(),
                                    _ => FxTree::default(),
                                }
                            } else {
                                FxTree::default()
                            };
                            fx_entries.push((t.guid.clone(), TrackFxData { tree }));
                        }
                        tracks.set(track_list);
                        fx_data.set(fx_entries);
                        error_msg.set(None);
                    }
                    Err(e) => {
                        error_msg.set(Some(format!("Failed to fetch tracks: {:?}", e)));
                    }
                },
                Err(e) => {
                    error_msg.set(Some(format!("Failed to get project: {:?}", e)));
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    });

    let is_connected = *connected.read();

    if !is_connected {
        return rsx! {
            div { class: "h-full w-full flex items-center justify-center bg-card text-muted-foreground text-sm",
                "Waiting for DAW connection..."
            }
        };
    }

    {
        let err = error_msg.read();
        if let Some(msg) = err.as_ref() {
            let msg = msg.clone();
            return rsx! {
                div { class: "h-full w-full flex items-center justify-center bg-card text-red-400 text-sm p-4",
                    "{msg}"
                }
            };
        }
    }

    let track_list = tracks.read().clone();
    let fx_list = fx_data.read().clone();

    rsx! {
        div { class: "h-full w-full flex flex-col bg-zinc-900 overflow-hidden",
            // Header
            div { class: "px-3 py-1.5 border-b border-zinc-700 flex items-center justify-between flex-shrink-0",
                h2 { class: "text-xs font-semibold text-zinc-300", "Mixer" }
                span { class: "text-[10px] text-zinc-500", "{track_list.len()} tracks" }
            }

            // Channel strips — horizontal scroll
            div { class: "flex-1 overflow-x-auto overflow-y-hidden",
                div { class: "flex h-full",
                    for (i, track) in track_list.iter().enumerate() {
                        {
                            let fx = fx_list.iter()
                                .find(|(g, _)| g == &track.guid)
                                .map(|(_, d)| d.tree.clone())
                                .unwrap_or_default();
                            rsx! {
                                ChannelStrip {
                                    key: "{track.guid}",
                                    track: track.clone(),
                                    fx_tree: fx,
                                    index: i as u32,
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Channel Strip ───────────────────────────────────────────────────

#[derive(Props, Clone)]
struct ChannelStripProps {
    track: Track,
    fx_tree: FxTree,
    index: u32,
}

impl PartialEq for ChannelStripProps {
    fn eq(&self, other: &Self) -> bool {
        self.track == other.track && self.index == other.index
    }
}

#[component]
fn ChannelStrip(props: ChannelStripProps) -> Element {
    let track = &props.track;
    let fx_tree = &props.fx_tree;

    let color_css = track
        .color
        .map(|c| format!("#{:06x}", c & 0xFFFFFF))
        .unwrap_or_else(|| "#6b7280".to_string());

    let vol_db = if track.volume > 0.0 {
        20.0 * track.volume.log10()
    } else {
        -100.0
    };
    // Map volume to fader percentage (sqrt scale for better visual)
    let vol_pct = (track.volume.sqrt() * 100.0).min(100.0);

    let pan_label = if track.pan.abs() < 0.01 {
        "C".to_string()
    } else if track.pan < 0.0 {
        format!("{:.0}L", track.pan.abs() * 100.0)
    } else {
        format!("{:.0}R", track.pan * 100.0)
    };

    let db_label = if vol_db > -100.0 {
        format!("{:.1}", vol_db)
    } else {
        "-inf".to_string()
    };

    let selected_border = if track.selected {
        "border-blue-500"
    } else {
        "border-zinc-700"
    };

    rsx! {
        div {
            class: "flex flex-col h-full w-[72px] flex-shrink-0 border-r {selected_border} bg-zinc-900",

            // ── FX Block Strip (top) ────────────────────────────
            // Shows colored rectangles for each top-level FX/container
            div { class: "flex flex-col gap-px px-0.5 py-1 flex-shrink-0 min-h-[40px] max-h-[120px] overflow-y-auto",
                for node in fx_tree.nodes.iter() {
                    {
                        let (name, block_color) = match &node.kind {
                            FxNodeKind::Container { name, .. } => {
                                // Containers get the track color
                                (name.as_str().to_string(), color_css.clone())
                            }
                            FxNodeKind::Plugin(fx) => {
                                // Plugins get a dimmer color
                                (fx.name.clone(), "#3f3f46".to_string())
                            }
                        };
                        let is_enabled = node.enabled;
                        let opacity = if is_enabled { "1.0" } else { "0.4" };
                        rsx! {
                            div {
                                class: "w-full rounded-sm px-1 py-px text-center truncate",
                                style: "background-color: {block_color}; opacity: {opacity}; font-size: 8px; color: white; line-height: 1.4;",
                                title: "{name}",
                                "{name}"
                            }
                        }
                    }
                }
            }

            // ── Separator ───────────────────────────────────────
            div { class: "border-t border-zinc-700 mx-1" }

            // ── M / S / FX buttons ──────────────────────────────
            div { class: "flex items-center justify-center gap-0.5 px-0.5 py-1 flex-shrink-0",
                // Mute
                span {
                    class: if track.muted {
                        "w-5 h-4 flex items-center justify-center rounded text-[8px] font-bold bg-red-600 text-white"
                    } else {
                        "w-5 h-4 flex items-center justify-center rounded text-[8px] font-bold bg-zinc-700 text-zinc-400"
                    },
                    "M"
                }
                // Solo
                span {
                    class: if track.soloed {
                        "w-5 h-4 flex items-center justify-center rounded text-[8px] font-bold bg-yellow-500 text-black"
                    } else {
                        "w-5 h-4 flex items-center justify-center rounded text-[8px] font-bold bg-zinc-700 text-zinc-400"
                    },
                    "S"
                }
                // FX indicator
                if track.fx_count > 0 {
                    span {
                        class: "w-5 h-4 flex items-center justify-center rounded text-[8px] font-bold bg-green-700 text-green-200",
                        "FX"
                    }
                }
            }

            // ── Volume Fader ────────────────────────────────────
            div { class: "flex-1 flex flex-col items-center px-2 py-1 min-h-0",
                div { class: "w-2 flex-1 bg-zinc-800 rounded-sm overflow-hidden flex flex-col-reverse relative",
                    // Green fill
                    div {
                        class: "w-full bg-green-500 transition-all duration-100",
                        style: "height: {vol_pct}%;",
                    }
                }
            }

            // ── dB + Pan readout ────────────────────────────────
            div { class: "text-center px-1 flex-shrink-0",
                div { class: "text-[9px] font-mono text-zinc-400", "{db_label}" }
                div { class: "text-[8px] text-zinc-500", "{pan_label}" }
            }

            // ── Record arm / monitoring ─────────────────────────
            div { class: "flex items-center justify-center gap-1 py-1 flex-shrink-0",
                // Record arm
                span {
                    class: if track.armed {
                        "w-4 h-4 rounded-full bg-red-500 border border-red-400"
                    } else {
                        "w-4 h-4 rounded-full bg-zinc-700 border border-zinc-600"
                    },
                }
                // Monitoring indicator (small circle)
                span {
                    class: "w-4 h-4 rounded-full bg-zinc-700 border border-zinc-600",
                }
            }

            // ── Track Name + Number (bottom) ────────────────────
            div {
                class: "px-1 py-1.5 text-center flex-shrink-0 border-t border-zinc-700",
                style: "background-color: color-mix(in srgb, {color_css} 20%, transparent);",
                div { class: "text-[10px] font-medium text-zinc-200 truncate leading-tight",
                    "{track.name}"
                }
                div { class: "text-[8px] text-zinc-500",
                    "{track.index}"
                }
            }
        }
    }
}

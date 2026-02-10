//! Arrangement View — timeline area showing track lanes.
//!
//! Currently a structural placeholder: shows track lanes with color tints
//! that align with the Track Control Panel. Will eventually render regions,
//! items, and a time ruler.

use crate::prelude::*;
use daw_proto::Track;

/// Arrangement view panel that shows track lanes aligned with the TCP.
///
/// Fetches the same track list as the TCP and renders horizontal lanes
/// so the two panels visually correspond. The lanes are placeholder bars
/// for now — region/item rendering will be added later.
#[component]
pub fn ArrangementView() -> Element {
    let mut tracks = use_signal(Vec::<Track>::new);
    let mut error_msg = use_signal(|| Option::<String>::None);
    let mut connected = use_signal(|| false);

    use_future(move || async move {
        tracing::info!("Arrangement: waiting for DAW connection...");

        loop {
            if daw_control::Daw::try_get().is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        let daw = daw_control::Daw::get();
        connected.set(true);
        tracing::info!("Arrangement: DAW connected, fetching tracks...");

        loop {
            match daw.current_project().await {
                Ok(project) => match project.tracks().all().await {
                    Ok(track_list) => {
                        tracks.set(track_list);
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
    let track_list = tracks.read();
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

    rsx! {
        div { class: "h-full w-full flex flex-col bg-zinc-950 overflow-hidden",
            // Time ruler header (placeholder)
            div { class: "h-7 border-b border-border flex items-center px-3 flex-shrink-0 bg-card",
                span { class: "text-[10px] font-semibold text-muted-foreground uppercase tracking-wider", "Arrangement" }
                div { class: "flex-1" }
                span { class: "text-[10px] text-muted-foreground italic", "Timeline coming soon" }
            }

            // Track lanes — vertical scroll, each lane is a horizontal bar
            div { class: "flex-1 overflow-y-auto overflow-x-hidden",
                for track in track_list.iter() {
                    TrackLane {
                        key: "{track.guid}",
                        track: track.clone(),
                    }
                }

                // Empty state when no tracks
                if track_list.is_empty() {
                    div { class: "flex items-center justify-center h-32 text-muted-foreground text-xs",
                        "No tracks in project"
                    }
                }
            }
        }
    }
}

// ── Track Lane ──────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
struct TrackLaneProps {
    track: Track,
}

#[component]
fn TrackLane(props: TrackLaneProps) -> Element {
    let track = &props.track;

    // Subtle color tint for the lane background
    let (r, g, b) = track
        .color
        .map(|c| ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF))
        .unwrap_or((100, 100, 100));

    let lane_bg = format!("rgba({r}, {g}, {b}, 0.06)");
    let border_color = format!("rgba({r}, {g}, {b}, 0.15)");

    // Row height matches TCP row height (~24px with py-0.5 + content)
    rsx! {
        div {
            class: "flex items-center border-b border-border/30 min-h-[23px]",
            style: "background: {lane_bg};",

            // Lane content area — empty for now, will hold regions/items
            div {
                class: "flex-1 h-full relative",

                // Subtle left accent line matching track color
                div {
                    class: "absolute left-0 top-0 bottom-0 w-px",
                    style: "background: {border_color};",
                }

                // Folder tracks get a label
                if track.is_folder {
                    div { class: "absolute inset-0 flex items-center px-2",
                        span { class: "text-[9px] text-zinc-600 font-medium uppercase tracking-wider",
                            "{track.name}"
                        }
                    }
                }
            }
        }
    }
}

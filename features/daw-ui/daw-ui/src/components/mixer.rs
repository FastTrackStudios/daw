//! Mixer Panel — DAW mixer with channel strips resembling REAPER's mixer.
//!
//! Each channel strip shows (top to bottom):
//! - FX container blocks (colored rectangles with container names)
//! - Mute / Solo / FX buttons
//! - Volume fader
//! - dB readout + pan label
//! - Record arm / monitoring buttons
//! - Track name + number

use crate::controls::{
    Collapse, ControlSync, FxButton, IoButton, MeterFeed, MonitorButton, MuteButton, PanAnchor,
    PanKnob, PhaseButton, RecordArmButton, RecordInputLabel, SoloButton, TrackMeter, TrackName,
    VolumeFader, VolumeWidget, use_daw_tracks, use_track_store,
};
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

    // The vector controls read their track from here, and it is fed by the
    // track-event subscription rather than by this panel's two-second poll:
    // a mute performed in REAPER shows up as soon as the event lands.
    use_daw_tracks(use_track_store());

    // Poll for tracks + FX trees
    use_future(move || async move {
        loop {
            if daw_control::Daw::try_get().is_some() {
                break;
            }
            // futures-timer, not tokio: this panel runs inside REAPER on
            // dioxus's own scheduler, where there is no tokio runtime and a
            // tokio timer is a non-unwinding panic that takes the host down
            // with it.
            futures_timer::Delay::new(std::time::Duration::from_millis(500)).await;
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

            futures_timer::Delay::new(std::time::Duration::from_secs(2)).await;
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
            // The single place a drag becomes an engine write.
            ControlSync {}
            // One meter subscription for the whole mixer. Mounted here
            // rather than per strip: the frame carries every track, and the
            // engine's pump is gated on there being a subscriber at all.
            MeterFeed {}

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

/// One strip, for a caller that already has its track.
///
/// [`MixerPanel`] polls the DAW for a track list and owns the subscriptions;
/// a test, a playground or a design review has the tracks in hand and wants
/// only the strip. Public so the assembled strip can be exercised — and
/// photographed — without a backend behind it.
#[component]
pub fn ChannelStripPreview(
    track: Track,
    #[props(default)] index: u32,
    /// The strip's height in px. Drives the collapse — see
    /// [`Collapse`][crate::controls::Collapse].
    #[props(default = 371.0)]
    height: f32,
) -> Element {
    rsx! {
        ChannelStrip { track, fx_tree: FxTree::default(), index, height }
    }
}

// ── Channel Strip ───────────────────────────────────────────────────

#[derive(Props, Clone)]
struct ChannelStripProps {
    track: Track,
    fx_tree: FxTree,
    index: u32,
    /// The height the strip is drawn at, which is what it collapses by.
    ///
    /// REAPER's own default MCP height; a panel that knows its box passes
    /// the real one.
    #[props(default = 371.0)]
    height: f32,
}

impl PartialEq for ChannelStripProps {
    fn eq(&self, other: &Self) -> bool {
        self.track == other.track && self.index == other.index && self.height == other.height
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

    // Resolved once, so the markup asks a question rather than doing the
    // arithmetic in six places.
    let shape = Collapse::at(props.height);
    let pad = shape.padding;

    let selected_border = if track.selected {
        "border-blue-500"
    } else {
        "border-zinc-700"
    };

    rsx! {
        div {
            class: "flex flex-col h-full w-[72px] flex-shrink-0 border-r {selected_border} bg-zinc-900",

            // REAPER's MCP stacks its strip in a fixed order, and a mixer
            // that reorders them stops being readable by anyone who knows
            // the host: FX at the top, then pan, then the input controls,
            // then the fader and meter taking whatever height is left, then
            // a fixed block at the bottom carrying identity.
            //
            // Layout is Tailwind because layout lives *outside* the `<svg>`
            // elements, where every target runs a real CSS engine — the
            // browser on web, Stylo and Taffy in every desktop, plugin and
            // REAPER window. Inside an `<svg>` there is no box model and no
            // custom properties, which is why the art states its own values
            // and takes none from here.

            // ── FX ──────────────────────────────────────────────
            div { class: "flex flex-col gap-px px-0.5 py-1 flex-shrink-0 min-h-[40px] max-h-[120px] overflow-y-auto",
                for node in fx_tree.nodes.iter() {
                    {
                        let (name, block_color) = match &node.kind {
                            FxNodeKind::Container { name, .. } => {
                                (name.as_str().to_string(), color_css.clone())
                            }
                            FxNodeKind::Plugin(fx) => {
                                (fx.name.clone(), "#3f3f46".to_string())
                            }
                        };
                        let opacity = if node.enabled { "1.0" } else { "0.4" };
                        rsx! {
                            div {
                                class: "w-full rounded-sm px-1 py-px text-center truncate",
                                // Explicit, not a class: a block that loses
                                // its colour and line-height when the sheet
                                // is late reads as a broken chain rather
                                // than an unstyled one.
                                style: "background-color: {block_color}; opacity: {opacity}; font-size: 8px; color: white; line-height: 1.4;",
                                title: "{name}",
                                "{name}"
                            }
                        }
                    }
                }
            }
            div { class: "flex items-center justify-center gap-0.5 px-0.5 pb-1 flex-shrink-0",
                MuteButton { track: track.guid.clone() }
                SoloButton { track: track.guid.clone() }
                FxButton { track: track.guid.clone() }
            }

            div { class: "border-t border-zinc-700 mx-1" }

            // ── Pan ─────────────────────────────────────────────
            //
            // A real box, not implied space: the gap below it is one of the
            // things REAPER's own thresholds are measured against, and a
            // gap that is not a box cannot be queried or tested.
            if shape.pan == PanAnchor::PanSection {
                div {
                    class: "flex flex-col items-center justify-center flex-shrink-0",
                    style: "padding-top:{pad}px; padding-bottom:{pad}px;",
                    PanKnob { track: track.guid.clone() }
                    if shape.show_pan_labels {
                        div { class: "text-[8px] text-zinc-500 leading-none", "pan" }
                    }
                }
            }

            // ── Input ───────────────────────────────────────────
            div {
                class: "flex items-center justify-center gap-1 flex-shrink-0",
                style: "padding-top:{pad}px; padding-bottom:{pad}px;",
                RecordArmButton { track: track.guid.clone() }
                // Record mode gives up its place when the pan control
                // re-anchors here — the two share one slot, which is what
                // makes this a re-anchor rather than a reposition.
                if shape.show_record_mode {
                    MonitorButton { track: track.guid.clone() }
                } else {
                    PanKnob { track: track.guid.clone() }
                }
                // Residual-driven: these key off the stretch section, not
                // the strip's own height.
                if shape.show_phase {
                    PhaseButton { track: track.guid.clone() }
                }
                if shape.show_io {
                    IoButton { track: track.guid.clone() }
                }
            }
            if shape.show_record_input {
                RecordInputLabel { track: track.guid.clone() }
            }

            // ── Fader and meter ─────────────────────────────────
            //
            // `flex-1` and `min-h-0`: this is the region that absorbs the
            // strip's height, which is what gives the fader's stretch band
            // something to stretch into.
            // The stretch section, as a real box — it is the residual the
            // IO, envelope and phase thresholds are measured against, so it
            // has to exist in the tree to be one.
            div {
                class: "flex-1 flex flex-row items-stretch justify-center gap-1 px-2 min-h-0",
                style: "padding-top:{pad}px; padding-bottom:{pad}px;",
                match shape.volume {
                    // Below the swap threshold a fader has no travel worth
                    // having, so it stops being a fader. A Rust
                    // conditional, because a widget type is not a style.
                    VolumeWidget::Fader => rsx! { VolumeFader { track: track.guid.clone() } },
                    VolumeWidget::Knob => rsx! { PanKnob { track: track.guid.clone(), large: true } },
                }
                TrackMeter { track: track.guid.clone(), height: shape.stretch.max(24.0) as u32 }
            }
            if shape.show_volume_label {
                div { class: "text-center px-1 flex-shrink-0",
                    div { class: "text-[9px] font-mono text-zinc-400", "{db_label}" }
                }
            }

            // ── Track Name + Number (bottom) ────────────────────
            div {
                class: "flex-shrink-0 border-t border-zinc-700",
                // The name and its colour come from the store, so a rename
                // or a recolour in the DAW lands here without this panel
                // refetching anything.
                TrackName { track: track.guid.clone() }
                div { class: "text-[8px] text-zinc-500 text-center",
                    "{track.index}"
                }
            }
        }
    }
}

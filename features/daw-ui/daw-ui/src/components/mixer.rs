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
pub fn MixerPanel(
    /// The height the strips are drawn at.
    ///
    /// One number, not two: the strip is drawn at this height *and*
    /// collapses by it. `h-full` plus a separate collapse prop let the two
    /// disagree silently — the strip resolved its bands for one height and
    /// was stretched to another, so every measured offset landed in the
    /// wrong band.
    #[props(default = 371.0)]
    height: f32,
) -> Element {
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
                                    height,
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

/// The strip's own geometry, from `rtconfig.txt` at scale 1 in wide mode.
///
/// `mcp_w` is 86 and the sections are a function of height — which is the
/// thing to know about this panel, and what [`Collapse`] resolves.
const STRIP_W: f32 = 86.0;
const FX_SECTION: f32 = 33.0;
/// Where the pill sits inside the FX section — measured, not nominal.
const FX_PILL_TOP: f32 = 9.0;
const BOTTOM_SECTION: f32 = 47.0;
/// The axis the right-hand column centres on: `mcp.recmon` and everything
/// anchored to it. The record arm's ring sits at 0.486 of its own 36-wide
/// cell rather than in the middle, which is why it is placed separately.
const COLUMN: f32 = 55.0;
/// `mcp.recmon`, `mcp.mute` and `mcp.solo` are all 21 wide. Stated because
/// the column has to *be* that wide: left to shrink-wrap, `align-items:
/// center` centres every button on the widest child — the IO button, which
/// `rtconfig` writes as `mcp.solo + [-1 23 23 30]`, deliberately one column
/// left and two wider — and the whole stack drifted a pixel right of the
/// record arm above it.
const BUTTON_W: f32 = 21.0;
/// The one vertical axis the arm and the column share.
const COLUMN_AXIS: f32 = COLUMN + BUTTON_W / 2.0;
/// `mcp_recarm_*` is a 36-wide cell whose ring is centred at 0.486 of it,
/// not at 18 — so the cell's left edge is not the axis minus half its width.
const ARM_CELL_W: f32 = 36.0;
const ARM_LEFT: f32 = COLUMN_AXIS - ARM_CELL_W * 0.486;

/// The chain `rtconfig` writes down the right-hand column, each step a
/// `[dx dy w h]` offset from the control above it.
const RECMON_FROM_ARM: f32 = 20.0;
const MUTE_FROM_RECMON: f32 = 19.0;
const SOLO_FROM_MUTE: f32 = 21.0;
const IO_FROM_SOLO: f32 = 23.0;
/// Not REAPER's: it anchors `mcp.env` and `mcp.phase` off the *bottom* of
/// the stretch section, and we do not draw an envelope button yet. The IO
/// button's own 30 rows is the honest stand-in until we do.
const PHASE_FROM_IO: f32 = 30.0;

/// One control in the right-hand column.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stacked {
    Monitor,
    Mute,
    Solo,
    Io,
    Phase,
}

impl Stacked {
    /// Where the control's own box starts, given the column's left edge.
    ///
    /// Everything sits on the column except the IO button, which
    /// `rtconfig` writes as `mcp.solo + [-1 23 23 30]` — one pixel left and
    /// two wider than the buttons above it, on purpose.
    fn left(self, column: f32) -> f32 {
        match self {
            Self::Io => column - 1.0,
            // 16 wide against the column's 21, so it centres rather than
            // hanging off the left.
            Self::Phase => column + (BUTTON_W - 16.0) / 2.0,
            _ => column,
        }
    }
}
/// `mcp.label` — 26 of the bottom section's 47, above the 20-row index
/// plate and the row that divides them.
const NAME_PLATE: u32 = 26;
/// How far the record arm's housing hangs below the coloured band.
///
/// Derived, not chosen: `mcp_recarm_*` is a 36x24 cell whose housing flares
/// out at 45° and then goes vertical at
/// [`HOUSING_SHOULDER`][daw_theme_art::vector_controls::HOUSING_SHOULDER].
/// Everything below that is a plain rectangle, and REAPER sinks exactly
/// that rectangle into the dark — so the flare emerges from the background
/// instead of the button sitting on top of the colour with a seam under it.
///
/// Measuring it off a screenshot undercounts: below the band the housing is
/// dark on dark, which is the whole point of it.
const ARM_CELL_H: f32 = 24.0;
const ARM_OVERHANG: f32 =
    ARM_CELL_H * (1.0 - daw_theme_art::vector_controls::HOUSING_SHOULDER);
/// The strip's own grey — `mcp_namebg`'s, and the same one the plate under
/// the track name uses.
const BODY_GREY: &str = "#262626";

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
    // The store first, the prop as the seed. Everything else on the strip
    // already follows the track stream, so reading colour and name off the
    // prop left the band and the plate a poll behind the buttons beside
    // them — a recolour in REAPER took up to two seconds to arrive.
    let store = use_track_store();
    let guid = props.track.guid.clone();
    let live = use_memo(use_reactive!(|guid| store.track(&guid)));
    let live = live.read();
    let track = live.as_ref().unwrap_or(&props.track);
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

    // The tinted band is `pan_sec` extended by `in_sec`, in the track's own
    // colour. An uncoloured track takes the panel grey rather than a colour
    // invented for it.
    let theme = daw_theme::Theme::default();
    let tint_colour = track
        .color
        .map(|c| daw_theme::Color::rgb((c >> 16) as u8, (c >> 8) as u8, c as u8))
        .unwrap_or(theme.chrome.surface_raised);
    let tint = tint_colour.css();
    // One row of highlight along the band's top edge — `mcp.custom.bg_hl_t`.
    // Without it the band starts flat where the source starts lit.
    let tint_hl = tint_colour.shade(0.13).css();

    let pan_h = if shape.pan == PanAnchor::PanSection { 33.0 } else { 6.0 };
    let input_h = if !shape.show_record_input {
        22.0
    } else if !shape.show_input_fx {
        42.0
    } else {
        54.0
    };
    let band = pan_h + input_h;

    // The button column, resolved on REAPER's chain. `mcp.recmon` hangs off
    // the record arm, which hangs through the band's floor — so the column's
    // first row is fixed relative to the arm rather than to the section.
    let column = {
        let mut at = ARM_OVERHANG - ARM_CELL_H + RECMON_FROM_ARM;
        let mut out = vec![(at, Stacked::Monitor)];
        for (step, control, shown) in [
            (MUTE_FROM_RECMON, Stacked::Mute, true),
            (SOLO_FROM_MUTE, Stacked::Solo, true),
            (IO_FROM_SOLO, Stacked::Io, shape.show_io),
            (PHASE_FROM_IO, Stacked::Phase, shape.show_phase),
        ] {
            at += shape.padding + step;
            if shown {
                out.push((at, control));
            }
        }
        out
    };
    // `mcp.meter` is inset 4 on every side of the stretch section.
    let meter_h = (shape.stretch - 8.0).max(24.0) as u32;

    let selected_border = if track.selected {
        "border-blue-500"
    } else {
        "border-zinc-700"
    };

    rsx! {
        div {
            class: "flex flex-col shrink-0 border-r {selected_border}",
            // The section stack is stated inline as well as in Tailwind.
            // Blitz windows embed the sheet as a static string and a panel
            // that mounts before it — or a test that mounts without it —
            // gets no `flex` at all: the fixed bands stack at their content
            // height and the stretch band collapses to nothing, so the
            // bottom section paints over the meter. Tailwind is the
            // additive layer here, not the structure.
            // #262626 stated inline, not `bg-zinc-900`: REAPER's strip body
            // is the same grey as `mcp_namebg`, which is why the plate under
            // the name looked right while everything above it sat in a
            // darker box. Inline because a class-only background disappears
            // in any window that renders before the sheet.
            style: "width:{STRIP_W}px; height:{props.height}px; \
                    display:flex; flex-direction:column; background:{BODY_GREY};",

            // Laid out on REAPER's own geometry, in Tailwind and explicit
            // pixels rather than by eye.
            //
            // Every number below is measured — the section heights and the
            // control offsets come from `rtconfig.txt` by way of
            // `daw-theme-art`'s panel sheet, which composes these same
            // components at those coordinates. The first version of this
            // strip stacked them in a flex column with its own paddings and
            // no dB scale, and the two mixers did not read as the same
            // mixer at a glance, which is the thing #135 is for.
            //
            // Structure is Tailwind (flex, flex-1, relative); position is
            // px, because REAPER's layout is a coordinate system and
            // pretending otherwise loses the match.

            // ── FX section (33 rows) ────────────────────────────
            //
            // `mcp.fx` is [7 7 43 20] of the section, with `mcp.fxbyp`
            // butted onto its right — the pill is one shape in two images.
            div {
                class: "relative shrink-0",
                style: "flex:0 0 auto; position:relative; height:{FX_SECTION}px;",
                // 9, not `mcp.fx`'s nominal 7. Measured against REAPER with
                // the coloured bands aligned: its pill's face runs 11..26 of
                // the section and ours ran 9..24 — the same 16 rows, two
                // high. The section carries a top inset the box coordinates
                // do not mention.
                div { style: "position:absolute; left:7px; top:{FX_PILL_TOP}px;",
                    FxButton { track: track.guid.clone(), width: 43.0 }
                }
            }

            // ── The tinted band: pan + input ────────────────────
            //
            // `mcp.custom.bg` is `pan_sec` extended by `in_sec`'s height,
            // in the track's own colour with one lit row along its top.
            // Fixed both ways — `grow-0 shrink-0` and a stated height — so
            // the band is the same depth on every strip whatever is inside
            // it. Left to the flex default a strip whose pan label had
            // collapsed sat a few rows shorter than its neighbours.
            div {
                class: "relative shrink-0 grow-0",
                style: "flex:0 0 auto; position:relative; height:{band}px; \
                        min-height:{band}px; max-height:{band}px; \
                        background:{tint}; border-top:1px solid {tint_hl};",

                // Pan at local x 9.5, the record arm on the right-hand
                // column's axis — both placed on the centres the source
                // gives, not on a corner.
                // The pan control is here either way — what changes is the
                // section around it. Compressed, `mcp.pan` re-anchors off
                // `mcp.recmode` and both it and the record arm sit inside
                // the band, pan at its left and the arm at its right, which
                // is where the source has them.
                div { style: "position:absolute; left:9px; top:2px;",
                    PanKnob { track: track.guid.clone() }
                    if shape.show_pan_labels {
                        div { class: "text-[8px] text-zinc-400 leading-none text-center", "pan" }
                    }
                }
                // Hung *through* the band's floor, not sat on it.
                //
                // In REAPER the housing spans rows 35..56 of a strip whose
                // coloured band ends at 52 — the last four rows land on the
                // dark below, so the round housing reads as one shape with
                // the background it is anchored to. Kept inside the band the
                // button floated in the colour with a seam under it.
                //
                // `z-index` because the stretch section is the next sibling
                // and would otherwise paint over the overhang.
                div {
                    style: "position:absolute; left:{ARM_LEFT}px; \
                            bottom:-{ARM_OVERHANG}px; z-index:2;",
                    RecordArmButton { track: track.guid.clone() }
                }
                if shape.show_record_input {
                    div {
                        class: "absolute w-full text-center",
                        style: "top:{pan_h}px;",
                        RecordInputLabel { track: track.guid.clone() }
                    }
                }
            }

            // ── Stretch: meter, fader, and the button column ────
            //
            // `mcp.meter` is one rect covering the bars *and* the scale;
            // the fader sits at x 28; and everything in the right-hand
            // column shares one axis, each control anchored to the last
            // exactly as `rtconfig` writes it:
            //
            //     mcp.recmon = mcp.recarm + [7 20 21 20]
            //     mcp.mute   = mcp.recmon + [0 19 21 20]
            //     mcp.solo   = mcp.mute   + [0 21 21 20]
            //     mcp.io     = mcp.solo   + [-1 23 23 30]
            div {
                class: "relative flex-1 min-h-0",
                style: "flex:1 1 0; min-height:0; position:relative;",
                // The numeric readout, which is a different thing from the
                // scale printed inside the meter: `hide_volume_label` drops
                // this one at 250 while the scale stays.
                if shape.show_volume_label {
                    div {
                        class: "absolute font-mono text-[9px] text-zinc-400",
                        style: "left:4px; top:-11px;",
                        "{db_label}"
                    }
                }
                // Both boxes state a height in px rather than `top/bottom`:
                // the fader's rail is `height:100%`, and a percentage of an
                // absolutely-positioned box with no stated height resolves
                // to nothing — the meter looked right only because its own
                // art carries pixel dimensions.
                div { style: "position:absolute; left:4px; top:4px; height:{meter_h}px;",
                    TrackMeter { track: track.guid.clone(), width: 24, height: meter_h }
                }
                div { style: "position:absolute; left:28px; top:4px; height:{meter_h}px;",
                    match shape.volume {
                        // Below the swap threshold a fader has no travel
                        // worth having, so it stops being a fader.
                        VolumeWidget::Fader => rsx! { VolumeFader { track: track.guid.clone() } },
                        VolumeWidget::Knob => rsx! {
                            PanKnob { track: track.guid.clone(), large: true }
                        },
                    }
                }
                // REAPER's own offset chain, not a uniform flex gap.
                //
                //     mcp.recmon = mcp.recarm + [7 20 21 20]
                //     mcp.mute   = mcp.recmon + [0 padding] + [0 19 21 20]
                //     mcp.solo   = mcp.mute   + [0 padding] + [0 21 21 20]
                //     mcp.io     = mcp.solo   + [0 padding] + [-1 23 23 30]
                //
                // The steps are not equal — 19 against a 20-tall button is a
                // one-row *overlap* before padding, and the IO button is
                // deliberately a column left and two wider. A single `gap`
                // got the first button right and then drifted, five rows by
                // the time it reached IO.
                for (top, control) in column {
                    div {
                        key: "{control:?}",
                        style: "position:absolute; left:{control.left(COLUMN)}px; \
                                top:{top}px;",
                        match control {
                            Stacked::Monitor => rsx! { MonitorButton { track: track.guid.clone() } },
                            Stacked::Mute => rsx! { MuteButton { track: track.guid.clone() } },
                            Stacked::Solo => rsx! { SoloButton { track: track.guid.clone() } },
                            Stacked::Io => rsx! { IoButton { track: track.guid.clone() } },
                            Stacked::Phase => rsx! { PhaseButton { track: track.guid.clone() } },
                        }
                    }
                }
            }

            // ── Bottom (47 rows: 26 of name, 20 of index) ──────
            div {
                class: "shrink-0 grow-0",
                style: "flex:0 0 auto; height:{BOTTOM_SECTION}px; \
                        min-height:{BOTTOM_SECTION}px; max-height:{BOTTOM_SECTION}px;",
                // 26 of the section's 47, with the index plate's 20 and a
                // dividing row under it. Left to its content the plate was
                // 16 tall and the strip ended in a band of dead grey.
                TrackName { track: track.guid.clone(), width: STRIP_W as u32, height: NAME_PLATE }
                div {
                    class: "text-center",
                    // `text-align` inline as well: without the sheet the
                    // index sat against the left edge of its own plate.
                    style: "height:20px; line-height:20px; font-size:11px; \
                            text-align:center; color:#f0f0f0; background:{tint}; \
                            border-top:1px solid {tint_hl};",
                    "{track.index + 1}"
                }
            }
        }
    }
}

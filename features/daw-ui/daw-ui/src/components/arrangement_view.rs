//! The arrangement view — ruler, lanes, items — drawn native.
//!
//! The third panel of the main window, beside `components::tcp` and above
//! `components::mixer`, built the same way: explicit inline layout on the
//! theme's tokens, no stylesheet assumed, no WALTER, no bitmaps. The lanes
//! share the TCP's row pitch — [`geometry::tcp::ROW_H`] plus its divider —
//! so a row in one panel is the same row in the other, which is the whole
//! contract between them.
//!
//! # What Blitz allows here
//!
//! Checked against blitz.is/status/css before this was built: flexbox and
//! absolute positioning are solid, `overflow:scroll` works but `auto` does
//! not, and there is no `position:sticky` — so the ruler is a sibling of
//! the lane area rather than a sticky header, and scrolling is deferred
//! until the panel needs it (the preview draws a window, not a viewport).

use crate::controls::{use_daw_tracks, use_track_store};
use crate::prelude::*;
use daw_proto::{Item, Track};

/// The ruler's band, above the lanes.
const RULER_H: f32 = 26.0;

/// One second of timeline, in pixels, at the default zoom.
///
/// Chosen so a 4/4 bar at 120 BPM is 80px — items read at a glance and an
/// eight-bar loop fits a small window. Zoom is a prop; this is its seed.
const PX_PER_SECOND: f32 = 40.0;

/// The arrange area for a caller that has the data in hand.
///
/// Pure, like `ChannelStripPreview` and `TrackRow`: tracks and items in,
/// markup out, no backend, no context — so a test, a screenshot or the
/// main-window composition can draw it without a DAW attached.
/// [`ArrangementView`] is the live wrapper.
#[component]
pub fn ArrangePreview(
    tracks: Vec<Track>,
    #[props(default)] items: Vec<Item>,
    width: f32,
    height: f32,
    /// Zoom, as pixels per second of timeline.
    #[props(default = PX_PER_SECOND)]
    pixels_per_second: f32,
    /// The tempo the ruler's bars are spaced by. One number for now — a
    /// tempo *map* changes the arithmetic from a stride to a walk, and
    /// nothing here draws tempo changes yet.
    #[props(default = 120.0)]
    bpm: f64,
    /// The lane pitch. Defaults to the TCP's row so the two panels line
    /// up; the caller that draws taller rows passes what it drew.
    #[props(default = daw_theme_art::geometry::tcp::ROW_H)]
    row_h: f32,
) -> Element {
    let t = daw_theme::Theme::default();
    // The arrange ground is the sunken surface — the one place the theme
    // reaches *down* the ladder, which is what makes the panels around it
    // read as raised.
    let ground = t.chrome.surface_sunken.css();
    let ruler_bg = t.chrome.surface_sunken.shade(-0.12).css();
    let ruler_ink = t.chrome.text_dim.css();
    let rule = t.chrome.surface_sunken.shade(-0.25).css();
    // Bar lines are cut into the ground, beat lines barely so.
    let bar_line = t.chrome.surface_sunken.shade(-0.18).css();
    let beat_line = t.chrome.surface_sunken.shade(-0.07).css();
    let lane_rule = t.chrome.surface_sunken.shade(-0.15).css();

    // 4/4 until the tempo map is wired; the ruler numbers measures.
    let bar_secs = 240.0 / bpm.max(1.0);
    let bar_px = bar_secs as f32 * pixels_per_second;
    let bars = (width / bar_px).ceil() as usize + 1;
    // Beat lines only when they have room to be lines rather than moiré.
    let beat_px = bar_px / 4.0;
    let draw_beats = beat_px >= 12.0;

    let lanes_h = height - RULER_H;

    rsx! {
        div {
            style: "position:relative; width:{width}px; height:{height}px; \
                    background:{ground}; overflow:hidden; \
                    display:flex; flex-direction:column;",

            // ── The ruler ──
            div {
                style: "position:relative; height:{RULER_H}px; flex:0 0 auto; \
                        background:{ruler_bg}; border-bottom:1px solid {rule}; \
                        overflow:hidden;",
                for bar in 0..bars {
                    div {
                        key: "r{bar}",
                        style: "position:absolute; left:{bar as f32 * bar_px}px; top:0; \
                                height:{RULER_H}px; border-left:1px solid {bar_line}; \
                                padding-left:4px; font-size:9px; \
                                line-height:{RULER_H}px; color:{ruler_ink}; \
                                font-family:Fira Sans, DejaVu Sans, sans-serif;",
                        "{bar + 1}"
                    }
                }
            }

            // ── The lanes ──
            div {
                style: "position:relative; flex:1 1 0; min-height:0; overflow:hidden;",

                // The grid, behind everything: a line per bar, and per
                // beat when the zoom leaves room for them.
                for bar in 1..bars {
                    div {
                        key: "b{bar}",
                        style: "position:absolute; left:{bar as f32 * bar_px}px; top:0; \
                                width:1px; height:{lanes_h}px; background:{bar_line};",
                    }
                }
                if draw_beats {
                    for beat in 1..bars * 4 {
                        if beat % 4 != 0 {
                            div {
                                key: "t{beat}",
                                style: "position:absolute; left:{beat as f32 * beat_px}px; \
                                        top:0; width:1px; height:{lanes_h}px; \
                                        background:{beat_line};",
                            }
                        }
                    }
                }

                // A lane per track, on the TCP's pitch: row plus one
                // divider. The lane carries the track's colour as a tint
                // faint enough to organise without shouting.
                for (i, track) in tracks.iter().enumerate() {
                    {
                        let top = i as f32 * (row_h + 1.0);
                        let tint = track
                            .color
                            .map(|c| format!(
                                "rgba({}, {}, {}, 0.05)",
                                (c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff
                            ))
                            .unwrap_or_else(|| "transparent".to_string());
                        rsx! {
                            div {
                                key: "{track.guid}",
                                style: "position:absolute; left:0; top:{top}px; \
                                        width:{width}px; height:{row_h}px; \
                                        background:{tint}; \
                                        border-bottom:1px solid {lane_rule};",
                            }
                        }
                    }
                }

                // The items, over the lanes and the grid.
                for item in items.iter() {
                    {
                        let lane = tracks.iter().position(|tr| tr.guid == item.track_guid);
                        match lane {
                            Some(i) => rsx! {
                                ArrangeItem {
                                    key: "{item.guid}",
                                    item: item.clone(),
                                    track_colour: tracks[i].color,
                                    top: i as f32 * (row_h + 1.0),
                                    height: row_h,
                                    pixels_per_second,
                                }
                            },
                            // An item whose track is not in the list draws
                            // nothing — better than inventing a lane.
                            None => rsx! {},
                        }
                    }
                }
            }
        }
    }
}

/// One item on a lane.
///
/// REAPER's item is a block of the track's colour with a thin darker
/// outline and its label set small in the top-left; muted items fade and
/// a selected item brightens its border. The fades and per-take lanes
/// come later — this is the block.
#[component]
fn ArrangeItem(
    item: Item,
    track_colour: Option<u32>,
    top: f32,
    height: f32,
    pixels_per_second: f32,
) -> Element {
    let t = daw_theme::Theme::default();
    let left = item.position.as_seconds() as f32 * pixels_per_second;
    let w = (item.length.as_seconds() as f32 * pixels_per_second).max(2.0);

    // The item's own colour beats the track's, as in REAPER.
    let colour = item.color.or(track_colour);
    let body = colour
        .map(|c| daw_theme_art::dress::panel_tint(daw_theme::Color::rgb(
            (c >> 16) as u8,
            (c >> 8) as u8,
            c as u8,
        )))
        .unwrap_or(t.signal.neutral_track);
    let edge = if item.selected {
        t.chrome.selected.css()
    } else {
        body.shade(-0.45).css()
    };
    let ink = body.shade(0.6).css();
    let alpha = if item.muted { 0.45 } else { 1.0 };
    let label = item.label.clone().unwrap_or_default();

    rsx! {
        div {
            style: "position:absolute; left:{left}px; top:{top + 1.0}px; \
                    width:{w}px; height:{height - 3.0}px; \
                    background:{body.css()}; border:1px solid {edge}; \
                    border-radius:3px; opacity:{alpha}; overflow:hidden;",
            if !label.is_empty() {
                div {
                    style: "padding:1px 4px; font-size:9px; line-height:11px; \
                            color:{ink}; white-space:nowrap; overflow:hidden; \
                            text-overflow:ellipsis; \
                            font-family:Fira Sans, DejaVu Sans, sans-serif;",
                    "{label}"
                }
            }
        }
    }
}

/// The live arrangement view: [`ArrangePreview`] fed from the DAW.
///
/// Tracks arrive on the track stream through the shared store, like every
/// panel; items are polled, because there is no item `#[subscribe]` stream
/// yet. The poll waits on the connection with `futures_timer`, **not**
/// tokio — this panel runs inside REAPER on dioxus's own scheduler, where
/// a tokio timer is a non-unwinding panic that takes the host down (the
/// placeholder this replaces had exactly that bug).
#[component]
pub fn ArrangementView() -> Element {
    let store = use_track_store();
    use_daw_tracks(store);

    let mut items = use_signal(Vec::<Item>::new);
    let mut connected = use_signal(|| false);
    let mut size = use_signal(|| Option::<(f32, f32)>::None);

    use_future(move || async move {
        let Some(project) = crate::controls::reach::connected_project().await else {
            return;
        };
        connected.set(true);
        loop {
            if let Ok(list) = project.items().all().await {
                items.set(list);
            }
            futures_timer::Delay::new(std::time::Duration::from_secs(2)).await;
        }
    });

    if !*connected.read() {
        return rsx! {
            div {
                style: "height:100%; width:100%; display:flex; align-items:center; \
                        justify-content:center; font-size:12px; color:#7b7b7b;",
                "Waiting for DAW connection..."
            }
        };
    }

    // Project order from the store's own ordering — the same order the
    // meter frame is indexed by.
    let tracks: Vec<Track> = store
        .order()
        .iter()
        .filter_map(|guid| store.track(guid))
        .collect();
    let item_list = items.read().clone();
    // Drawn at the box the dock gives, measured on mount — the same move
    // the mixer makes, for the same reason.
    let (w, h) = size.read().unwrap_or((800.0, 600.0));

    rsx! {
        div {
            style: "height:100%; width:100%; overflow:hidden;",
            onmounted: move |evt| {
                spawn(async move {
                    if let Ok(rect) = evt.get_client_rect().await {
                        if rect.size.width > 0.0 && rect.size.height > 0.0 {
                            size.set(Some((rect.size.width as f32, rect.size.height as f32)));
                        }
                    }
                });
            },
            ArrangePreview { tracks, items: item_list, width: w, height: h }
        }
    }
}

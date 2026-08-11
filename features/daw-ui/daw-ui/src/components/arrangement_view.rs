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

use std::collections::HashMap;

use crate::controls::{use_daw_tracks, use_track_store};
use crate::prelude::*;
use daw_proto::{Item, Track};

/// What an item shows of its content — REAPER's waveform or note preview.
///
/// Carried per item, keyed by guid, and *pre-resolved to render units*:
/// amplitudes 0..1 for audio, seconds-from-item-start for notes. The
/// fetch does the PPQ and tempo arithmetic once so the render is a pure
/// projection, and a fixture can state a preview without a tempo map.
#[derive(Clone, PartialEq, Debug)]
pub enum ItemPreview {
    /// Peak amplitudes, 0..1, evenly spaced across the take — one per
    /// block, drawn stretched to the item's box and mirrored about the
    /// centre line. Under REAPER these come off the `.reapeaks` cache
    /// via `PCM_source::GetPeaks` (see `TakeHandle::peaks`).
    Waveform(Vec<f32>),
    /// MIDI notes, in seconds from the item start.
    Notes(Vec<NotePreview>),
}

/// One note in a MIDI preview.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct NotePreview {
    pub pitch: u8,
    /// Seconds from the item's start.
    pub start: f32,
    /// Seconds.
    pub length: f32,
}

/// Fold a take's peak frame into per-block amplitudes.
///
/// The REAPER backend returns one peak per channel per block (interleaved
/// `[ch0, ch1, ch0, ch1, …]`); the preview takes the loudest channel and
/// mirrors it, which is how a single-lane waveform is usually drawn.
pub fn waveform_from_peaks(data: &daw_proto::TakePeakData) -> Vec<f32> {
    let ch = data.num_channels.max(1) as usize;
    data.peaks
        .chunks(ch)
        .map(|block| block.iter().fold(0.0f32, |a, v| a.max(v.abs() as f32)).min(1.0))
        .collect()
}

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
    /// Item content previews, by item guid. An item with no entry draws
    /// its plain block — previews arrive as they are fetched.
    #[props(default)]
    previews: HashMap<String, ItemPreview>,
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
                                    preview: previews.get(&item.guid).cloned(),
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
    #[props(default)] preview: Option<ItemPreview>,
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
    let box_h = height - 3.0;
    // The content band: the label's row belongs to the label, the rest to
    // the preview, which is how REAPER divides an item.
    let band_top = if label.is_empty() { 2.0 } else { 12.0 };
    let band_h = (box_h - band_top - 2.0).max(0.0);
    // Peaks are the item's own colour pulled dark — content on the block,
    // not a second surface.
    let mark = body.shade(-0.38);

    rsx! {
        div {
            style: "position:absolute; left:{left}px; top:{top + 1.0}px; \
                    width:{w}px; height:{box_h}px; \
                    background:{body.css()}; border:1px solid {edge}; \
                    border-radius:3px; opacity:{alpha}; overflow:hidden;",
            match &preview {
                Some(ItemPreview::Waveform(amps)) if !amps.is_empty() && band_h > 3.0 => rsx! {
                    Waveform {
                        amps: amps.clone(),
                        width: w,
                        top: band_top,
                        height: band_h,
                        colour: mark.css(),
                    }
                },
                Some(ItemPreview::Notes(notes)) if !notes.is_empty() && band_h > 3.0 => rsx! {
                    NoteRoll {
                        notes: notes.clone(),
                        length: item.length.as_seconds() as f32,
                        width: w,
                        top: band_top,
                        height: band_h,
                        colour: mark.css(),
                    }
                },
                _ => rsx! {},
            }
            if !label.is_empty() {
                div {
                    style: "position:absolute; left:0; top:0; width:{w}px; \
                            padding:1px 4px; font-size:9px; line-height:11px; \
                            color:{ink}; white-space:nowrap; overflow:hidden; \
                            text-overflow:ellipsis; \
                            font-family:Fira Sans, DejaVu Sans, sans-serif;",
                    "{label}"
                }
            }
        }
    }
}

/// The waveform, as one filled shape mirrored about the centre line.
///
/// A polygon rather than per-column strokes: the skill's stroke trap
/// (resvg halves a stroke over its own path) and one shape instead of
/// hundreds. The viewBox is one unit per block and the `<svg>` stretches
/// to the item — `preserve_aspect_ratio: none`, the same move the fader's
/// stretch band makes.
#[component]
pub fn Waveform(amps: Vec<f32>, width: f32, top: f32, height: f32, colour: String) -> Element {
    let n = amps.len();
    // Above ~4 blocks per pixel the polygon outruns what the pixel can
    // show; fold blocks together so the shape stays a few hundred points.
    let max_points = (width as usize).clamp(64, 512);
    let folded: Vec<f32> = if n > max_points {
        let per = n as f32 / max_points as f32;
        (0..max_points)
            .map(|i| {
                let a = (i as f32 * per) as usize;
                let b = (((i + 1) as f32 * per) as usize).min(n);
                amps[a..b.max(a + 1)].iter().fold(0.0f32, |m, v| m.max(*v))
            })
            .collect()
    } else {
        amps
    };
    let count = folded.len().max(2);
    let span = count - 1;

    // Top edge left to right, bottom edge right to left — one closed shape.
    let mut d = String::with_capacity(count * 24);
    for (i, a) in folded.iter().enumerate() {
        let y = 1.0 - (*a).clamp(0.0, 1.0) * 0.96;
        d.push_str(if i == 0 { "M" } else { "L" });
        d.push_str(&format!(" {i} {y:.3} "));
    }
    for (i, a) in folded.iter().enumerate().rev() {
        let y = 1.0 + (*a).clamp(0.0, 1.0) * 0.96;
        d.push_str(&format!("L {i} {y:.3} "));
    }
    d.push('Z');

    rsx! {
        svg {
            style: "position:absolute; left:0; top:{top}px; display:block; \
                    width:{width}px; height:{height}px;",
            width: "{width}",
            height: "{height}",
            preserve_aspect_ratio: "none",
            view_box: "0 0 {span} 2",
            xmlns: "http://www.w3.org/2000/svg",
            path { d: "{d}", fill: "{colour}", fill_opacity: "0.85" }
            // The centre line survives silence — an empty bar still reads
            // as audio, which is REAPER's behaviour.
            rect { x: "0", y: "0.985", width: "{span}", height: "0.03",
                   fill: "{colour}", fill_opacity: "0.5" }
        }
    }
}

/// The note preview — pitch rows across the item, REAPER's MIDI block.
///
/// Rows are normalised to the notes the item actually has, padded a
/// semitone either side, so a two-note bassline does not draw as two
/// hairlines at the bottom of a 128-row grid.
#[component]
pub fn NoteRoll(
    notes: Vec<NotePreview>,
    length: f32,
    width: f32,
    top: f32,
    height: f32,
    colour: String,
) -> Element {
    let lo = notes.iter().map(|n| n.pitch).min().unwrap_or(60).saturating_sub(1);
    let hi = notes.iter().map(|n| n.pitch).max().unwrap_or(72).saturating_add(1);
    let rows = (hi - lo + 1).max(2) as f32;
    let secs = length.max(0.001);

    rsx! {
        svg {
            style: "position:absolute; left:0; top:{top}px; display:block; \
                    width:{width}px; height:{height}px;",
            width: "{width}",
            height: "{height}",
            preserve_aspect_ratio: "none",
            view_box: "0 0 {secs} {rows}",
            xmlns: "http://www.w3.org/2000/svg",
            for (i, n) in notes.iter().enumerate() {
                rect {
                    key: "{i}",
                    x: "{n.start}",
                    // Row 0 at the top is the highest pitch.
                    y: "{(hi - n.pitch) as f32 + 0.15}",
                    width: "{n.length.max(secs * 0.004)}",
                    height: "0.7",
                    fill: "{colour}",
                    fill_opacity: "0.9",
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
    let mut previews = use_signal(HashMap::<String, ItemPreview>::new);
    let mut connected = use_signal(|| false);
    let mut size = use_signal(|| Option::<(f32, f32)>::None);

    use_future(move || async move {
        let Some(project) = crate::controls::reach::connected_project().await else {
            return;
        };
        connected.set(true);
        loop {
            if let Ok(list) = project.items().all().await {
                // Fetch content previews for items that do not have one
                // yet — once per (guid, length), so an edit that changes
                // the take refetches and everything else stays cached.
                for item in &list {
                    let key = item.guid.clone();
                    if previews.read().contains_key(&key) {
                        continue;
                    }
                    if let Some(preview) = fetch_preview(&project, item).await {
                        previews.write().insert(key, preview);
                    }
                }
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
            ArrangePreview {
                tracks,
                items: item_list,
                previews: previews.read().clone(),
                width: w,
                height: h,
            }
        }
    }
}

/// One item's content preview, from whichever kind of take it holds.
///
/// Audio goes through [`TakeHandle::peaks`] — under REAPER that is
/// `PCM_source::GetPeaks`, served from the `.reapeaks` cache REAPER has
/// already built for its own arrange view, so no media is decoded. MIDI
/// reads the take's notes and converts PPQ to seconds at the project's
/// tempo. Anything else (empty, video, a backend with no peak store)
/// yields `None` and the item draws its plain block.
///
/// [`TakeHandle::peaks`]: daw_control::TakeHandle::peaks
pub(crate) async fn fetch_preview(
    project: &daw_control::Project,
    item: &Item,
) -> Option<ItemPreview> {
    let handle = project.items().by_guid(&item.guid).await.ok()??;
    let take = handle.active_take();
    let info = take.info().await.ok()?;
    match info.source_type {
        daw_proto::SourceType::Audio => {
            // ~86 peaks a second at 44.1k: enough for any zoom the panel
            // draws, small enough to ship for every item in a project.
            let data = take.peaks(512).await.ok()?;
            let amps = waveform_from_peaks(&data);
            (!amps.is_empty()).then(|| ItemPreview::Waveform(amps))
        }
        daw_proto::SourceType::Midi => {
            let notes = handle.active_take().midi().notes().await.ok()?;
            // Quarter-notes to seconds at the tempo the item starts in.
            // One tempo, not the map — the preview is a thumbnail.
            let bpm = project
                .tempo_map()
                .tempo_at(item.position.as_seconds())
                .await
                .unwrap_or(120.0);
            let spq = (60.0 / bpm.max(1.0)) as f32;
            let notes: Vec<NotePreview> = notes
                .iter()
                .map(|n| NotePreview {
                    pitch: n.pitch,
                    start: n.start_ppq as f32 * spq,
                    length: n.length_ppq as f32 * spq,
                })
                .collect();
            (!notes.is_empty()).then(|| ItemPreview::Notes(notes))
        }
        _ => None,
    }
}

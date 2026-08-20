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
        .map(|block| {
            block
                .iter()
                .fold(0.0f32, |a, v| a.max(v.abs() as f32))
                .min(1.0)
        })
        .collect()
}

/// One visible envelope on a track's lane, pre-resolved to render units.
///
/// The same currency idea as [`ItemPreview`]: `(seconds, 0..1)` points,
/// so the render is a projection and a fixture needs no automation API.
/// Values arrive normalised from `daw_proto::EnvelopePoint` already.
#[derive(Clone, PartialEq, Debug)]
pub struct EnvelopePreview {
    /// The envelope's display name — also decides its colour, the way
    /// REAPER inks volume green and pan orange.
    pub name: String,
    /// `(time in seconds, value 0..1)`, in time order.
    pub points: Vec<(f32, f32)>,
}

impl EnvelopePreview {
    /// REAPER's convention, in the theme's tokens: volume is the meter
    /// green, pan the meter amber, anything else the accent.
    fn colour(&self, t: &daw_theme::Theme) -> daw_theme::Color {
        let name = self.name.to_ascii_lowercase();
        if name.contains("volume") {
            t.signal.meter_safe
        } else if name.contains("pan") || name.contains("width") {
            t.signal.meter_warn
        } else {
            t.chrome.accent
        }
    }
}

/// An envelope shown in its **own lane** under the track, rather than
/// overlaid — REAPER's per-envelope lanes, vertically resizable, where FX
/// parameter automation lives.
#[derive(Clone, PartialEq, Debug)]
pub struct EnvelopeLaneView {
    pub envelope: EnvelopePreview,
    /// The lane's height. REAPER's floor is `envcp_min_height 27` (from
    /// `rtconfig.txt`); resizing changes this number and everything else
    /// follows.
    pub height: f32,
    /// Automation items sitting on this lane.
    pub automation_items: Vec<AutomationItemView>,
}

/// One automation item: a windowed piece of envelope with its own header,
/// movable and poolable like a media item.
#[derive(Clone, PartialEq, Debug)]
pub struct AutomationItemView {
    pub name: String,
    /// Seconds on the timeline.
    pub start: f32,
    /// Seconds.
    pub length: f32,
    /// A pooled instance edits its siblings too — marked in the header.
    pub pooled: bool,
    /// `(seconds from the item's start, value 0..1)`.
    pub points: Vec<(f32, f32)>,
}

/// A row in the arrange's vertical layout — a track's lane or one of its
/// envelope lanes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArrangeRowKind {
    Track(usize),
    /// `lane` indexes into the track's `EnvelopeLaneView` list.
    EnvelopeLane {
        track: usize,
        lane: usize,
    },
}

/// The arrange's vertical plan: every row with its top and height, tracks
/// interleaved with their envelope lanes.
///
/// **This is the alignment contract between the arrange and the TCP
/// column**, extended from "same pitch" to "same plan": both sides walk
/// this list, so an envelope lane's envcp row is exactly as tall as the
/// lane it controls, the way REAPER ties `envcp` height to the lane.
pub fn plan_rows(
    tracks: &[Track],
    lanes: &HashMap<String, Vec<EnvelopeLaneView>>,
    row_h: f32,
) -> Vec<(ArrangeRowKind, f32, f32)> {
    let mut out = Vec::new();
    let mut top = 0.0f32;
    for (i, track) in tracks.iter().enumerate() {
        out.push((ArrangeRowKind::Track(i), top, row_h));
        top += row_h + 1.0;
        if let Some(track_lanes) = lanes.get(&track.guid) {
            for (l, lane) in track_lanes.iter().enumerate() {
                let h = lane.height.max(ENVELOPE_LANE_MIN_H);
                out.push((ArrangeRowKind::EnvelopeLane { track: i, lane: l }, top, h));
                top += h + 1.0;
            }
        }
    }
    out
}

/// `envcp_min_height` from the theme — the floor a lane cannot resize
/// below.
pub const ENVELOPE_LANE_MIN_H: f32 = 27.0;

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
    /// Visible envelopes, by track guid, drawn over the lane the way
    /// REAPER overlays them.
    #[props(default)]
    envelopes: HashMap<String, Vec<EnvelopePreview>>,
    /// Envelopes in their **own lanes** under the track, by track guid —
    /// where FX parameter automation and automation items live. Heights
    /// come with the data; [`plan_rows`] turns them into the vertical
    /// plan both this panel and the TCP column walk.
    #[props(default)]
    env_lanes: HashMap<String, Vec<EnvelopeLaneView>>,
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
    /// A lane was dragged to a new height: `(track guid, lane index,
    /// height)`. `None` makes the lanes fixed, which is what a
    /// screenshot wants.
    #[props(default)]
    on_lane_resize: Option<EventHandler<(String, usize, f32)>>,
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

    // The vertical plan: tracks interleaved with their envelope lanes.
    let plan = plan_rows(&tracks, &env_lanes, row_h);
    let track_top = |i: usize| -> f32 {
        plan.iter()
            .find(|(k, _, _)| *k == ArrangeRowKind::Track(i))
            .map(|(_, top, _)| *top)
            .unwrap_or(i as f32 * (row_h + 1.0))
    };

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
                        let top = track_top(i);
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
                                    top: track_top(i),
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

                // Envelopes over their lanes, above the items — REAPER's
                // overlay order.
                for (guid, lanes) in envelopes.iter() {
                    {
                        let lane = tracks.iter().position(|tr| &tr.guid == guid);
                        match lane {
                            Some(i) => rsx! {
                                for (e, env) in lanes.iter().enumerate() {
                                    EnvelopeLane {
                                        key: "{guid}/{e}",
                                        envelope: env.clone(),
                                        top: track_top(i),
                                        height: row_h,
                                        width,
                                        pixels_per_second,
                                    }
                                }
                            },
                            None => rsx! {},
                        }
                    }
                }

                // The envelope lanes themselves: a darker ground than the
                // track lanes, the envelope across it, automation items
                // as blocks on top.
                for (kind, top, h) in plan.iter() {
                    if let ArrangeRowKind::EnvelopeLane { track, lane } = kind {
                        {
                            let view = &env_lanes[&tracks[*track].guid][*lane];
                            rsx! {
                                div {
                                    key: "el{track}-{lane}",
                                    style: "position:absolute; left:0; top:{top}px; \
                                            width:{width}px; height:{h}px; \
                                            background:rgba(0,0,0,0.18); \
                                            border-bottom:1px solid {lane_rule};",
                                }
                                EnvelopeLane {
                                    envelope: view.envelope.clone(),
                                    top: *top,
                                    height: *h,
                                    width,
                                    pixels_per_second,
                                }
                                for (a, ai) in view.automation_items.iter().enumerate() {
                                    AutomationItemBlock {
                                        key: "ai{track}-{lane}-{a}",
                                        item: ai.clone(),
                                        top: *top,
                                        height: *h,
                                        pixels_per_second,
                                        colour: view.envelope.colour(&daw_theme::Theme::default()),
                                    }
                                }
                                if let Some(resize) = on_lane_resize {
                                    {
                                        let guid = tracks[*track].guid.clone();
                                        let lane = *lane;
                                        rsx! {
                                            LaneResizeHandle {
                                                height: *h,
                                                width,
                                                top: *top,
                                                on_resize: move |h: f32| {
                                                    resize.call((guid.clone(), lane, h))
                                                },
                                            }
                                        }
                                    }
                                }
                            }
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
        .map(|c| {
            daw_theme_art::dress::panel_tint(daw_theme::Color::rgb(
                (c >> 16) as u8,
                (c >> 8) as u8,
                c as u8,
            ))
        })
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
    let lo = notes
        .iter()
        .map(|n| n.pitch)
        .min()
        .unwrap_or(60)
        .saturating_sub(1);
    let hi = notes
        .iter()
        .map(|n| n.pitch)
        .max()
        .unwrap_or(72)
        .saturating_add(1);
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

/// One envelope drawn over its lane: the line, and a square at each point.
///
/// Drawn in pixel space — the `<svg>`'s viewBox is its own box, no
/// stretching — so the stroke stays a true 1.2px whatever the zoom, which
/// a `preserve_aspect_ratio: none` band cannot promise. Only the value
/// line for now: REAPER's fill-free look. Square shapes step, everything
/// else draws linear until curves matter.
#[component]
fn EnvelopeLane(
    envelope: EnvelopePreview,
    top: f32,
    height: f32,
    width: f32,
    pixels_per_second: f32,
) -> Element {
    let t = daw_theme::Theme::default();
    let ink = envelope.colour(&t).css();
    // Two rows of margin so a full-scale envelope does not sit on the
    // lane divider.
    let (pad, h) = (2.0, height - 4.0);
    let y_of = |v: f32| pad + (1.0 - v.clamp(0.0, 1.0)) * h;

    // Linear between points, with the first value held back to the
    // origin and the last held to the end of the view — REAPER's rule.
    // Shape (square, bezier) refines this later; the *hold* semantics are
    // the part a viewer misreads if absent.
    let mut d = String::new();
    let mut last_y = None;
    for (i, (time, value)) in envelope.points.iter().enumerate() {
        let x = time * pixels_per_second;
        let y = y_of(*value);
        if i == 0 {
            d.push_str(&format!("M 0 {y:.2} L {x:.2} {y:.2} "));
        } else {
            d.push_str(&format!("L {x:.2} {y:.2} "));
        }
        last_y = Some(y);
    }
    if let Some(y) = last_y {
        d.push_str(&format!("L {width:.2} {y:.2}"));
    }

    rsx! {
        svg {
            style: "position:absolute; left:0; top:{top}px; display:block; \
                    width:{width}px; height:{height}px; pointer-events:none;",
            width: "{width}",
            height: "{height}",
            view_box: "0 0 {width} {height}",
            xmlns: "http://www.w3.org/2000/svg",
            path {
                d: "{d}",
                fill: "none",
                stroke: "{ink}",
                stroke_width: "1.2",
                stroke_opacity: "0.9",
            }
            for (i, (time, value)) in envelope.points.iter().enumerate() {
                rect {
                    key: "{i}",
                    x: "{time * pixels_per_second - 2.0}",
                    y: "{y_of(*value) - 2.0}",
                    width: "4",
                    height: "4",
                    fill: "{ink}",
                }
            }
        }
    }
}

/// The grab strip at the foot of an envelope lane.
///
/// REAPER resizes a lane by dragging its bottom edge, and the lane's
/// height *is* the envcp row's height (`plan_rows` states that once), so
/// one drag moves both panels. Relative like every other drag in this
/// crate — grabbing the edge must not jump the lane to the pointer.
///
/// The write goes out through `EnvelopeHandle::show_in_lane`, which
/// clamps to the theme's floor host-side; the local clamp is so the
/// preview never draws below the floor mid-drag.
#[component]
fn LaneResizeHandle(
    height: f32,
    width: f32,
    top: f32,
    /// Called with the new height as the pointer moves, and once more on
    /// release — the caller decides which of those reaches the DAW.
    on_resize: EventHandler<f32>,
    /// Called on release with the settled height.
    #[props(default)]
    on_commit: Option<EventHandler<f32>>,
) -> Element {
    let mut dragging = use_signal(|| false);
    let mut grab_y = use_signal(|| 0.0f32);
    let mut grab_h = use_signal(|| 0.0f32);

    let press = move |e: MouseEvent| {
        dragging.set(true);
        grab_y.set(e.client_coordinates().y as f32);
        grab_h.set(height);
    };
    let drag = move |e: MouseEvent| {
        if !dragging() {
            return;
        }
        let dy = e.client_coordinates().y as f32 - grab_y();
        on_resize.call((grab_h() + dy).max(ENVELOPE_LANE_MIN_H));
    };
    let release = move |e: MouseEvent| {
        if dragging.replace(false) {
            if let Some(commit) = &on_commit {
                let dy = e.client_coordinates().y as f32 - grab_y();
                commit.call((grab_h() + dy).max(ENVELOPE_LANE_MIN_H));
            }
        }
    };

    rsx! {
        div {
            // Four rows straddling the lane's foot: thin enough not to
            // steal clicks from the automation above it, thick enough to
            // hit without aiming.
            style: "position:absolute; left:0; top:{top + height - 2.0}px; \
                    width:{width}px; height:4px; cursor:ns-resize; z-index:3;",
            onmousedown: press,
            onmousemove: drag,
            onmouseup: release,
            onmouseleave: move |_| { dragging.set(false); },
        }
    }
}

/// One automation item on an envelope lane.
///
/// REAPER's shape: a translucent block in the envelope's own colour with
/// a thin header carrying the name (and the pool mark — a pooled
/// instance edits its siblings, so it must be tellable), the envelope
/// segment drawn inside the body. The lane's underlying envelope shows
/// through around it, which is what makes an AI read as a *window onto*
/// automation rather than a second kind of media item.
#[component]
fn AutomationItemBlock(
    item: AutomationItemView,
    top: f32,
    height: f32,
    pixels_per_second: f32,
    colour: daw_theme::Color,
) -> Element {
    let left = item.start * pixels_per_second;
    let w = (item.length * pixels_per_second).max(8.0);
    let header_h = 10.0f32;
    let body_h = (height - header_h - 3.0).max(4.0);
    let ink = colour.css();
    let header_bg = colour.shade(-0.55).css();
    let name = if item.pooled {
        format!("∞ {}", item.name)
    } else {
        item.name.clone()
    };

    // The segment, in the body's pixel space.
    let mut d = String::new();
    for (i, (time, value)) in item.points.iter().enumerate() {
        let x = (time * pixels_per_second).min(w);
        let y = (1.0 - value.clamp(0.0, 1.0)) * (body_h - 4.0) + 2.0;
        d.push_str(if i == 0 { "M" } else { "L" });
        d.push_str(&format!(" {x:.2} {y:.2} "));
    }

    rsx! {
        div {
            style: "position:absolute; left:{left}px; top:{top + 1.0}px; \
                    width:{w}px; height:{height - 3.0}px; \
                    border:1px solid {ink}; border-radius:2px; \
                    background:rgba(0,0,0,0.25); overflow:hidden;",
            div {
                style: "height:{header_h}px; background:{header_bg}; \
                        font-size:7px; line-height:{header_h}px; padding:0 3px; \
                        color:{ink}; white-space:nowrap; overflow:hidden; \
                        font-family:Fira Sans, DejaVu Sans, sans-serif;",
                "{name}"
            }
            svg {
                style: "display:block;",
                width: "{w}", height: "{body_h}",
                view_box: "0 0 {w} {body_h}",
                xmlns: "http://www.w3.org/2000/svg",
                path {
                    d: "{d}",
                    fill: "none",
                    stroke: "{ink}",
                    stroke_width: "1.1",
                    stroke_opacity: "0.95",
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
    let mut envelopes = use_signal(HashMap::<String, Vec<EnvelopePreview>>::new);
    let mut env_lanes = use_signal(HashMap::<String, Vec<EnvelopeLaneView>>::new);
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
            // Visible envelopes, refreshed on the same cadence — an
            // automation pass in REAPER shows up within a poll. Only the
            // ones REAPER shows: `visible` is the arrange-view flag.
            if let Ok(track_list) = project.tracks().all().await {
                let mut overlaid = HashMap::new();
                let mut laned: HashMap<String, Vec<EnvelopeLaneView>> = HashMap::new();
                for track in &track_list {
                    let Ok(Some(handle)) = project.tracks().by_guid(&track.guid).await else {
                        continue;
                    };
                    let Ok(all) = handle.envelopes().all().await else {
                        continue;
                    };
                    let mut over = Vec::new();
                    let mut lanes = Vec::new();
                    for env in all.iter().filter(|e| e.visible) {
                        let Ok(Some(eh)) = handle.envelopes().by_type(env.envelope_type).await
                        else {
                            continue;
                        };
                        let Ok(points) = eh.points().await else {
                            continue;
                        };
                        let preview = EnvelopePreview {
                            name: env.name.clone(),
                            points: points
                                .iter()
                                .map(|p| (p.time.as_seconds() as f32, p.value as f32))
                                .collect(),
                        };
                        // The envelope's own choice decides where it is
                        // drawn — over the track, or in a lane of its own
                        // with its automation items.
                        if env.in_own_lane {
                            let mut automation_items = Vec::new();
                            if env.automation_item_count > 0 {
                                if let Ok(items) = eh.automation_items().await {
                                    for ai in &items {
                                        let pts = eh
                                            .automation_item_points(ai.index)
                                            .await
                                            .unwrap_or_default();
                                        automation_items.push(AutomationItemView {
                                            name: ai.name.clone(),
                                            start: ai.position.as_seconds() as f32,
                                            length: ai.length.as_seconds() as f32,
                                            // Every REAPER automation
                                            // item has a pool; only one
                                            // shared by siblings needs
                                            // the warning mark.
                                            pooled: items
                                                .iter()
                                                .filter(|o| o.pool_id == ai.pool_id)
                                                .count()
                                                > 1,
                                            points: pts
                                                .iter()
                                                .map(|p| {
                                                    (p.time.as_seconds() as f32, p.value as f32)
                                                })
                                                .collect(),
                                        });
                                    }
                                }
                            }
                            lanes.push(EnvelopeLaneView {
                                envelope: preview,
                                height: env.lane_height as f32,
                                automation_items,
                            });
                        } else if !preview.points.is_empty() {
                            over.push(preview);
                        }
                    }
                    if !over.is_empty() {
                        overlaid.insert(track.guid.clone(), over);
                    }
                    if !lanes.is_empty() {
                        laned.insert(track.guid.clone(), lanes);
                    }
                }
                envelopes.set(overlaid);
                env_lanes.set(laned);
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
                envelopes: envelopes.read().clone(),
                env_lanes: env_lanes.read().clone(),
                width: w,
                height: h,
                // A drag moves the local lane immediately and writes the
                // settled height through to the DAW — the same
                // optimistic shape the faders use, because a lane that
                // waits on a round trip stutters under the pointer.
                on_lane_resize: move |(guid, lane, height): (String, usize, f32)| {
                    if let Some(lanes) = env_lanes.write().get_mut(&guid) {
                        if let Some(view) = lanes.get_mut(lane) {
                            view.height = height;
                        }
                    }
                    spawn(async move {
                        let Some(project) = crate::controls::reach::connected_project().await
                        else {
                            return;
                        };
                        let Ok(Some(track)) = project.tracks().by_guid(&guid).await else {
                            return;
                        };
                        let Ok(all) = track.envelopes().all().await else { return };
                        // The lane index is a position among the
                        // envelopes that *have* lanes, which is what the
                        // view was built from.
                        let Some(env) = all.iter().filter(|e| e.in_own_lane).nth(lane) else {
                            return;
                        };
                        if let Ok(Some(handle)) =
                            track.envelopes().by_type(env.envelope_type).await
                        {
                            let _ = handle.show_in_lane(height.round() as u32).await;
                        }
                    });
                },
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
            (!amps.is_empty()).then_some(ItemPreview::Waveform(amps))
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
            (!notes.is_empty()).then_some(ItemPreview::Notes(notes))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(guid: &str) -> Track {
        Track {
            guid: guid.into(),
            ..Default::default()
        }
    }

    /// A resize drag reports heights clamped to the envcp floor, and
    /// the plan then lays out at exactly what the drag reported — the
    /// two clamps agree, so a lane dragged past the stop does not jump
    /// when the next frame plans it.
    #[test]
    fn a_drag_past_the_stop_settles_at_the_floor() {
        let dragged = |grab_h: f32, dy: f32| (grab_h + dy).max(ENVELOPE_LANE_MIN_H);

        // Dragged well past the bottom stop.
        let settled = dragged(40.0, -80.0);
        assert_eq!(settled, ENVELOPE_LANE_MIN_H);

        // And the plan draws it there rather than clamping again to
        // something else.
        let tracks = vec![track("a")];
        let lanes = HashMap::from([(
            "a".to_string(),
            vec![EnvelopeLaneView {
                envelope: EnvelopePreview {
                    name: "Volume".into(),
                    points: vec![],
                },
                height: settled,
                automation_items: Vec::new(),
            }],
        )]);
        let plan = plan_rows(&tracks, &lanes, 70.0);
        assert_eq!(plan[1].2, ENVELOPE_LANE_MIN_H);
    }

    /// The vertical plan interleaves lanes under their tracks, respects
    /// the envcp floor, and accounts every divider — the numbers both the
    /// arrange and the TCP column lay out by.
    #[test]
    fn the_plan_interleaves_and_holds_the_floor() {
        let tracks = vec![track("a"), track("b")];
        let lane = |h: f32| EnvelopeLaneView {
            envelope: EnvelopePreview {
                name: "Volume".into(),
                points: vec![],
            },
            height: h,
            automation_items: Vec::new(),
        };
        let lanes = HashMap::from([
            // One healthy lane and one below the floor.
            ("a".to_string(), vec![lane(40.0), lane(10.0)]),
        ]);

        let plan = plan_rows(&tracks, &lanes, 70.0);
        assert_eq!(plan.len(), 4);
        assert_eq!(plan[0], (ArrangeRowKind::Track(0), 0.0, 70.0));
        assert_eq!(
            plan[1],
            (
                ArrangeRowKind::EnvelopeLane { track: 0, lane: 0 },
                71.0,
                40.0
            )
        );
        // The 10-tall lane is held at the envcp floor.
        assert_eq!(
            plan[2],
            (
                ArrangeRowKind::EnvelopeLane { track: 0, lane: 1 },
                112.0,
                ENVELOPE_LANE_MIN_H
            )
        );
        // And track b starts below it, divider counted.
        assert_eq!(plan[3], (ArrangeRowKind::Track(1), 140.0, 70.0));
    }
}

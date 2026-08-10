//! The stacked multitrack view — every track at once, on one timeline.
//!
//! Each track gets a horizontal dimension and is drawn in *its own* mode: a
//! vocal as blobs, its reference MIDI as a roll, a guitar as tab, a kit
//! as slices. Time is shared; vertical space is divided.
//!
//! That sharing is the entire feature, so it has to be exact. Two things
//! make it non-trivial:
//!
//! **Tracks do not agree on a time unit.** A pitched-audio document is
//! in analysis frames at the pitch tracker's hop; a percussive one is in
//! frames at the onset hop; a MIDI one is in ticks. Drawing all three
//! against the active track's camera without converting puts them at
//! wildly different scales — and it *looks* plausible, which is worse
//! than looking broken. Everything is converted through seconds.
//!
//! **Row spaces have different extents.** 128 pitch rows, 6 strings, 3
//! bands. Giving each dimension the same rows-per-pixel would leave the kit
//! occupying a twentieth of its dimension. Each dimension instead fits its own
//! content to its own height.

use expression_editor_core::doc::{ExpressionDoc, Note};
use expression_editor_core::rows::RowSpace;
use expression_editor_core::tracks::StackRow;
use expression_editor_core::{Editor, Mode, Viewport};

use dioxus::prelude::*;

use crate::{canvas, theme};

/// One track's dimension in the stack.
pub struct LaneView {
    /// Index into the workspace.
    pub track: usize,
    pub name: String,
    pub mode: Mode,
    /// Top edge and height, in viewport pixels.
    pub y: f64,
    pub h: f64,
    /// The track being edited. Drawn brighter; the only one that takes
    /// gestures.
    pub active: bool,
    /// The track other tracks are aligned against, if any.
    pub reference: bool,
    pub notes: Vec<LaneNote>,
    /// Row dividers inside the dimension, for spaces where a row is a named
    /// thing (strings, bands, drum lanes). Empty for pitch, which has
    /// too many to draw and a keyboard to read instead.
    pub dividers: Vec<f64>,
    /// Labels down the left edge, paired with their y.
    pub labels: Vec<(f64, String)>,
}

/// A note as it appears in a dimension.
pub struct LaneNote {
    pub x: f64,
    pub w: f64,
    pub y: f64,
    pub h: f64,
    pub fill: String,
    /// Slices and drum hits draw as triangles; everything else as bars.
    pub triangle: bool,
}

/// Vertical padding inside a dimension, in pixels, top and bottom.
const LANE_PAD: f64 = 3.0;

/// Rows of headroom added around a dimension's content before fitting.
///
/// Without it a track whose notes are all on one row fits to zero height
/// and draws as a hairline, and a melody that touches its own extremes
/// has notes flush against the dimension edge where they read as clipped.
const FIT_PAD: f64 = 1.0;

/// Lay out every visible track over the viewport.
///
/// `active_boost` and `min_row` are passed through to
/// [`expression_editor_core::tracks::Workspace::stack`].
pub fn lanes(ed: &Editor, active_boost: f32, min_row: f32) -> Vec<LaneView> {
    let rows = ed.tracks.stack(ed.viewport.h as f32, active_boost, min_row);
    rows.iter().filter_map(|row| lane_view(ed, row)).collect()
}

fn lane_view(ed: &Editor, row: &StackRow) -> Option<LaneView> {
    let track = ed.tracks.track(row.track)?;
    let active = row.track == ed.tracks.active();
    // The active track's parked copy is stale by design — the live
    // document is the editor's. `doc_of` refuses to hand out the stale
    // one, which is exactly the guard that keeps a stacked view from
    // drawing a track's previous state.
    let doc = if active {
        &ed.doc
    } else {
        ed.tracks.doc_of(row.track)?
    };

    let y0 = row.y as f64 + LANE_PAD;
    let h = (row.height as f64 - LANE_PAD * 2.0).max(1.0);
    let (lo, hi) = fit(doc, &track.mode);
    let span = (hi - lo).max(1e-6);
    let row_h = h / span;
    // Rows run bottom-up in every space this draws: higher pitch higher,
    // brighter band higher, and a string roll reads like tab with the
    // lowest string at the bottom.
    let y_of = move |r: f64| y0 + h - (r - lo) * row_h;

    let notes = doc
        .notes
        .iter()
        .map(|n| lane_note(ed, doc, n, &doc.row_space, &track.mode, active, y_of, row_h))
        .collect();

    let (dividers, labels) = guides(&doc.row_space, lo, hi, y_of);

    Some(LaneView {
        track: row.track,
        name: track.name.clone(),
        mode: track.mode,
        y: row.y as f64,
        h: row.height as f64,
        active,
        reference: track.reference,
        notes,
        dividers,
        labels,
    })
}

/// The row range a dimension shows: its own content, padded.
fn fit(doc: &ExpressionDoc, mode: &Mode) -> (f64, f64) {
    let (bound_lo, bound_hi) = doc.row_space.bounds();
    // A space with few rows shows all of them — three bands or six
    // strings are the axis, and fitting to content would move a kit's
    // lanes around as hits come and go.
    if !matches!(doc.row_space, RowSpace::Pitch) {
        return (bound_lo as f64 - 0.5, bound_hi as f64 + 0.5);
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for n in &doc.notes {
        lo = lo.min(n.row as f64);
        hi = hi.max(n.row as f64);
    }
    if !lo.is_finite() || !hi.is_finite() {
        // Nothing to fit to. An octave around middle C is a better blank
        // dimension than the full 128 rows, which would draw everything
        // subsequently loaded as a smear at the bottom.
        let centre = 60.0;
        return (centre - 6.0, centre + 6.0);
    }
    // A blob needs room above and below for its pitch excursions; a bar
    // does not.
    let pad = if mode.draws_blobs() {
        FIT_PAD + 1.0
    } else {
        FIT_PAD
    };
    (lo - pad, hi + pad + 1.0)
}

#[allow(clippy::too_many_arguments)]
fn lane_note(
    ed: &Editor,
    doc: &ExpressionDoc,
    n: &Note,
    space: &RowSpace,
    mode: &Mode,
    active: bool,
    y_of: impl Fn(f64) -> f64,
    row_h: f64,
) -> LaneNote {
    let x0 = ed.camera.x(to_editor_time(ed, doc, n.start));
    let x1 = ed.camera.x(to_editor_time(ed, doc, n.end));
    // Slices are often a frame or two long at this zoom. A note that
    // rounds to zero width is a note the eye cannot find, which defeats
    // the purpose of showing the track at all.
    let w = (x1 - x0).max(1.5);

    let base = space
        .row_color(n.row)
        .map(str::to_string)
        .unwrap_or_else(|| theme::pitch_class_color(n.row).to_string());
    let fill = if active {
        base
    } else {
        // Parked lanes are context, not content. Dimming them keeps the
        // dimension you are editing legible when six are on screen.
        format!("{base}80")
    };

    LaneNote {
        x: x0,
        w,
        y: y_of(n.row as f64 + 1.0),
        h: (row_h * 0.9).max(1.0),
        fill,
        triangle: matches!(
            space.note_shape(),
            expression_editor_core::rows::NoteShape::Triangle
        ) || mode.draws_slices(),
    }
}

/// Convert a track's own document time into the editor's units.
///
/// Through seconds, because the two documents need not share a time
/// base: a percussive take is in frames at the onset hop, a pitched one
/// at the pitch hop, a MIDI take in ticks. Drawing them against one
/// camera without this is off by the ratio between the two rates — and
/// it produces a plausible-looking picture, which is the dangerous kind
/// of wrong for a view whose whole job is answering "is this in time".
fn to_editor_time(ed: &Editor, doc: &ExpressionDoc, t: f64) -> f64 {
    let bpm = ed.bpm;
    let from = doc.time_base.units_per_second(bpm);
    let to = ed.doc.time_base.units_per_second(bpm);
    if from.abs() < 1e-9 {
        return t;
    }
    t / from * to
}

/// Dividers and labels for a dimension's row space.
fn guides(
    space: &RowSpace,
    lo: f64,
    hi: f64,
    y_of: impl Fn(f64) -> f64,
) -> (Vec<f64>, Vec<(f64, String)>) {
    // Pitch space has 128 rows and a keyboard of its own; drawing a line
    // per semitone inside a 60 px dimension is a grey block.
    if matches!(space, RowSpace::Pitch) {
        return (Vec::new(), Vec::new());
    }
    let (bound_lo, bound_hi) = space.bounds();
    let mut dividers = Vec::new();
    let mut labels = Vec::new();
    for r in bound_lo..=bound_hi {
        let rf = r as f64;
        if rf < lo - 1.0 || rf > hi + 1.0 {
            continue;
        }
        dividers.push(y_of(rf + 0.5));
        labels.push((y_of(rf), space.row_label(r)));
    }
    (dividers, labels)
}

/// How much taller the dimension being edited is than the rest.
const ACTIVE_BOOST: f32 = 1.8;

/// Shortest a dimension may be, in pixels.
///
/// Enough to click, because clicking a dimension is how you make it the
/// active one — a dimension too small to hit is a track you cannot get back
/// to.
const MIN_LANE: f32 = 22.0;

/// Every track at once, on one timeline.
///
/// Read-only by design. The stack answers "which track needs work" and
/// "is this in time with that"; the roll is where the work happens.
/// Clicking a dimension makes it active, which is the handover between the
/// two — and it means the one gesture the stack does take is the one
/// that gets you out of it.
#[component]
pub fn StackView(editor: Signal<Editor>) -> Element {
    let mut editor = editor;
    let ed = editor.read();
    let vp = ed.viewport;
    let lanes = lanes(&ed, ACTIVE_BOOST, MIN_LANE);
    let ticks = canvas::ruler(&ed);
    drop(ed);

    rsx! {
        svg {
            style: "display: block; width: 100%; height: 100%; \
                    touch-action: none; user-select: none; cursor: pointer;",
            view_box: "0 0 {vp.w + canvas::GUTTER_W:.0} {vp.h + canvas::RULER_H:.0}",
            preserve_aspect_ratio: "none",
            onmounted: move |e| {
                let data = e.data();
                spawn(async move {
                    if let Ok(r) = data.get_client_rect().await {
                        editor.write().resize(Viewport::new(
                            r.width() - canvas::GUTTER_W,
                            r.height() - canvas::RULER_H,
                        ));
                    }
                });
            },
            onpointerdown: move |e: PointerEvent| {
                let c = e.data().element_coordinates();
                let y = c.y - canvas::RULER_H;
                // Resolve against a snapshot: the read guard has to be
                // gone before the write below.
                let hit = {
                    let ed = editor.read();
                    let rows = ed.tracks.stack(ed.viewport.h as f32, ACTIVE_BOOST, MIN_LANE);
                    expression_editor_core::tracks::Workspace::row_at(&rows, y as f32)
                };
                if let Some(track) = hit {
                    editor.write().switch_track(track);
                }
            },

            rect {
                x: 0, y: 0,
                width: "{vp.w + canvas::GUTTER_W}",
                height: "{vp.h + canvas::RULER_H}",
                fill: theme::BG,
            }

            // One ruler for the whole stack — the shared axis is the
            // reason the view exists, so it is drawn once rather than
            // per dimension.
            g {
                transform: "translate({canvas::GUTTER_W}, 0)",
                for t in ticks.iter() {
                    line {
                        x1: "{t.x:.1}", x2: "{t.x:.1}",
                        y1: "{canvas::RULER_H - 6.0}", y2: "{canvas::RULER_H}",
                        stroke: theme::TEXT_DIM, stroke_width: 1,
                    }
                }
            }

            g {
                transform: "translate(0, {canvas::RULER_H})",
                for dimension in lanes.iter() {
                    g {
                        // A dimension's own background, so the active one
                        // reads as the foreground even when a neighbour
                        // is busier.
                        rect {
                            x: 0, y: "{dimension.y:.1}",
                            width: "{vp.w + canvas::GUTTER_W}",
                            height: "{dimension.h:.1}",
                            fill: if dimension.active { theme::ROW_WHITE } else { theme::BG },
                        }
                        line {
                            x1: 0, x2: "{vp.w + canvas::GUTTER_W}",
                            y1: "{dimension.y:.1}", y2: "{dimension.y:.1}",
                            stroke: theme::OCTAVE_LINE, stroke_width: 1,
                        }
                        g {
                            transform: "translate({canvas::GUTTER_W}, 0)",
                            for d in dimension.dividers.iter() {
                                line {
                                    x1: 0, x2: "{vp.w}",
                                    y1: "{d:.1}", y2: "{d:.1}",
                                    stroke: theme::GRID_SUB, stroke_width: 1,
                                }
                            }
                            for n in dimension.notes.iter() {
                                if n.triangle {
                                    polygon {
                                        points: "{n.x:.1},{n.y:.1} {n.x + n.w:.1},{n.y + n.h / 2.0:.1} {n.x:.1},{n.y + n.h:.1}",
                                        fill: "{n.fill}",
                                    }
                                } else {
                                    rect {
                                        x: "{n.x:.1}", y: "{n.y:.1}",
                                        width: "{n.w:.1}", height: "{n.h:.1}",
                                        rx: 1,
                                        fill: "{n.fill}",
                                    }
                                }
                            }
                        }
                        // The name last, so it sits over the material
                        // rather than under it.
                        text {
                            x: 4, y: "{dimension.y + 11.0:.1}",
                            font_size: "9",
                            fill: if dimension.active { theme::TEXT } else { theme::TEXT_DIM },
                            "{dimension.name}"
                        }
                    }
                }
            }
        }
    }
}

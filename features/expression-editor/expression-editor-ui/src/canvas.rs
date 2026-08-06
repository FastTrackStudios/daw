//! Canvas geometry — the SVG the piano roll is drawn from.
//!
//! Pure functions over the editor state, returning ready-to-render
//! primitives. Keeping them out of the rsx means the layout can be
//! asserted in tests without mounting a DOM, and it keeps the component
//! body about events rather than arithmetic.

use expression_editor_core::doc::{Lane, Note, NoteId};
use expression_editor_core::tools;
use expression_editor_core::Editor;

use crate::theme;

/// A piano-roll background row.
pub struct Row {
    pub row: i32,
    pub y: f64,
    pub h: f64,
    pub fill: &'static str,
    pub is_c: bool,
}

/// Background rows across the visible pitch span.
pub fn rows(ed: &Editor) -> Vec<Row> {
    let (lo, hi) = ed.camera.pitch_span(ed.viewport);
    let h = ed.camera.px_per_semitone;
    ((lo.floor() as i32).max(0)..=(hi.ceil() as i32).min(127))
        .map(|row| Row {
            row,
            y: ed.camera.y(row as f64 + 0.5, ed.viewport),
            h,
            fill: if theme::is_black_key(row) {
                theme::ROW_BLACK
            } else {
                theme::ROW_WHITE
            },
            is_c: row.rem_euclid(12) == 0,
        })
        .collect()
}

/// A vertical gridline.
pub struct GridLine {
    pub x: f64,
    pub beat: bool,
}

/// Gridlines at the editor's own local grid — never the project grid.
pub fn grid_lines(ed: &Editor) -> Vec<GridLine> {
    let step = ed.grid.step(ed.units_per_beat());
    let beat = ed.units_per_beat();
    if step <= 0.0 {
        return Vec::new();
    }
    let (t0, t1) = ed.camera.time_span(ed.viewport);
    // Bail out rather than emit thousands of invisible lines when zoomed
    // far out.
    if (t1 - t0) / step > 512.0 {
        return Vec::new();
    }
    let first = ((t0 - ed.doc.start) / step).floor() * step + ed.doc.start;
    let mut out = Vec::new();
    let mut t = first;
    while t <= t1 {
        if t >= t0 {
            let on_beat = ((t - ed.doc.start) / beat).fract().abs() < 1e-6;
            out.push(GridLine {
                x: ed.camera.x(t),
                beat: on_beat,
            });
        }
        t += step;
    }
    out
}

/// A note rectangle ready to draw.
pub struct NoteRect {
    pub id: NoteId,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub fill: &'static str,
    pub opacity: f64,
    pub selected: bool,
    pub ambiguous: bool,
    /// Zone spans in pixels, and whether each is the active target.
    pub zones: Vec<(f64, f64, bool)>,
    /// Sounding detune in cents, when it differs from 12-TET.
    pub cents: Option<f64>,
}

pub fn note_rects(ed: &Editor) -> Vec<NoteRect> {
    let (t0, t1) = ed.camera.time_span(ed.viewport);
    let h = ed.camera.px_per_semitone;
    ed.doc
        .notes
        .iter()
        .filter(|n| n.end >= t0 && n.start <= t1)
        .map(|n| {
            let x = ed.camera.x(n.start);
            let w = (ed.camera.x(n.end) - x).max(2.0);
            let active_zone = match n.target {
                expression_editor_core::Target::WholeNote => None,
                expression_editor_core::Target::Zone(i) => Some(i),
            };
            let zones = n
                .zones()
                .iter()
                .enumerate()
                .map(|(i, &(a, b))| {
                    // Either exactly one segment is highlighted, or
                    // every one is — never a partial state.
                    let active = active_zone.is_none_or(|z| z == i);
                    (ed.camera.x(a), ed.camera.x(b), active)
                })
                .collect();
            let cents = sounding_cents(ed, n);
            NoteRect {
                id: n.id,
                x,
                y: ed.camera.y(n.row as f64 + 0.5, ed.viewport),
                w,
                h,
                fill: theme::pitch_class_color(n.row),
                opacity: 0.35 + 0.45 * n.weight.clamp(0.0, 1.0),
                selected: ed.selection.contains(n.id),
                ambiguous: n.ambiguous,
                zones,
                cents,
            }
        })
        .collect()
}

/// How far the note actually sounds from 12-TET, if it does.
///
/// Read from the pitch curve, not the tuning table: a note is off-ET
/// because of what was drawn or sung, and the badge should follow the
/// audible truth.
fn sounding_cents(ed: &Editor, n: &Note) -> Option<f64> {
    let mid = (n.start + n.end) * 0.5;
    let offset = n.pitch.sample(mid, 0.0);
    let cents = offset * 100.0;
    let _ = ed;
    (cents.abs() > 3.0).then_some(cents)
}

/// A rendered expression curve.
pub struct CurvePath {
    pub note: NoteId,
    pub lane: Lane,
    pub points: String,
    pub color: &'static str,
    pub active: bool,
    pub selected: bool,
}

/// Polyline paths for every visible note on every drawn lane, back to
/// front.
///
/// Pitch uses the full piano-roll height; Pressure and Timbre are
/// confined to a fixed box one semitone above and below the note, so
/// their normalized shape reads the same at every zoom level.
pub fn curve_paths(ed: &Editor) -> Vec<CurvePath> {
    let (t0, t1) = ed.camera.time_span(ed.viewport);
    let mut out = Vec::new();
    for lane in ed.draw_order() {
        let active = lane == ed.lane;
        for n in ed.doc.notes.iter().filter(|n| n.end >= t0 && n.start <= t1) {
            let curve = n.lane(lane);
            if curve.is_empty() {
                continue;
            }
            let mut s = String::new();
            for p in curve.points() {
                let x = ed.camera.x(p.t);
                let y = match lane {
                    Lane::Pitch => ed.camera.y(n.row as f64 + p.value, ed.viewport),
                    _ => tools::lane_box_y(&ed.camera, ed.viewport, n.row, p.value),
                };
                s.push_str(&format!("{x:.1},{y:.1} "));
            }
            out.push(CurvePath {
                note: n.id,
                lane,
                points: s,
                color: theme::lane_color(lane),
                active,
                selected: ed.selection.contains(n.id),
            });
        }
    }
    out
}

/// The editing box Pressure and Timbre are drawn inside.
pub struct LaneBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub fn lane_boxes(ed: &Editor) -> Vec<LaneBox> {
    if ed.lane == Lane::Pitch {
        return Vec::new();
    }
    ed.doc
        .notes
        .iter()
        .filter(|n| ed.selection.contains(n.id))
        .map(|n| {
            let top = ed.camera.y(n.row as f64 + 1.0, ed.viewport);
            let bottom = ed.camera.y(n.row as f64 - 1.0, ed.viewport);
            let x = ed.camera.x(n.start);
            LaneBox {
                x,
                y: top,
                w: (ed.camera.x(n.end) - x).max(2.0),
                h: bottom - top,
            }
        })
        .collect()
}

/// A microtonal center guide: where a row actually sounds under the
/// active temperament.
pub struct TuningGuide {
    pub y: f64,
    pub cents: f64,
    pub label: String,
}

/// Gold center lines for every visible row whose tuned center differs
/// from 12-TET.
pub fn tuning_guides(ed: &Editor) -> Vec<TuningGuide> {
    if ed.tuning.temperament.is_equal() {
        return Vec::new();
    }
    let (lo, hi) = ed.camera.pitch_span(ed.viewport);
    ((lo.floor() as i32).max(0)..=(hi.ceil() as i32).min(127))
        .filter_map(|row| {
            let cents = ed.tuning.cents(row);
            (cents.abs() > 0.5).then(|| TuningGuide {
                y: ed.camera.y(ed.tuning.center(row), ed.viewport),
                cents,
                label: format!(
                    "{} {}{:.0}¢",
                    expression_editor_core::tuning::note_name(row),
                    if cents > 0.0 { "+" } else { "" },
                    cents
                ),
            })
        })
        .collect()
}

/// Effective-pitch guides: a horizontal line across each zone at the
/// pitch its curve actually dwells on.
///
/// This is what a zone scales around — not the note's own row, so a
/// scoop that settles a fourth above its onset still expands about
/// where it lands.
pub struct ZoneGuide {
    pub x0: f64,
    pub x1: f64,
    pub y: f64,
}

pub fn zone_guides(ed: &Editor) -> Vec<ZoneGuide> {
    if ed.lane != Lane::Pitch {
        return Vec::new();
    }
    let mut out = Vec::new();
    for n in ed.doc.notes.iter().filter(|n| ed.selection.contains(n.id)) {
        if n.splits.is_empty() {
            continue;
        }
        for (a, b) in n.zones() {
            let center =
                expression_editor_core::blob::effective_center(&n.pitch, a, b, 48, 0.0);
            out.push(ZoneGuide {
                x0: ed.camera.x(a),
                x1: ed.camera.x(b),
                y: ed.camera.y(n.row as f64 + center, ed.viewport),
            });
        }
    }
    out
}

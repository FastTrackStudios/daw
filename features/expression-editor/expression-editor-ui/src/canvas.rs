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
    pub row: i32,
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
    /// Sounding detune in cents from the row's tuned center.
    pub cents: Option<f64>,
    /// Amplitude ribbon polygon, when Pressure has been authored.
    pub ribbon: Option<String>,
    /// What the body prints — note name, fret number, or lyric.
    pub label: Option<String>,
    /// Articulation badge, drawn above the note.
    pub badge: Option<&'static str>,
    /// Triangle points, when this space draws heads instead of bars.
    pub head: Option<String>,
    /// Joined to the next note on its row.
    pub legato: bool,
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
            // The label comes from the row space: a note name in pitch
            // space, a fret number on a string roll, and a lyric
            // whenever the note carries one.
            let label = ed.row_space.note_label(n).filter(|l| {
                // A fret number always fits; a word may not, and a
                // clipped lyric is worse than none.
                l.len() <= 2 || (w > l.len() as f64 * 6.5 + 8.0 && h >= 12.0)
            });
            let badge = n
                .articulation
                .map(|a| a.glyph())
                .filter(|g| !g.is_empty());
            let head = matches!(
                ed.row_space.note_shape(),
                expression_editor_core::NoteShape::Triangle
            )
            .then(|| {
                // Flat edge on the onset, apex to the right: the attack
                // is a straight vertical line you can align to the grid
                // by eye, which is the whole reason for a head over a
                // diamond.
                let size = h.min(18.0).max(5.0);
                let top = ed.camera.y(n.row as f64 + 0.5, ed.viewport) + (h - size) * 0.5;
                format!(
                    "{x:.1},{top:.1} {x:.1},{:.1} {:.1},{:.1}",
                    top + size,
                    x + size * 0.9,
                    top + size * 0.5,
                )
            });
            // A string roll colours by string, a kit by section; pitch
            // space keeps its pitch-class hue.
            let fill = ed
                .row_space
                .row_color(n.row)
                .unwrap_or_else(|| theme::pitch_class_color(n.row));
            NoteRect {
                id: n.id,
                row: n.row,
                x,
                y: ed.camera.y(n.row as f64 + 0.5, ed.viewport),
                w,
                h,
                fill,
                opacity: 0.35 + 0.45 * n.weight.clamp(0.0, 1.0),
                selected: ed.selection.contains(n.id),
                ambiguous: n.ambiguous,
                zones,
                cents,
                ribbon: note_ribbon(ed, n),
                label,
                badge,
                head,
                legato: n.legato,
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
    // Only where it is being asked about: the selected note always, and
    // otherwise only when a non-equal tuning makes the row's own center
    // ambiguous. A badge on every note is a wall of numbers.
    if !ed.selection.contains(n.id) && ed.tuning.temperament.is_equal() {
        return None;
    }
    // Measured against the row's tuned center, not raw 12-TET, so under
    // a temperament the badge reads "off the target" rather than
    // restating the temperament.
    let center_offset = ed.tuning.cents(n.row) / 100.0;
    let mid = (n.start + n.end) * 0.5;
    let cents = (n.pitch.sample(mid, 0.0) - center_offset) * 100.0;
    (cents.abs() > 3.0).then_some(cents)
}

/// The note body's amplitude ribbon: the Pressure curve drawn as a
/// filled shape inside the note rectangle.
///
/// This is what makes a note read as a *blob* rather than a bar — you
/// can see where it swells and where it dies without switching lanes.
pub fn note_ribbon(ed: &Editor, n: &Note) -> Option<String> {
    if n.pressure.is_empty() {
        return None;
    }
    let top = ed.camera.y(n.row as f64 + 0.5, ed.viewport);
    let h = ed.camera.px_per_semitone;
    if h < 6.0 {
        return None;
    }
    let mid = top + h * 0.5;
    let pts: Vec<(f64, f64)> = n
        .pressure
        .points()
        .iter()
        .map(|p| (ed.camera.x(p.t), p.value.clamp(0.0, 1.0)))
        .collect();
    if pts.len() < 2 {
        return None;
    }
    // Symmetric about the note's centre line: out along the top, back
    // along the bottom, closed.
    let mut s = String::new();
    for &(x, v) in &pts {
        s.push_str(&format!("{:.1},{:.1} ", x, mid - h * 0.46 * v));
    }
    for &(x, v) in pts.iter().rev() {
        s.push_str(&format!("{:.1},{:.1} ", x, mid + h * 0.46 * v));
    }
    Some(s)
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

// ── chrome: keyboard gutter and timeline ruler ───────────────────────
//
// Both live inside the same SVG as the roll, drawn in the margins the
// roll is translated away from. One element, one coordinate system, one
// set of pointer handlers — a separate scrolling keyboard element would
// have to be kept in sync with the camera every frame.

/// Width of the piano-key gutter on the left.
pub const GUTTER_W: f64 = 54.0;
/// Height of the timeline ruler on top.
pub const RULER_H: f64 = 28.0;

/// One key in the gutter.
pub struct Key {
    pub row: i32,
    pub y: f64,
    pub h: f64,
    pub black: bool,
    /// Only C rows are labelled, so the gutter stays readable when the
    /// rows get short.
    pub label: Option<String>,
}

pub fn keyboard(ed: &Editor) -> Vec<Key> {
    let (lo, hi) = ed.camera.pitch_span(ed.viewport);
    let h = ed.camera.px_per_semitone;
    let (rlo, rhi) = ed.row_space.bounds();
    // Named rows always carry their label — a drum lane called nothing
    // is unusable, where an unlabelled piano key can still be counted.
    let named = !matches!(ed.row_space, expression_editor_core::RowSpace::Pitch);
    let label_rows = h >= 8.0;
    ((lo.floor() as i32).max(rlo)..=(hi.ceil() as i32).min(rhi))
        .map(|row| Key {
            row,
            y: ed.camera.y(row as f64 + 0.5, ed.viewport),
            h,
            black: ed.row_space.is_accidental(row),
            label: (label_rows && (named || row.rem_euclid(12) == 0 || h >= 18.0))
                .then(|| ed.row_space.row_label(row)),
        })
        .collect()
}

/// A ruler tick.
pub struct Tick {
    pub x: f64,
    /// Bar starts get a full-height line and a number.
    pub bar: bool,
    pub label: Option<String>,
}

pub fn ruler(ed: &Editor) -> Vec<Tick> {
    let (t0, t1) = ed.camera.time_span(ed.viewport);
    let beat = ed.units_per_beat();
    let bar = ed.units_per_bar();
    // Label bars only while they are far enough apart to read; beat
    // ticks disappear entirely once they would be a grey smear.
    let show_beats = beat / ed.camera.units_per_px >= 14.0;
    let step = if show_beats { beat } else { bar };
    if step <= 0.0 || (t1 - t0) / step > 400.0 {
        return Vec::new();
    }
    let first = ((t0 - ed.doc.start) / step).floor() * step + ed.doc.start;
    let label_bars = bar / ed.camera.units_per_px >= 40.0;
    let mut out = Vec::new();
    let mut t = first;
    while t <= t1 {
        if t >= t0 {
            let (b, beat_n) = ed.bar_beat(t + step * 0.001);
            let is_bar = beat_n == 1;
            out.push(Tick {
                x: ed.camera.x(t),
                bar: is_bar,
                label: (is_bar && label_bars).then(|| b.to_string()),
            });
        }
        t += step;
    }
    out
}

/// Marker flags the host supplied.
pub struct MarkerFlag {
    pub x: f64,
    pub label: String,
}

pub fn markers(ed: &Editor) -> Vec<MarkerFlag> {
    let (t0, t1) = ed.camera.time_span(ed.viewport);
    ed.doc
        .markers
        .iter()
        .filter(|m| m.t >= t0 && m.t <= t1)
        .map(|m| MarkerFlag {
            x: ed.camera.x(m.t),
            label: m.label.clone().unwrap_or_default(),
        })
        .collect()
}


/// A razor area in pixels.
pub struct RazorRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Razor areas, clipped to the visible span.
///
/// Drawn as a filled rectangle with hard vertical edges: the edges are
/// where notes get sliced, so they have to read as exact boundaries
/// rather than as a soft highlight.
pub fn razor_rects(ed: &Editor) -> Vec<RazorRect> {
    let h = ed.camera.px_per_semitone;
    ed.razor
        .areas
        .iter()
        .map(|a| {
            let x = ed.camera.x(a.t0);
            let top = ed.camera.y(a.row_hi as f64 + 0.5, ed.viewport);
            RazorRect {
                x,
                y: top,
                w: (ed.camera.x(a.t1) - x).max(1.0),
                h: h * a.rows() as f64,
            }
        })
        .collect()
}

// ── velocity / CC lane strip ─────────────────────────────────────────

/// One note's velocity stem in the strip.
pub struct Stem {
    pub note: NoteId,
    pub x: f64,
    /// Bar width — narrow, so dense passages stay countable.
    pub w: f64,
    pub y: f64,
    pub h: f64,
    pub color: &'static str,
    pub selected: bool,
    pub muted: bool,
}

/// Velocity stems for the visible notes, in a strip `h` pixels tall.
///
/// Stems are drawn at the note's *onset*, not across its length: what
/// is being edited is a single value per note, and a full-width bar
/// invites dragging a duration that does not exist here.
pub fn stems(ed: &Editor, h: f64) -> Vec<Stem> {
    use expression_editor_core::StripLane;
    let (t0, t1) = ed.camera.time_span(ed.viewport);
    let per_note = matches!(
        ed.strip_lane,
        StripLane::Velocity | StripLane::OffVelocity
    );
    if !per_note {
        return Vec::new();
    }
    ed.doc
        .notes
        .iter()
        .filter(|n| n.end >= t0 && n.start <= t1)
        .map(|n| {
            let v = match ed.strip_lane {
                StripLane::OffVelocity => n.off_velocity,
                _ => n.velocity,
            }
            .clamp(0.0, 1.0);
            let bar = (ed.camera.x(n.end) - ed.camera.x(n.start)).clamp(3.0, 9.0);
            Stem {
                note: n.id,
                x: ed.camera.x(n.start),
                w: bar,
                y: h * (1.0 - v),
                h: h * v,
                color: theme::pitch_class_color(n.row),
                selected: ed.selection.contains(n.id),
                muted: n.muted,
            }
        })
        .collect()
}

/// The strip's continuous-lane curve, when it is showing one.
///
/// Every visible note's curve is drawn end to end, normalized into the
/// strip — so a Pressure sweep reads as one gesture across the phrase
/// rather than as a set of disconnected per-note boxes.
pub fn strip_curves(ed: &Editor, h: f64) -> Vec<CurvePath> {
    use expression_editor_core::StripLane;
    let StripLane::Expression(lane) = ed.strip_lane else {
        return Vec::new();
    };
    let (t0, t1) = ed.camera.time_span(ed.viewport);
    let (lo, hi) = match lane {
        Lane::Pitch => (-2.0, 2.0),
        _ => (0.0, 1.0),
    };
    ed.doc
        .notes
        .iter()
        .filter(|n| n.end >= t0 && n.start <= t1 && !n.lane(lane).is_empty())
        .map(|n| {
            let mut s = String::new();
            for p in n.lane(lane).points() {
                let y = h * (1.0 - ((p.value - lo) / (hi - lo)).clamp(0.0, 1.0));
                s.push_str(&format!("{:.1},{y:.1} ", ed.camera.x(p.t)));
            }
            CurvePath {
                note: n.id,
                lane,
                points: s,
                color: theme::lane_color(lane),
                active: true,
                selected: ed.selection.contains(n.id),
            }
        })
        .collect()
}

/// Horizontal reference lines in the strip, at eighths of full scale.
pub fn strip_guides(h: f64) -> Vec<(f64, bool)> {
    // Only quarter, half and three-quarter get a visible line; more
    // would read as noise behind the stems.
    [0.25, 0.5, 0.75]
        .iter()
        .map(|f| (h * (1.0 - f), (*f - 0.5).abs() < 1e-9))
        .collect()
}

// ── pinned controller lanes ──────────────────────────────────────────

/// A controller lane ready to draw behind the roll.
pub struct CcPath {
    pub number: u8,
    pub label: String,
    pub color: &'static str,
    /// The line.
    pub points: String,
    /// The same path closed to the bottom, for the fill.
    pub fill: String,
    pub opacity: f64,
    pub active: bool,
}

/// Pinned lanes, spanning the full roll height.
///
/// The curve is sampled at the visible edges as well as at its own
/// points, so a lane whose last authored value is off-screen still
/// draws across the whole view instead of stopping mid-canvas.
pub fn cc_paths(ed: &Editor) -> Vec<CcPath> {
    let (t0, t1) = ed.camera.time_span(ed.viewport);
    let h = ed.viewport.h;
    let d = ed.cc_display;

    ed.doc
        .cc
        .pinned()
        .map(|lane| {
            let active = ed.cc_edit == Some(lane.number);
            let default = lane.default_value();

            let mut ts: Vec<f64> = vec![t0];
            ts.extend(
                lane.curve
                    .points()
                    .iter()
                    .map(|p| p.t)
                    .filter(|t| *t > t0 && *t < t1),
            );
            ts.push(t1);

            let mut line = String::new();
            for &t in &ts {
                let v = lane.curve.sample(t, default);
                line.push_str(&format!(
                    "{:.1},{:.1} ",
                    ed.camera.x(t),
                    expression_editor_core::cc::cc_y(v, h)
                ));
            }
            // Close down to the baseline and back, so the fill is a
            // shape rather than a self-intersecting ribbon.
            let fill = format!(
                "{:.1},{:.1} {line}{:.1},{:.1}",
                ed.camera.x(t0),
                h,
                ed.camera.x(t1),
                h
            );

            CcPath {
                number: lane.number,
                label: lane.label(),
                color: expression_editor_core::cc::CC_COLORS
                    [lane.color % expression_editor_core::cc::CC_COLORS.len()],
                points: line,
                fill,
                opacity: if active {
                    d.active_opacity
                } else {
                    d.background_opacity
                },
                active,
            }
        })
        .collect()
}

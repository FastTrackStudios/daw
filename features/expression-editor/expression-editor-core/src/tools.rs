//! Tools, selection, and hit testing.
//!
//! Hit testing is shared by every tool on purpose. The rule that makes
//! the canvas predictable is that a note's full top-to-bottom rectangle
//! belongs to that note's pitch row, and clicking an existing note
//! always selects it rather than creating something underneath — so
//! Select, Curve, and Note Draw all agree about what is under the
//! pointer.

use crate::camera::{Camera, Viewport};
use crate::doc::{ExpressionDoc, Dimension, NoteId, Target};

/// The active drawing tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Tool {
    Select,
    Pen,
    #[default]
    Curve,
    Eraser,
    NoteDraw,
    NoteErase,
}

impl Tool {
    /// The gestures this tool takes over from the base map.
    ///
    /// Plain (unmodified) gestures only. A tool claiming
    /// `Ctrl+drag` would silently take the pen override, or the razor,
    /// or whatever the user had bound there — the modified gestures are
    /// the map's and stay the map's.
    ///
    /// Empty means "the base map already does the right thing", which is
    /// true of Select: a plain drag already marquees on the roll and
    /// moves a note.
    pub fn claims(self) -> &'static [(crate::mouse::Context, crate::mouse::Gesture)] {
        use crate::mouse::{Context as C, Gesture as G};
        match self {
            // The base map is already this tool.
            Tool::Select => &[],
            // The expression tools want the roll *and* notes: a freehand
            // stroke that stopped at the edge of a note would be useless.
            Tool::Pen | Tool::Curve | Tool::Eraser => {
                &[(C::PianoRoll, G::Drag), (C::Note, G::Drag)]
            }
            // Drawing owns empty roll; a drag that starts on a note still
            // moves it, which is what every DAW does with a pencil.
            Tool::NoteDraw => &[(C::PianoRoll, G::Drag)],
            // Erasing owns both, or sweeping across notes would move the
            // first one it touched instead of deleting it.
            Tool::NoteErase => &[(C::PianoRoll, G::Drag), (C::Note, G::Drag)],
        }
    }

    pub const ALL: [Tool; 6] = [
        Tool::Select,
        Tool::Pen,
        Tool::Curve,
        Tool::Eraser,
        Tool::NoteDraw,
        Tool::NoteErase,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Pen => "Pen",
            Tool::Curve => "Curve",
            Tool::Eraser => "Eraser",
            Tool::NoteDraw => "Note Draw",
            Tool::NoteErase => "Note Erase",
        }
    }

    /// Tools that author expression rather than notes.
    pub fn edits_expression(&self) -> bool {
        matches!(self, Tool::Pen | Tool::Curve | Tool::Eraser)
    }

    pub fn edits_notes(&self) -> bool {
        matches!(self, Tool::NoteDraw | Tool::NoteErase)
    }
}

/// Modifier keys, normalized across platforms (`cmd` on macOS is
/// reported as `ctrl`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// What the pointer is over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    /// A note body. `zone` is the zone under the pointer.
    Note { id: NoteId, zone: usize },
    /// A note edge, within the resize handle width.
    NoteEdge { id: NoteId, start_edge: bool },
    /// A Q-zone split handle in the bottom strip or note body.
    ZoneSplit { id: NoteId, index: usize, t: f64 },
    /// A point on the active dimension's curve.
    CurvePoint { id: NoteId, dimension: Dimension, t: f64 },
    /// Empty canvas at this time and pitch.
    Empty { t: f64, pitch: f64 },
}

/// Pixel tolerances for hit testing.
#[derive(Clone, Copy, Debug)]
pub struct HitConfig {
    pub edge_px: f64,
    pub split_px: f64,
    pub point_px: f64,
}

impl Default for HitConfig {
    fn default() -> Self {
        Self {
            edge_px: 6.0,
            split_px: 8.0,
            point_px: 5.0,
        }
    }
}

/// Resolve what is under `(x, y)`.
///
/// Order is deliberate: split handles beat note edges, which beat note
/// bodies, which beat curve points. Handles are small and intentional;
/// bodies are large and easy to hit by accident.
pub fn hit_test(
    doc: &ExpressionDoc,
    camera: &Camera,
    vp: Viewport,
    dimension: Dimension,
    x: f64,
    y: f64,
    cfg: HitConfig,
) -> Hit {
    let t = camera.t_at(x);
    let pitch = camera.pitch_at(y, vp);
    let row = pitch.round() as i32;

    for n in &doc.notes {
        for (i, &s) in n.splits.iter().enumerate() {
            if (camera.x(s) - x).abs() <= cfg.split_px && n.row == row {
                return Hit::ZoneSplit {
                    id: n.id,
                    index: i,
                    t: s,
                };
            }
        }
    }

    for n in &doc.notes {
        if n.row != row {
            continue;
        }
        if (camera.x(n.start) - x).abs() <= cfg.edge_px {
            return Hit::NoteEdge {
                id: n.id,
                start_edge: true,
            };
        }
        if (camera.x(n.end) - x).abs() <= cfg.edge_px {
            return Hit::NoteEdge {
                id: n.id,
                start_edge: false,
            };
        }
    }

    for n in &doc.notes {
        // The full row height belongs to the note, so a pointer
        // anywhere in the row band selects it.
        if n.row == row && t >= n.start && t <= n.end {
            return Hit::Note {
                id: n.id,
                zone: n.zone_at(t),
            };
        }
    }

    for n in &doc.notes {
        if t < n.start || t > n.end {
            continue;
        }
        let curve = n.curve(dimension);
        for p in curve.points() {
            let py = match dimension {
                Dimension::Pitch => camera.y(n.row as f64 + p.value, vp),
                _ => lane_box_y(camera, vp, n.row, p.value),
            };
            if (camera.x(p.t) - x).abs() <= cfg.point_px && (py - y).abs() <= cfg.point_px {
                return Hit::CurvePoint {
                    id: n.id,
                    dimension,
                    t: p.t,
                };
            }
        }
    }

    Hit::Empty { t, pitch }
}

/// Pressure and Timbre draw inside a fixed box one semitone above and
/// below the note's row, at every zoom level — so a normalized gesture
/// keeps the same shape whether you are zoomed to a bar or to a beat.
pub fn lane_box_y(camera: &Camera, vp: Viewport, row: i32, normalized: f64) -> f64 {
    let top = camera.y(row as f64 + 1.0, vp);
    let bottom = camera.y(row as f64 - 1.0, vp);
    top + (bottom - top) * (1.0 - normalized.clamp(0.0, 1.0))
}

/// Inverse of [`lane_box_y`].
pub fn lane_box_value(camera: &Camera, vp: Viewport, row: i32, y: f64) -> f64 {
    let top = camera.y(row as f64 + 1.0, vp);
    let bottom = camera.y(row as f64 - 1.0, vp);
    if (bottom - top).abs() < 1e-9 {
        return 0.5;
    }
    (1.0 - (y - top) / (bottom - top)).clamp(0.0, 1.0)
}

/// Selected notes and, within them, selected curve points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    pub notes: Vec<NoteId>,
    /// `(note, dimension, t)` of individually selected points.
    pub points: Vec<(NoteId, Dimension, f64)>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.points.is_empty()
    }

    pub fn clear(&mut self) {
        self.notes.clear();
        self.points.clear();
    }

    pub fn contains(&self, id: NoteId) -> bool {
        self.notes.contains(&id)
    }

    pub fn set_single(&mut self, id: NoteId) {
        self.notes.clear();
        self.points.clear();
        self.notes.push(id);
    }

    pub fn toggle(&mut self, id: NoteId) {
        match self.notes.iter().position(|&n| n == id) {
            Some(i) => {
                self.notes.remove(i);
            }
            None => self.notes.push(id),
        }
    }

    pub fn add(&mut self, id: NoteId) {
        if !self.contains(id) {
            self.notes.push(id);
        }
    }

    /// Marquee select every note intersecting the rectangle.
    pub fn marquee(
        &mut self,
        doc: &ExpressionDoc,
        camera: &Camera,
        vp: Viewport,
        (x0, y0): (f64, f64),
        (x1, y1): (f64, f64),
        additive: bool,
    ) {
        if !additive {
            self.clear();
        }
        let (t0, t1) = min_max(camera.t_at(x0), camera.t_at(x1));
        let (p0, p1) = min_max(camera.pitch_at(y0, vp), camera.pitch_at(y1, vp));
        for n in &doc.notes {
            let row = n.row as f64;
            if n.start <= t1 && n.end >= t0 && row >= p0 - 0.5 && row <= p1 + 0.5 {
                self.add(n.id);
            }
        }
    }
}

fn min_max(a: f64, b: f64) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Clicking a note's active zone toggles between targeting that one
/// zone and targeting the whole note.
///
/// The visual contract: either exactly one segment is highlighted, or
/// every segment is.
pub fn toggle_target(current: Target, clicked_zone: usize) -> Target {
    match current {
        Target::Zone(i) if i == clicked_zone => Target::WholeNote,
        _ => Target::Zone(clicked_zone),
    }
}

/// Which notes a gesture writes to.
///
/// A drag that starts over an unselected note while others are selected
/// still belongs to the selection — the note under the pointer is only
/// selected if the gesture turns out to be a click.
pub fn gesture_targets(selection: &Selection, under_pointer: Option<NoteId>) -> Vec<NoteId> {
    if !selection.notes.is_empty() {
        return selection.notes.clone();
    }
    under_pointer.into_iter().collect()
}

/// Directional clamping for a gesture that begins outside the target
/// span.
///
/// Starting to the left extends only to the left boundary; the right
/// side of the gesture is left alone, and vice versa. Starting *above*
/// or *below* is still an ordinary interval edit — only horizontal
/// overshoot clamps.
pub fn clamp_gesture(span: (f64, f64), start_t: f64, t0: f64, t1: f64) -> (f64, f64) {
    let (lo, hi) = span;
    let (mut a, mut b) = min_max(t0, t1);
    if start_t < lo {
        a = lo;
        b = b.min(hi);
    } else if start_t > hi {
        b = hi;
        a = a.max(lo);
    } else {
        a = a.max(lo);
        b = b.min(hi);
    }
    (a, b)
}

/// Local grid, private to the editor — it never touches the project or
/// MIDI-editor grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// Division as a fraction of a whole note (0.25 = quarter).
    pub division: f64,
    pub triplet: bool,
    pub enabled: bool,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            division: 1.0 / 16.0,
            triplet: false,
            enabled: true,
        }
    }
}

impl Grid {
    /// Grid step in document units.
    pub fn step(&self, units_per_beat: f64) -> f64 {
        let beats = self.division * 4.0;
        let beats = if self.triplet {
            beats * 2.0 / 3.0
        } else {
            beats
        };
        beats * units_per_beat
    }

    pub fn snap(&self, t: f64, origin: f64, units_per_beat: f64) -> f64 {
        if !self.enabled {
            return t;
        }
        let step = self.step(units_per_beat);
        if step <= 0.0 {
            return t;
        }
        origin + ((t - origin) / step).round() * step
    }

    /// `1/16` / `1/16T`, for the fixed-width toolbar readout.
    pub fn label(&self) -> String {
        let denom = (1.0 / self.division).round() as i64;
        if self.triplet {
            format!("1/{denom}T")
        } else {
            format!("1/{denom}")
        }
    }

    pub fn coarser(&mut self) {
        self.division = (self.division * 2.0).min(1.0);
    }

    pub fn finer(&mut self) {
        self.division = (self.division / 2.0).max(1.0 / 128.0);
    }
}

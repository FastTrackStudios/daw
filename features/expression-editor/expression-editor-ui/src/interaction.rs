//! Drag state and the pointer/keyboard logic that drives it.
//!
//! Kept out of the rsx so it stays readable and testable: every handler
//! here is a plain function over `&mut Editor`, and the component just
//! routes events into them.

use expression_editor_core::doc::{Dimension, NoteId, Point, Target};
use expression_editor_core::edit::Edit;
use expression_editor_core::handles::{self, Handle};
use expression_editor_core::menu::Command;
use expression_editor_core::mouse::{Action, Context, Gesture};
use expression_editor_core::razor::RazorArea;
use expression_editor_core::tools::{self, Hit, Mods};
use expression_editor_core::zoom::ZoomModes;
use expression_editor_core::{Editor, Mode, Shape, Tool};

/// What the pointer is currently doing. `None` between gestures.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Drag {
    #[default]
    None,
    /// Right-drag or ctrl+shift-drag.
    Pan {
        last: (f64, f64),
    },
    Marquee {
        origin: (f64, f64),
        current: (f64, f64),
        additive: bool,
    },
    /// Freehand: samples accumulate at pointer resolution and commit
    /// continuously, so the stroke feels like spraypaint rather than
    /// appearing on release.
    Pen {
        notes: Vec<NoteId>,
        start: (f64, f64),
        samples: Vec<(f64, f64)>,
    },
    /// A shaped ramp between two points; stays "live" after release so
    /// the shape buttons can restyle it.
    Curve {
        notes: Vec<NoteId>,
        start: (f64, f64),
        current: (f64, f64),
    },
    Erase {
        notes: Vec<NoteId>,
        start_t: f64,
        current_t: f64,
    },
    /// Shift-drag: vertical transposition against tuning targets.
    Transpose {
        notes: Vec<NoteId>,
        origin: (f64, f64),
        base_rows: Vec<i32>,
        applied: i32,
    },
    /// Note Draw / Select: two-axis note movement, both axes fluid
    /// through the same drag.
    MoveNotes {
        notes: Vec<NoteId>,
        origin: (f64, f64),
        applied_rows: i32,
        applied_time: f64,
    },
    /// Alt-drag: scale and invert expression about the effective
    /// center.
    Scale {
        notes: Vec<NoteId>,
        origin: (f64, f64),
        applied: f64,
    },
    Resize {
        note: NoteId,
        start_edge: bool,
        original: (f64, f64),
    },
    SplitDrag {
        note: NoteId,
        from: f64,
    },
    NoteErase,
    /// Dragging out a new razor rectangle.
    RazorCreate {
        origin: (f64, f64),
        current: (f64, f64),
    },
    /// Moving or copying an existing area's contents.
    RazorDrag {
        area: RazorArea,
        index: usize,
        origin: (f64, f64),
        copy: bool,
        applied_t: f64,
        applied_rows: i32,
    },
    /// Painting notes along a swept path.
    Paint {
        last_cell: Option<(i64, i32)>,
        snap: bool,
    },
    /// Freehand drawing into a pinned controller lane.
    CcPen {
        number: u8,
        /// Previous sample, so a fast drag interpolates instead of
        /// leaving a staircase.
        last: Option<(f64, f64)>,
    },
    /// A shaped ramp across a controller lane. Like [`Drag::Curve`] it
    /// survives release, so the toolbar's shape buttons restyle the
    /// stroke you just drew instead of the one before it.
    CcLine {
        number: u8,
        start: (f64, f64),
        current: (f64, f64),
    },
    /// Sweeping a controller lane back to its default.
    CcErase {
        number: u8,
        start_t: f64,
        current_t: f64,
    },
    /// Vertical drag scales the swept range's depth about a pivot.
    ///
    /// Rebuilt from `base` on every move rather than scaling the live
    /// curve, because `apply_live` writes straight through and a chain
    /// of live scales would compound into an exponential.
    CcScale {
        number: u8,
        t0: f64,
        t1: f64,
        /// The dimension's points as they were when the gesture opened.
        base: Vec<Point>,
        /// Widest range the drag has covered, so shrinking it back
        /// restores what the wider sweep had already scaled.
        spanned: (f64, f64),
        /// The value the range is scaled about — its own mean, so a
        /// factor of 0 flattens it to where it already sat rather than
        /// slamming it to zero.
        pivot: f64,
        origin_y: f64,
    },
    /// Dragging a pitch-drawing anchor.
    DraftAnchor {
        index: usize,
    },
    /// Dragging a timing separator. The law is captured at press, from
    /// where on the line the grab landed — reading it live would let the
    /// gesture change meaning halfway through.
    Separator {
        sep: expression_editor_core::Separator,
        law: expression_editor_core::StretchLaw,
    },
    /// One of the seven note handles.
    Handle(Box<handles::HandleDrag>),
    /// Dragging out a temporary note: a range inside one note that the
    /// handles will then address.
    TempNote {
        note: NoteId,
        origin_t: f64,
        current_t: f64,
    },
    /// Not a drag at all — the signal that the surface should open a
    /// context menu here. It rides `Drag` because that is already the
    /// one value `pointer_down` hands back to the component, and a
    /// second return channel for one gesture is not worth the churn.
    ContextMenu {
        x: f64,
        y: f64,
        under: Option<NoteId>,
        t: f64,
    },
    /// Vertical drag over notes edits velocity.
    Velocity {
        notes: Vec<NoteId>,
        origin_y: f64,
        fine: bool,
        applied: f64,
    },
}

impl Drag {
    pub fn is_active(&self) -> bool {
        !matches!(self, Drag::None)
    }

    /// The gesture that shape buttons target first — a Curve stroke
    /// stays selected after release.
    pub fn live_curve(&self) -> Option<(&[NoteId], f64, f64)> {
        match self {
            Drag::Curve {
                notes,
                start,
                current,
            } => Some((notes, start.0, current.0)),
            _ => None,
        }
    }

    /// The controller ramp the shape buttons target, same contract as
    /// [`Drag::live_curve`] but for a CC dimension.
    pub fn live_cc_line(&self) -> Option<(u8, f64, f64)> {
        match self {
            Drag::CcLine {
                number,
                start,
                current,
            } => Some((*number, start.0, current.0)),
            _ => None,
        }
    }
}

/// How close to the drawn controller curve counts as grabbing it.
const CC_EVENT_PX: f64 = 5.0;

/// Which mouse-modifier context `(x, y)` falls in.
///
/// CC edit mode outranks everything: while it is on, the roll *is* that
/// controller's dimension, so the razor and the notes behind it are not what
/// the pointer is addressing.
///
/// Razor areas outrank notes: once you have drawn a rectangle, dragging
/// inside it must operate on the region, not on whatever note happens
/// to be under the pointer.
pub fn context_at(ed: &Editor, x: f64, y: f64) -> Context {
    let t = ed.camera.t_at(x);
    let row = ed.camera.pitch_at(y, ed.viewport).round() as i32;

    if let Some(number) = ed.cc_edit {
        // Near the existing curve is `CcEvent`, open dimension is `CcLane` —
        // the same distinction REAPER draws, so a mouse map written for
        // REAPER lands on the right binding without translation.
        let on_curve = ed
            .doc
            .cc
            .get(number)
            .map(|l| {
                let v = l.curve.sample(t, l.default_value());
                (expression_editor_core::cc::cc_y(v, ed.viewport.h) - y).abs() <= CC_EVENT_PX
            })
            .unwrap_or(false);
        return if on_curve {
            Context::CcEvent
        } else {
            Context::CcLane
        };
    }

    if let Some((_, area)) = ed.razor.at(t, row) {
        let edge_px = 6.0;
        if (ed.camera.x(area.t0) - x).abs() <= edge_px
            || (ed.camera.x(area.t1) - x).abs() <= edge_px
        {
            return Context::RazorEdge;
        }
        return Context::RazorArea;
    }
    match ed.hit_test(x, y) {
        Hit::ZoneSplit { .. } => Context::ZoneSplit,
        Hit::NoteEdge { .. } => Context::NoteEdge,
        Hit::Note { .. } | Hit::CurvePoint { .. } => Context::Note,
        Hit::Empty { .. } => Context::PianoRoll,
    }
}

/// Begin a gesture at element coordinates `(x, y)`.
///
/// Resolves the binding through [`Editor::mouse`] and then executes it.
/// This function decides *nothing* about policy — which modifier does
/// what lives in the map, so the drum, guitar and Melodyne editors can
/// disagree without forking this code.
pub fn pointer_down(ed: &mut Editor, x: f64, y: f64, mods: Mods, button: u16) -> Drag {
    let gesture = match button {
        2 => Gesture::RightClick,
        1 => Gesture::MiddleClick,
        _ => Gesture::Drag,
    };

    // Timing separators outrank the notes they sit between: in timing
    // mode the boundary is what the pointer is addressing.
    if let Some(drag) = separator_press(ed, x, y, gesture) {
        return drag;
    }

    // The note handles sit in front of everything, because they are
    // drawn in front of everything: a press that visibly lands on a
    // handle must not fall through to the note or the roll behind it.
    // Right-click is the exception the manual calls out — on the
    // amplitude handle it mutes rather than dragging.
    if let Some(drag) = handle_press(ed, x, y, mods, gesture) {
        return drag;
    }

    let context = context_at(ed, x, y);
    let action = ed.mouse.resolve(context, gesture, mods);
    if action != Action::None {
        if let Some(drag) = run_action(ed, action, context, x, y, mods) {
            return drag;
        }
    }
    legacy_pointer_down(ed, x, y, mods, button)
}

/// Execute a resolved action, returning the drag it opens.
///
/// `None` means "the map had nothing useful here" and the caller falls
/// through to the tool-driven path, which still owns the expression
/// tools (pen, curve, eraser).
fn run_action(
    ed: &mut Editor,
    action: Action,
    context: Context,
    x: f64,
    y: f64,
    mods: Mods,
) -> Option<Drag> {
    let t = ed.camera.t_at(x);
    let row = ed.camera.pitch_at(y, ed.viewport).round() as i32;
    let under = match ed.hit_test(x, y) {
        Hit::Note { id, .. } | Hit::NoteEdge { id, .. } => Some(id),
        _ => None,
    };
    if action.is_edit() {
        ed.begin_gesture();
    }

    match action {
        Action::Pan => Some(Drag::Pan { last: (x, y) }),

        // ── razor ────────────────────────────────────────────────────
        Action::RazorCreate => Some(Drag::RazorCreate {
            origin: (x, y),
            current: (x, y),
        }),
        Action::RazorMoveContents | Action::RazorMoveContentsNoSnap | Action::RazorCopyContents => {
            let (index, area) = ed.razor.at(t, row)?;
            Some(Drag::RazorDrag {
                area,
                index,
                origin: (x, y),
                copy: action == Action::RazorCopyContents,
                applied_t: 0.0,
                applied_rows: 0,
            })
        }
        Action::RazorRemoveArea => {
            ed.razor.remove_at(t, row);
            Some(Drag::None)
        }
        Action::RazorDeleteContents => {
            let (_, area) = ed.razor.at(t, row)?;
            expression_editor_core::razor::delete_contents(&mut ed.doc, area);
            Some(Drag::None)
        }
        Action::RazorClearAll => {
            ed.razor.clear();
            Some(Drag::None)
        }

        // ── notes ────────────────────────────────────────────────────
        Action::SelectNote => {
            let id = under?;
            ed.selection.set_single(id);
            Some(Drag::None)
        }
        Action::AddNoteToSelection => {
            ed.selection.add(under?);
            Some(Drag::None)
        }
        Action::ToggleNoteSelection => {
            ed.selection.toggle(under?);
            Some(Drag::None)
        }
        Action::DeselectAll => {
            ed.selection.clear();
            Some(Drag::None)
        }
        Action::SelectNoteAndLater | Action::SelectNoteAndLaterSameRow => {
            let id = under?;
            let (start, note_row) = {
                let n = ed.doc.note(id)?;
                (n.start, n.row)
            };
            let same_row = action == Action::SelectNoteAndLaterSameRow;
            ed.selection.notes = ed
                .doc
                .notes
                .iter()
                .filter(|n| n.start >= start && (!same_row || n.row == note_row))
                .map(|n| n.id)
                .collect();
            Some(Drag::None)
        }
        Action::ToggleNoteMute => {
            let id = under?;
            ed.apply_live(&Edit::ToggleMuted { notes: vec![id] });
            Some(Drag::None)
        }
        Action::EraseNote => {
            let id = under?;
            ed.apply_live(&Edit::DeleteNotes(vec![id]));
            Some(Drag::None)
        }
        Action::DoubleNoteLength | Action::HalveNoteLength => {
            let id = under?;
            let factor = if action == Action::DoubleNoteLength {
                2.0
            } else {
                0.5
            };
            ed.apply_live(&Edit::ScaleLength {
                notes: vec![id],
                factor,
            });
            Some(Drag::None)
        }
        Action::EditNoteVelocity | Action::EditNoteVelocityFine => {
            let notes = tools::gesture_targets(&ed.selection, under);
            if notes.is_empty() {
                return None;
            }
            Some(Drag::Velocity {
                notes,
                origin_y: y,
                fine: action == Action::EditNoteVelocityFine,
                applied: 0.0,
            })
        }
        Action::CopyNote | Action::CopyNoteNoSnap => {
            let notes = tools::gesture_targets(&ed.selection, under);
            if notes.is_empty() {
                return None;
            }
            // Duplicate in place; the drag then moves the copies, so
            // the originals stay put exactly where they were.
            ed.apply_live(&Edit::CopyNotes {
                notes: notes.clone(),
                time_delta: 0.0,
                row_delta: 0,
            });
            let copies: Vec<NoteId> = ed
                .doc
                .notes
                .iter()
                .rev()
                .take(notes.len())
                .map(|n| n.id)
                .collect();
            ed.selection.notes = copies.clone();
            Some(Drag::MoveNotes {
                notes: copies,
                origin: (x, y),
                applied_rows: 0,
                applied_time: 0.0,
            })
        }
        Action::MoveNote | Action::MoveNoteNoSnap | Action::MoveNoteOneAxis => {
            let id = under?;
            if !ed.selection.contains(id) {
                ed.selection.set_single(id);
            }
            Some(Drag::MoveNotes {
                notes: ed.selection.notes.clone(),
                origin: (x, y),
                applied_rows: 0,
                applied_time: 0.0,
            })
        }
        Action::MoveNoteEdge | Action::MoveNoteEdgeNoSnap => {
            let Hit::NoteEdge { id, start_edge } = ed.hit_test(x, y) else {
                return None;
            };
            let n = ed.doc.note(id)?;
            let original = (n.start, n.end);
            Some(Drag::Resize {
                note: id,
                start_edge,
                original,
            })
        }
        Action::InsertNote | Action::InsertNoteNoSnap | Action::InsertNoteDragToExtend => {
            Some(begin_new_note(ed, x, y, mods))
        }
        Action::PaintNotes | Action::PaintNotesNoSnap => {
            let snap = action == Action::PaintNotes;
            let mut drag = Drag::Paint {
                last_cell: None,
                snap,
            };
            paint_at(ed, &mut drag, x, y);
            Some(drag)
        }
        Action::MarqueeSelect | Action::MarqueeAdd | Action::MarqueeToggle => {
            if action == Action::MarqueeSelect {
                ed.selection.clear();
            }
            Some(Drag::Marquee {
                origin: (x, y),
                current: (x, y),
                additive: action != Action::MarqueeSelect,
            })
        }
        Action::MovePlayhead | Action::MovePlayheadNoSnap => {
            ed.playhead = Some(if action == Action::MovePlayhead {
                ed.snap_time(t)
            } else {
                t
            });
            Some(Drag::None)
        }
        Action::SelectRow => {
            ed.selection.notes = ed
                .doc
                .notes
                .iter()
                .filter(|n| n.row == row)
                .map(|n| n.id)
                .collect();
            Some(Drag::None)
        }
        // ── controller lanes ─────────────────────────────────────────
        // All four need a dimension to act on. Outside CC edit mode there is
        // no active controller, so they fall through to the tool path
        // rather than inventing one.
        Action::EditCcEvents => {
            let number = ed.cc_edit?;
            let mut drag = Drag::CcPen { number, last: None };
            cc_draw(ed, &mut drag, x, y);
            Some(drag)
        }
        Action::DrawCcLine => {
            let number = ed.cc_edit?;
            Some(Drag::CcLine {
                number,
                start: (x, y),
                current: (x, y),
            })
        }
        Action::EraseCcEvents => {
            let number = ed.cc_edit?;
            Some(Drag::CcErase {
                number,
                start_t: t,
                current_t: t,
            })
        }
        Action::ScaleCcEvents => {
            let number = ed.cc_edit?;
            // The gesture opens with no width; the range grows as the
            // drag sweeps sideways while the vertical offset sets depth.
            Some(Drag::CcScale {
                number,
                t0: t,
                t1: t,
                base: ed
                    .doc
                    .cc
                    .get(number)
                    .map(|l| l.curve.points().to_vec())
                    .unwrap_or_default(),
                spanned: (t, t),
                pivot: cc_mean(ed, number, t, t),
                origin_y: y,
            })
        }

        Action::ContextMenu => Some(Drag::ContextMenu { x, y, under, t }),

        // Expression tools and anything unmapped stay with the
        // tool-driven path.
        Action::ActiveTool | Action::PenOverride => None,
        _ => {
            let _ = context;
            None
        }
    }
}

/// Resolve a press against the timing separators.
///
/// Returns `None` when the press was not on one, so the caller falls
/// through to the ordinary path.
fn separator_press(ed: &mut Editor, x: f64, y: f64, gesture: Gesture) -> Option<Drag> {
    if !ed.timing_mode {
        return None;
    }
    let grab = crate::canvas::SEPARATOR_GRAB_PX;
    let line = crate::canvas::separators(ed)
        .into_iter()
        .find(|s| (s.x - x).abs() <= grab)?;

    // Double-click puts the boundary on the beat, which is the fastest
    // way to fix a phrase that drifted.
    if gesture == Gesture::DoubleClick {
        let step = ed.grid.step(ed.units_per_beat());
        let to = expression_editor_core::timing::snap_to_beat(line.sep.t, ed.doc.start, step);
        ed.begin_gesture();
        for e in expression_editor_core::timing::plan(
            &ed.doc,
            line.sep,
            to,
            expression_editor_core::StretchLaw::BothStretch,
        ) {
            ed.apply_live(&e);
        }
        return Some(Drag::None);
    }

    let law = expression_editor_core::StretchLaw::at(y, ed.viewport.h);
    ed.begin_gesture();
    Some(Drag::Separator { sep: line.sep, law })
}

/// Route a press while a pitch drawing is open.
///
/// The draft owns the surface: while it is up, a click is an anchor and
/// nothing else. That is deliberate — the drawing is a modal state with
/// an explicit apply, and letting notes be selected or moved underneath
/// it would make "what does Escape throw away?" unanswerable.
pub fn draft_press(
    ed: &Editor,
    draft: &mut expression_editor_core::PitchDraft,
    x: f64,
    y: f64,
) -> Drag {
    let t = ed.camera.t_at(x);
    let Some(note) = ed.doc.note(draft.note) else {
        return Drag::None;
    };
    // Values are semitones from the note's row, the same units the
    // pitch curve uses.
    let value = ed.camera.pitch_at(y, ed.viewport) - note.row as f64;
    let tolerance = ed.camera.units_per_px * expression_editor_core::draft::GRAB_PX;

    match draft.anchor_at(t, tolerance) {
        Some(i) => {
            // One checkpoint for the whole drag, not one per move.
            draft.begin_move();
            draft.move_to(i, t, value);
            Drag::DraftAnchor { index: i }
        }
        None => {
            draft.add(t, value);
            let i = draft.anchor_at(t, tolerance).unwrap_or(0);
            Drag::DraftAnchor { index: i }
        }
    }
}

/// Continue an anchor drag.
pub fn draft_move(
    ed: &Editor,
    draft: &mut expression_editor_core::PitchDraft,
    index: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    let note = ed.doc.note(draft.note)?;
    let t = ed.camera.t_at(x);
    let value = ed.camera.pitch_at(y, ed.viewport) - note.row as f64;
    draft.move_to(index, t, value);
    // Re-sorting can move the grabbed anchor, so the caller has to be
    // told where it went or the next frame drags the wrong one.
    let tolerance = ed.camera.units_per_px * expression_editor_core::draft::GRAB_PX;
    draft.anchor_at(t, tolerance.max(1e-9))
}

/// Resolve a press against the note handles.
///
/// Returns `None` when the press was not on a handle, so the caller
/// falls through to the ordinary map-driven path.
fn handle_press(ed: &mut Editor, x: f64, y: f64, mods: Mods, gesture: Gesture) -> Option<Drag> {
    if !ed.mode.has_handles() {
        return None;
    }
    let sets = crate::canvas::note_handles(ed);
    let (id, handle) = sets
        .iter()
        .find_map(|s| handles::hit(&s.rects, x, y).map(|h| (s.id, h)))?;

    // A horizontal drag across the *body* draws a temporary note rather
    // than moving pitch: the range gesture has to start somewhere, and
    // the body is the only handle wide enough to sweep across.
    // Committing to it on the first horizontal movement is what keeps a
    // plain vertical pitch drag from being stolen.
    if handle == Handle::Pitch && mods.alt {
        let t = ed.camera.t_at(x);
        return Some(Drag::TempNote {
            note: id,
            origin_t: t,
            current_t: t,
        });
    }

    // Right-click on the amplitude handle mutes, exactly as the manual
    // has it, and opens no drag.
    if gesture == Gesture::RightClick {
        if handle == Handle::Amplitude {
            ed.begin_gesture();
            ed.apply(&Edit::ToggleMuted { notes: vec![id] });
            return Some(Drag::None);
        }
        return None;
    }

    let note = ed.doc.note(id)?;
    let scope = ed.scope_for(id);
    if !scope.is_valid(note) {
        return None;
    }
    // Captured at press, not read live: releasing Shift mid-drag must
    // not change which spans a gesture has already started writing.
    let sibilants = ed.sibilant_scope != mods.shift;
    let drag = handles::HandleDrag::begin_with(handle, note, scope, y, sibilants);
    ed.begin_gesture();
    Some(Drag::Handle(Box::new(drag)))
}

/// The mean value a controller holds over `[t0, t1]`.
///
/// Used as the pivot for [`Drag::CcScale`]: scaling about the range's
/// own mean is what makes a factor of 0 flatten a swell to its average
/// level instead of dropping it to silence.
fn cc_mean(ed: &Editor, number: u8, t0: f64, t1: f64) -> f64 {
    let Some(dimension) = ed.doc.cc.get(number) else {
        return 0.0;
    };
    let default = dimension.default_value();
    if (t1 - t0).abs() < f64::EPSILON {
        return dimension.curve.sample(t0, default);
    }
    const STEPS: usize = 32;
    let sum: f64 = (0..=STEPS)
        .map(|i| {
            let f = i as f64 / STEPS as f64;
            dimension.curve.sample(t0 + (t1 - t0) * f, default)
        })
        .sum();
    sum / (STEPS + 1) as f64
}

/// Write a shaped ramp between the two ends of a [`Drag::CcLine`].
///
/// Endpoints snap to the grid unless shift reverses it, which is the
/// same rule the razor and note gestures follow. The interior is
/// resampled through the toolbar shape, so switching shape after
/// release restyles the ramp without redrawing it.
fn cc_line(ed: &mut Editor, number: u8, start: (f64, f64), current: (f64, f64), mods: Mods) {
    let snap = ed.grid.enabled && !mods.shift;
    let raw0 = ed.camera.t_at(start.0);
    let raw1 = ed.camera.t_at(current.0);
    let (mut t0, mut t1) = (raw0, raw1);
    if snap {
        t0 = ed.snap_time(t0);
        t1 = ed.snap_time(t1);
    }
    let v0 = expression_editor_core::cc::cc_value(start.1, ed.viewport.h);
    let v1 = expression_editor_core::cc::cc_value(current.1, ed.viewport.h);
    let (lo, hi, v_lo, v_hi) = if t0 <= t1 {
        (t0, t1, v0, v1)
    } else {
        (t1, t0, v1, v0)
    };
    if (hi - lo).abs() < f64::EPSILON {
        return;
    }
    let steps = (((hi - lo) / ed.camera.units_per_px).abs().ceil() as usize).clamp(2, 256);
    let shape = ed.shape;
    let points: Vec<Point> = (0..=steps)
        .map(|i| {
            let f = i as f64 / steps as f64;
            Point {
                t: lo + (hi - lo) * f,
                value: v_lo + (v_hi - v_lo) * shape.amount(f),
            }
        })
        .collect();
    ed.apply_live(&Edit::DrawCc {
        number,
        t0: lo,
        t1: hi,
        points,
    });
}

/// Write one controller sample at the pointer.
///
/// The roll's full height is 0..127, so a controller drawn here spans
/// the whole canvas — which is the point of editing it on the roll
/// rather than in a short strip.
fn cc_draw(ed: &mut Editor, drag: &mut Drag, x: f64, y: f64) {
    let Drag::CcPen { number, last } = drag else {
        return;
    };
    let t = ed.camera.t_at(x);
    let value = expression_editor_core::cc::cc_value(y, ed.viewport.h);

    // Ramp from the previous sample to this one. Holding the new value
    // flat across the gap and jumping at the end is what turns a smooth
    // stroke into a staircase — visible the moment the pointer moves
    // faster than one sample per pixel.
    let (from_t, from_v) = last.unwrap_or((t, value));
    let (lo, hi, v_lo, v_hi) = if from_t <= t {
        (from_t, t, from_v, value)
    } else {
        (t, from_t, value, from_v)
    };
    let steps = (((hi - lo) / ed.camera.units_per_px).abs().ceil() as usize).clamp(1, 256);
    let points: Vec<Point> = (0..=steps)
        .map(|i| {
            let f = i as f64 / steps as f64;
            Point {
                t: lo + (hi - lo) * f,
                value: v_lo + (v_hi - v_lo) * f,
            }
        })
        .collect();
    ed.apply_live(&Edit::DrawCc {
        number: *number,
        t0: lo,
        t1: hi,
        points,
    });
    *last = Some((t, value));
}

/// Fill the grid cell under the pointer, once per cell.
fn paint_at(ed: &mut Editor, drag: &mut Drag, x: f64, y: f64) {
    let Drag::Paint { last_cell, snap } = drag else {
        return;
    };
    let step = ed.grid.step(ed.units_per_beat()).max(1.0);
    let raw = ed.camera.t_at(x);
    let row = ed.camera.pitch_at(y, ed.viewport).round() as i32;
    let cell = ((raw - ed.doc.start) / step).floor() as i64;
    // One note per cell per sweep, or a slow drag stacks dozens.
    if *last_cell == Some((cell, row)) {
        return;
    }
    *last_cell = Some((cell, row));

    let start = if *snap {
        ed.doc.start + cell as f64 * step
    } else {
        raw
    };
    let end = start + step;
    if ed
        .doc
        .notes
        .iter()
        .any(|n| n.row == row && n.start < end && n.end > start)
    {
        return;
    }
    let id = ed.doc.mint_id();
    let mut note = expression_editor_core::Note::new(id, start, end, row.clamp(0, 127));
    for dimension in Dimension::ALL {
        let v = dimension.default_value();
        note.curve_mut(dimension).set(start, v);
        note.curve_mut(dimension).set(end, v);
    }
    ed.apply_live(&Edit::AddNote(Box::new(note)));
}

/// The tool-driven path, for expression tools the map defers to.
fn legacy_pointer_down(ed: &mut Editor, x: f64, y: f64, mods: Mods, button: u16) -> Drag {
    // Right-drag pans regardless of tool.
    if button == 2 || (mods.ctrl && mods.shift) {
        return Drag::Pan { last: (x, y) };
    }

    let hit = ed.hit_test(x, y);

    // A split handle always wins — it is small and deliberate.
    if let Hit::ZoneSplit { id, t, .. } = hit {
        if mods.alt {
            ed.apply(&Edit::RemoveZoneSplit {
                note: id,
                t,
                tolerance: f64::INFINITY,
            });
            return Drag::None;
        }
        ed.begin_gesture();
        return Drag::SplitDrag { note: id, from: t };
    }

    if let Hit::NoteEdge { id, start_edge } = hit {
        if ed.tool == Tool::NoteDraw || ed.tool == Tool::Select {
            let n = ed.doc.note(id).expect("hit test returned a live note");
            let original = (n.start, n.end);
            ed.begin_gesture();
            return Drag::Resize {
                note: id,
                start_edge,
                original,
            };
        }
    }

    let under = match hit {
        Hit::Note { id, zone } => {
            // Clicking a selected note's active zone toggles between
            // single-zone and whole-note targeting.
            if ed.selection.contains(id) {
                if let Some(n) = ed.doc.note(id) {
                    let next = tools::toggle_target(n.target, zone);
                    ed.apply(&Edit::SetTarget {
                        note: id,
                        target: next,
                    });
                }
            } else if mods.shift {
                ed.selection.add(id);
            } else if mods.ctrl {
                ed.selection.toggle(id);
            } else {
                ed.selection.set_single(id);
                ed.apply(&Edit::SetTarget {
                    note: id,
                    target: Target::Zone(zone),
                });
            }
            Some(id)
        }
        Hit::CurvePoint { id, .. } => Some(id),
        _ => None,
    };

    if ed.tool == Tool::NoteErase {
        if let Some(id) = under {
            ed.begin_gesture();
            ed.apply_live(&Edit::DeleteNotes(vec![id]));
        }
        return Drag::NoteErase;
    }

    let notes = tools::gesture_targets(&ed.selection, under);

    // Shift-drag transposes; alt-drag scales. Both outrank the active
    // drawing tool.
    if !notes.is_empty() {
        if mods.shift && !ed.tool.edits_notes() {
            let base_rows = notes
                .iter()
                .filter_map(|&id| ed.doc.note(id).map(|n| n.row))
                .collect();
            ed.begin_gesture();
            return Drag::Transpose {
                notes,
                origin: (x, y),
                base_rows,
                applied: 0,
            };
        }
        if mods.alt && ed.tool.edits_expression() {
            ed.begin_gesture();
            return Drag::Scale {
                notes,
                origin: (x, y),
                applied: 1.0,
            };
        }
    }

    // Ctrl-drag is a temporary off-grid Pen from any tool.
    let tool = if mods.ctrl { Tool::Pen } else { ed.tool };

    match tool {
        Tool::Select => {
            if under.is_some() {
                ed.begin_gesture();
                Drag::MoveNotes {
                    notes,
                    origin: (x, y),
                    applied_rows: 0,
                    applied_time: 0.0,
                }
            } else {
                if !mods.shift {
                    ed.selection.clear();
                }
                Drag::Marquee {
                    origin: (x, y),
                    current: (x, y),
                    additive: mods.shift,
                }
            }
        }
        Tool::Pen if !notes.is_empty() => {
            ed.begin_gesture();
            Drag::Pen {
                notes,
                start: (x, y),
                samples: vec![(x, y)],
            }
        }
        Tool::Curve if !notes.is_empty() => {
            ed.begin_gesture();
            Drag::Curve {
                notes,
                start: (x, y),
                current: (x, y),
            }
        }
        Tool::Eraser if !notes.is_empty() => {
            let t = ed.camera.t_at(x);
            ed.begin_gesture();
            Drag::Erase {
                notes,
                start_t: t,
                current_t: t,
            }
        }
        Tool::NoteDraw => {
            if under.is_some() {
                ed.begin_gesture();
                Drag::MoveNotes {
                    notes,
                    origin: (x, y),
                    applied_rows: 0,
                    applied_time: 0.0,
                }
            } else {
                begin_new_note(ed, x, y, mods)
            }
        }
        _ => {
            if under.is_none() && !mods.shift {
                ed.selection.clear();
            }
            Drag::None
        }
    }
}

/// Note Draw on blank canvas: fill the grid division under the pointer.
fn begin_new_note(ed: &mut Editor, x: f64, y: f64, mods: Mods) -> Drag {
    let raw_t = ed.camera.t_at(x);
    let row = ed.camera.pitch_at(y, ed.viewport).round() as i32;
    let snap = ed.grid.enabled && !mods.shift;
    let (start, end) = if snap {
        let step = ed.grid.step(ed.units_per_beat());
        let cell = ed.snap_time(raw_t - step * 0.5);
        (cell, cell + step)
    } else {
        (raw_t, raw_t + ed.units_per_beat() * 0.25)
    };

    let id = ed.doc.mint_id();
    let mut note = expression_editor_core::Note::new(id, start, end, row.clamp(0, 127));
    // A new note is playable immediately: centered pitch, MPE-default
    // pressure and timbre across its full length.
    for dimension in Dimension::ALL {
        let v = dimension.default_value();
        note.curve_mut(dimension).set(start, v);
        note.curve_mut(dimension).set(end, v);
    }
    ed.begin_gesture();
    ed.apply_live(&Edit::AddNote(Box::new(note)));
    ed.apply_live(&Edit::AssignChannels {
        notes: vec![id],
        seed: id.0,
    });
    ed.selection.set_single(id);
    Drag::Resize {
        note: id,
        start_edge: false,
        original: (start, end),
    }
}

/// Continue a gesture.
pub fn pointer_move(ed: &mut Editor, drag: &mut Drag, x: f64, y: f64, mods: Mods) {
    match drag {
        // Not a drag; the menu is already open and owns the pointer.
        Drag::None | Drag::ContextMenu { .. } => {}
        Drag::Pan { last } => {
            ed.pan_px(x - last.0, y - last.1);
            *last = (x, y);
        }
        Drag::Marquee { current, .. } => *current = (x, y),
        Drag::Pen {
            notes,
            start,
            samples,
        } => {
            samples.push((x, y));
            write_pen(ed, notes, *start, samples, mods);
        }
        Drag::Curve {
            notes,
            start,
            current,
        } => {
            *current = (x, y);
            write_curve(ed, notes, *start, *current, ed.shape);
        }
        Drag::Erase {
            notes,
            start_t,
            current_t,
        } => {
            *current_t = ed.camera.t_at(x);
            let (t0, t1) = (start_t.min(*current_t), start_t.max(*current_t));
            for &id in notes.iter() {
                let Some(n) = ed.doc.note(id) else { continue };
                let (s, e) = n.target_span();
                ed.apply_live(&Edit::EraseDimension {
                    note: id,
                    dimension: ed.dimension,
                    t0: t0.max(s),
                    t1: t1.min(e),
                });
            }
        }
        Drag::Transpose {
            notes,
            origin,
            base_rows,
            applied,
        } => {
            let base = base_rows.first().copied().unwrap_or(60);
            let raw =
                ed.camera.pitch_at(y, ed.viewport) - ed.camera.pitch_at(origin.1, ed.viewport);
            let target = ed.tuning.snap(base as f64 + raw);
            let delta = target.row - base;
            if delta != *applied {
                ed.apply_live(&Edit::Transpose {
                    notes: notes.clone(),
                    semitones: delta - *applied,
                });
                *applied = delta;
            }
        }
        Drag::MoveNotes {
            notes,
            origin,
            applied_rows,
            applied_time,
        } => {
            let rows = (ed.camera.pitch_at(y, ed.viewport)
                - ed.camera.pitch_at(origin.1, ed.viewport))
            .round() as i32;
            if rows != *applied_rows {
                ed.apply_live(&Edit::Transpose {
                    notes: notes.clone(),
                    semitones: rows - *applied_rows,
                });
                *applied_rows = rows;
            }
            // Horizontal movement preserves the note's grid offset
            // rather than re-snapping it to the grid.
            let raw = (x - origin.0) * ed.camera.units_per_px;
            let delta = if ed.grid.enabled && !mods.shift {
                let step = ed.grid.step(ed.units_per_beat());
                (raw / step).round() * step
            } else {
                raw
            };
            if (delta - *applied_time).abs() > 1e-9 {
                ed.apply_live(&Edit::MoveTime {
                    notes: notes.clone(),
                    delta: delta - *applied_time,
                });
                *applied_time = delta;
            }
        }
        Drag::Scale {
            notes,
            origin,
            applied,
        } => {
            // Exponential response: up expands, down compresses, and
            // continuing past flat reconstructs the gesture inverted.
            let dy = origin.1 - y;
            let factor = (dy / 120.0).exp() * if dy < -240.0 { -1.0 } else { 1.0 };
            let relative = factor / *applied;
            for &id in notes.iter() {
                let Some(n) = ed.doc.note(id) else { continue };
                let spans: Vec<(f64, f64)> = match n.target {
                    // Whole-note targeting scales each zone about its
                    // own center, so the larger melodic contour is
                    // preserved while each zone tightens or expands.
                    Target::WholeNote => n.zones(),
                    Target::Zone(_) => vec![n.target_span()],
                };
                for (t0, t1) in spans {
                    ed.apply_live(&Edit::ScaleDimension {
                        note: id,
                        dimension: ed.dimension,
                        t0,
                        t1,
                        factor: relative,
                    });
                }
            }
            *applied = factor;
        }
        Drag::Resize {
            note,
            start_edge,
            original,
        } => {
            let raw = ed.camera.t_at(x);
            let t = if ed.grid.enabled && !mods.shift {
                ed.snap_time(raw)
            } else {
                raw
            };
            let (start, end) = if *start_edge {
                (t.min(original.1 - 1.0), original.1)
            } else {
                (original.0, t.max(original.0 + 1.0))
            };
            ed.apply_live(&Edit::Resize {
                note: *note,
                start,
                end,
            });
        }
        Drag::SplitDrag { note, from } => {
            let to = ed.camera.t_at(x);
            if ed.apply_live(&Edit::MoveZoneSplit {
                note: *note,
                from: *from,
                to,
            }) {
                // Track where it actually landed — the edit clamps
                // against the neighbouring boundaries.
                if let Some(n) = ed.doc.note(*note) {
                    if let Some(&s) = n.splits.iter().min_by(|a, b| {
                        (*a - to)
                            .abs()
                            .partial_cmp(&(*b - to).abs())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }) {
                        *from = s;
                    }
                }
            }
        }
        Drag::NoteErase => {
            // Writes continuously while swiping, so notes disappear
            // during the gesture rather than after release.
            if let Hit::Note { id, .. } = ed.hit_test(x, y) {
                ed.apply_live(&Edit::DeleteNotes(vec![id]));
            }
        }
        Drag::RazorCreate { current, .. } => *current = (x, y),
        Drag::RazorDrag {
            area,
            origin,
            copy,
            applied_t,
            applied_rows,
            index,
        } => {
            let raw = (x - origin.0) * ed.camera.units_per_px;
            let dt = if ed.grid.enabled && !mods.shift {
                let step = ed.grid.step(ed.units_per_beat());
                (raw / step).round() * step
            } else {
                raw
            };
            let rows = (ed.camera.pitch_at(y, ed.viewport)
                - ed.camera.pitch_at(origin.1, ed.viewport))
            .round() as i32;
            if (dt - *applied_t).abs() < 1e-9 && rows == *applied_rows {
                return;
            }
            // Re-run from the captured area each frame rather than
            // accumulating deltas: a razor move slices, and slicing
            // repeatedly would shred the material.
            expression_editor_core::razor::move_contents(&mut ed.doc, *area, dt, rows, *copy);
            *applied_t = dt;
            *applied_rows = rows;
            let moved = area.translated(dt, rows);
            if let Some(slot) = ed.razor.areas.get_mut(*index) {
                *slot = moved;
            }
        }
        Drag::Paint { .. } => paint_at(ed, drag, x, y),
        Drag::DraftAnchor { .. } => {}
        Drag::Separator { sep, law } => {
            // Rebuilt from the separator captured at press, so the
            // stretch is computed from the original layout every frame
            // rather than compounding on the last one.
            let to = if ed.grid.enabled && !mods.shift {
                ed.snap_time(ed.camera.t_at(x))
            } else {
                ed.camera.t_at(x)
            };
            let edits = expression_editor_core::timing::plan(&ed.doc, *sep, to, *law);
            for e in edits {
                ed.apply_live(&e);
            }
        }
        Drag::Handle(h) => {
            // Shift reverses the pitch snap, as everywhere else here.
            let snap = ed.snap_pitch != mods.shift;
            ed.drag_handle(h, y, snap);
        }
        Drag::TempNote {
            note,
            origin_t,
            current_t,
        } => {
            *current_t = ed.camera.t_at(x);
            let (n, t0, t1) = (*note, *origin_t, *current_t);
            // Live, so the shaded range follows the pointer rather than
            // appearing on release.
            ed.set_temp_note(n, t0, t1);
        }
        Drag::CcPen { .. } => cc_draw(ed, drag, x, y),
        Drag::CcLine {
            number,
            start,
            current,
        } => {
            *current = (x, y);
            cc_line(ed, *number, *start, *current, mods);
        }
        Drag::CcErase {
            number,
            start_t,
            current_t,
        } => {
            *current_t = ed.camera.t_at(x);
            let (t0, t1) = (start_t.min(*current_t), start_t.max(*current_t));
            ed.apply_live(&Edit::EraseCc {
                number: *number,
                t0,
                t1,
            });
        }
        Drag::CcScale {
            number,
            t0,
            t1,
            base,
            spanned,
            pivot,
            origin_y,
        } => {
            let t = ed.camera.t_at(x);
            let (lo, hi) = (t0.min(t), t0.max(t));
            // The pivot is captured the moment the range first has
            // width and then held — recomputing it as the sweep grows
            // would move the ground under the vertical drag.
            if (*t1 - *t0).abs() < f64::EPSILON && (hi - lo).abs() > f64::EPSILON {
                *pivot = cc_mean(ed, *number, lo, hi);
            }
            *t1 = t;
            spanned.0 = spanned.0.min(lo);
            spanned.1 = spanned.1.max(hi);

            // A full viewport of travel spans 0x..2x, so the useful
            // range is reachable without a marathon drag and neutral
            // stays where the gesture started.
            let dy = (*origin_y - y) / ed.viewport.h.max(1.0);
            let factor = (1.0 + dy * 2.0).clamp(0.0, 4.0);

            // Rewrite the whole swept-ever span from the captured
            // points: scaled inside the current range, verbatim outside
            // it, so narrowing the sweep puts back what widening it
            // touched.
            let (p, s0, s1) = (*pivot, spanned.0, spanned.1);
            let points: Vec<Point> = base
                .iter()
                .filter(|pt| pt.t >= s0 && pt.t <= s1)
                .map(|pt| Point {
                    t: pt.t,
                    value: if pt.t >= lo && pt.t <= hi {
                        (p + (pt.value - p) * factor).clamp(0.0, 1.0)
                    } else {
                        pt.value
                    },
                })
                .collect();
            ed.apply_live(&Edit::DrawCc {
                number: *number,
                t0: s0,
                t1: s1,
                points,
            });
        }
        Drag::Velocity {
            notes,
            origin_y,
            fine,
            applied,
        } => {
            // Fine mode is a tenth the travel — the difference between
            // shaping a phrase and nudging one hit.
            let scale = if *fine { 0.0008 } else { 0.008 };
            let delta = (*origin_y - y) * scale;
            let step = delta - *applied;
            if step.abs() > 1e-6 {
                ed.apply_live(&Edit::NudgeVelocity {
                    notes: notes.clone(),
                    delta: step,
                });
                *applied = delta;
            }
        }
    }
}

/// Finish a gesture. Returns the drag to keep live (Curve stays
/// selected so the shape buttons can restyle it).
pub fn pointer_up(ed: &mut Editor, drag: Drag, x: f64, y: f64, mods: Mods) -> Drag {
    match drag {
        Drag::Marquee {
            origin,
            current,
            additive,
        } => {
            let moved = (current.0 - origin.0).abs() + (current.1 - origin.1).abs();
            if moved > 3.0 {
                let mut sel = ed.selection.clone();
                sel.marquee(&ed.doc, &ed.camera, ed.viewport, origin, current, additive);
                ed.selection = sel;
            } else if !mods.shift {
                ed.selection.clear();
            }
            Drag::None
        }
        // The Curve gesture survives release, and so does its CC twin —
        // both stay live so the shape buttons restyle the stroke that
        // was just drawn.
        live @ (Drag::Curve { .. } | Drag::CcLine { .. }) => live,
        Drag::Handle(h) => {
            // Fold whole semitones back into the row, restoring the
            // invariant the pitch drag was allowed to break while it ran.
            ed.end_handle_drag(&h);
            Drag::None
        }
        Drag::TempNote {
            note,
            origin_t,
            current_t,
        } => {
            // A range too small to see is a click, not a selection, and
            // leaves no scope armed on the note.
            if !ed.set_temp_note(note, origin_t, current_t) {
                ed.clear_temp_note();
            }
            Drag::None
        }
        Drag::RazorCreate { origin, current } => {
            let t0 = ed.camera.t_at(origin.0);
            let t1 = ed.camera.t_at(current.0);
            let r0 = ed.camera.pitch_at(origin.1, ed.viewport).round() as i32;
            let r1 = ed.camera.pitch_at(current.1, ed.viewport).round() as i32;
            let (t0, t1) = if ed.grid.enabled && !mods.shift {
                (ed.snap_time(t0), ed.snap_time(t1))
            } else {
                (t0, t1)
            };
            ed.razor.add(RazorArea::new(t0, t1, r0, r1));
            Drag::None
        }
        Drag::Pen {
            notes,
            start,
            samples,
        } => {
            let _ = (notes, start, samples, x, y);
            Drag::None
        }
        _ => Drag::None,
    }
}

/// Map one pointer stroke across every targeted note, honouring each
/// note's own whole-note or zone target.
fn write_pen(
    ed: &mut Editor,
    notes: &[NoteId],
    start: (f64, f64),
    samples: &[(f64, f64)],
    mods: Mods,
) {
    let dimension = ed.dimension;
    let start_t = ed.camera.t_at(start.0);
    for &id in notes {
        let Some(n) = ed.doc.note(id) else { continue };
        let (span0, span1) = n.target_span();
        let row = n.row;

        let points: Vec<Point> = samples
            .iter()
            .map(|&(x, y)| {
                let t = ed.camera.t_at(x).clamp(span0, span1);
                let value = match dimension {
                    // Pitch drawing snaps to semitones unless shift is
                    // held for continuous pitch.
                    Dimension::Pitch => {
                        let p = ed.camera.pitch_at(y, ed.viewport);
                        let p = if mods.shift {
                            p
                        } else {
                            ed.tuning.snap(p).pitch
                        };
                        p - row as f64
                    }
                    _ => tools::lane_box_value(&ed.camera, ed.viewport, row, y),
                };
                Point {
                    t,
                    value: dimension.clamp(value),
                }
            })
            .collect();

        let (t0, t1) = gesture_bounds(&points);
        let (t0, t1) = tools::clamp_gesture((span0, span1), start_t, t0, t1);
        ed.apply_live(&Edit::DrawDimension {
            note: id,
            dimension,
            t0,
            t1,
            points,
        });
    }
}

/// A shaped ramp from the gesture's start to its current position.
fn write_curve(
    ed: &mut Editor,
    notes: &[NoteId],
    start: (f64, f64),
    end: (f64, f64),
    shape: Shape,
) {
    const SAMPLES: usize = 48;
    let dimension = ed.dimension;
    let start_t = ed.camera.t_at(start.0);
    for &id in notes {
        let Some(n) = ed.doc.note(id) else { continue };
        let (span0, span1) = n.target_span();
        let row = n.row;

        let value_at = |y: f64| -> f64 {
            match dimension {
                Dimension::Pitch => ed.camera.pitch_at(y, ed.viewport) - row as f64,
                _ => tools::lane_box_value(&ed.camera, ed.viewport, row, y),
            }
        };
        let (v0, v1) = (value_at(start.1), value_at(end.1));
        let raw0 = ed.camera.t_at(start.0);
        let raw1 = ed.camera.t_at(end.0);
        let (t0, t1) = tools::clamp_gesture((span0, span1), start_t, raw0, raw1);
        if t1 - t0 <= 0.0 {
            continue;
        }
        // Value order follows the drag direction, not time order, so
        // dragging right-to-left still ramps the way it looks.
        let (from, to) = if raw0 <= raw1 { (v0, v1) } else { (v1, v0) };

        let points: Vec<Point> = (0..SAMPLES)
            .map(|i| {
                let f = i as f64 / (SAMPLES - 1) as f64;
                Point {
                    t: t0 + (t1 - t0) * f,
                    value: dimension.clamp(from + (to - from) * shape.amount(f)),
                }
            })
            .collect();
        ed.apply_live(&Edit::DrawDimension {
            note: id,
            dimension,
            t0,
            t1,
            points,
        });
    }
}

fn gesture_bounds(points: &[Point]) -> (f64, f64) {
    let mut lo = f64::MAX;
    let mut hi = f64::MIN;
    for p in points {
        lo = lo.min(p.t);
        hi = hi.max(p.t);
    }
    if lo > hi {
        (0.0, 0.0)
    } else {
        (lo, hi)
    }
}

/// Restyle the most recent Curve gesture if there is one; otherwise the
/// active target of every selected note.
pub fn apply_shape(ed: &mut Editor, drag: &Drag, shape: Shape) {
    ed.shape = shape;
    if let Some((notes, x0, x1)) = drag.live_curve() {
        let notes = notes.to_vec();
        let start = (x0, 0.0);
        let end = (x1, 0.0);
        let _ = (start, end);
        // Re-run the gesture with the new shape, keeping its endpoints.
        ed.begin_gesture();
        for &id in &notes {
            let Some(n) = ed.doc.note(id) else { continue };
            let (span0, span1) = n.target_span();
            let (t0, t1) = tools::clamp_gesture(
                (span0, span1),
                ed.camera.t_at(x0),
                ed.camera.t_at(x0),
                ed.camera.t_at(x1),
            );
            ed.apply_live(&Edit::ReshapeDimension {
                note: id,
                dimension: ed.dimension,
                t0,
                t1,
                shape,
                samples: 48,
            });
        }
        return;
    }

    let notes = ed.selection.notes.clone();
    if notes.is_empty() {
        return;
    }
    ed.begin_gesture();
    for id in notes {
        let Some(n) = ed.doc.note(id) else { continue };
        let (t0, t1) = n.target_span();
        ed.apply_live(&Edit::ReshapeDimension {
            note: id,
            dimension: ed.dimension,
            t0,
            t1,
            shape,
            samples: 48,
        });
    }
}

/// Wheel/trackpad routing. `(dx, dy)` are the raw deltas.
pub fn wheel(ed: &mut Editor, x: f64, y: f64, dx: f64, dy: f64, mods: Mods) {
    let factor = (dy.abs() / 300.0).exp();
    match (mods.ctrl, mods.alt, mods.shift) {
        // Ctrl+shift nudges selected notes off-grid — the fine-timing
        // gesture that grid snapping would otherwise make impossible.
        (true, _, true) => {
            let notes = ed.selection.notes.clone();
            if !notes.is_empty() {
                let step = match ed.doc.time_base {
                    expression_editor_core::TimeBase::Ppq { .. } => 10.0,
                    expression_editor_core::TimeBase::Frames { .. } => 1.0,
                };
                ed.apply(&Edit::MoveTime {
                    notes,
                    delta: if dy > 0.0 { step } else { -step },
                });
            }
        }
        // Ctrl+alt scrolls vertically.
        (true, true, _) => ed.pan_px(0.0, -dy),
        // Ctrl zooms pitch.
        (true, false, _) => {
            if dy < 0.0 {
                ed.zoom_in_at(x, y, factor);
            } else {
                ed.zoom_out_at(x, y, factor);
            }
        }
        // Alt scrolls horizontally.
        (false, true, _) => ed.pan_px(-dy, 0.0),
        // Plain wheel zooms time at the pointer; a horizontal
        // trackpad swipe scrolls.
        _ => {
            if dx.abs() > dy.abs() {
                ed.pan_px(-dx, 0.0);
            } else if dy < 0.0 {
                ed.zoom_in_at(x, y, factor);
            } else {
                ed.zoom_out_at(x, y, factor);
            }
        }
    }
}

/// Keyboard shortcuts. Returns true if the key was consumed.
///
/// Space is deliberately absent: transport belongs to the host, and a
/// global Space intercept that outlives a crash would leave the DAW's
/// keyboard locked.
pub fn key_down(ed: &mut Editor, drag: &Drag, key: &str, mods: Mods) -> bool {
    let mouse_t = ed.camera.t_at(ed.viewport.w * 0.5);
    match (key, mods.ctrl, mods.shift) {
        ("z", true, false) => ed.undo(),
        ("z", true, true) | ("y", true, _) => ed.redo(),
        ("a", true, _) => {
            ed.selection.notes = ed.doc.notes.iter().map(|n| n.id).collect();
            true
        }
        ("v", false, _) => {
            ed.reset_view();
            true
        }
        // Contextual zoom. One key, and the pointer region picks the
        // behaviour — the MeMagic idea.
        // Contextual zoom. Shift widens the intent: plain F hugs the
        // local passage, Shift+F frames the whole part.
        ("f", false, shift) => {
            let anchor = ed.playhead.unwrap_or(mouse_t);
            let row = ed.camera.pitch_center;
            let modes = if shift {
                ZoomModes::KEYS
            } else {
                ZoomModes::NOTE_AREA
            };
            ed.smart_zoom(modes, anchor, row);
            true
        }
        // Ctrl+X/C/V go through the same command path the context menu
        // uses, so a keyboard cut and a menu cut cannot diverge.
        ("x", true, _) => ed.run_command(&Command::Cut, None),
        ("c", true, _) => ed.run_command(&Command::Copy, None),
        ("v", true, _) => ed.run_command(&Command::Paste, None),
        // Timing mode. The manual's `2`, and free here.
        ("2", false, true) => {
            ed.timing_mode = !ed.timing_mode;
            true
        }
        // Hold to bring the MIDI reference forward, the manual's `M`.
        // Momentary for the same reason `Shift+R` is: it is a thing you
        // check mid-edit, not a mode to be left in.
        ("m", false, _) if ed.reference.is_some() => {
            ed.reference_to_front = true;
            true
        }
        // Arm sibilant editing. Only where there are sibilants: in a
        // MIDI mode this key would toggle an invisible state.
        ("i", false, _) if ed.mode.draws_blobs() => {
            ed.sibilant_scope = !ed.sibilant_scope;
            true
        }
        // Shift+R always brings references forward, so the gesture is
        // reachable from every mode including MPE, where bare `R` is
        // spoken for.
        ("r", false, true) => {
            ed.refs_to_front = true;
            true
        }
        ("s", false, _) => set_tool(ed, Tool::Select),
        ("c", false, _) => set_tool(ed, Tool::Curve),
        ("d", false, _) => set_tool(ed, Tool::NoteDraw),
        ("e", false, _) => set_tool(ed, Tool::NoteErase),
        ("p", false, _) => set_tool(ed, Tool::Pen),
        ("1", false, _) => {
            ed.grid.coarser();
            true
        }
        ("2", false, _) => {
            ed.grid.finer();
            true
        }
        ("t", false, _) => {
            ed.grid.triplet = !ed.grid.triplet;
            true
        }
        ("q", false, _) => {
            let Some(&id) = ed.selection.notes.first() else {
                return false;
            };
            ed.apply(&Edit::AddZoneSplit {
                note: id,
                t: mouse_t,
            })
        }
        ("g", false, _) => {
            let Some(&id) = ed.selection.notes.first() else {
                return false;
            };
            ed.apply(&Edit::SplitNote {
                note: id,
                t: mouse_t,
            })
        }
        // Bare `R` belongs to whichever meaning the current mode can
        // actually use. Channel reassignment is MPE-only — audio and
        // vocal notes have no member channel to assign — so everywhere
        // else the key is free, and goes to Vovious's meaning:
        // bring references forward, held rather than toggled.
        ("r", false, _) if ed.mode != Mode::Mpe => {
            ed.refs_to_front = true;
            true
        }
        ("r", false, _) => {
            let notes = ed.selection.notes.clone();
            !notes.is_empty()
                && ed.apply(&Edit::AssignChannels {
                    notes,
                    seed: 0x5EED,
                })
        }
        ("Delete", _, _) | ("Backspace", _, _) => {
            let notes = ed.selection.notes.clone();
            if notes.is_empty() {
                return false;
            }
            let applied = if drag.is_active() {
                false
            } else {
                ed.apply(&Edit::DeleteNotes(notes))
            };
            ed.selection.clear();
            applied
        }
        _ => false,
    }
}

fn set_tool(ed: &mut Editor, tool: Tool) -> bool {
    ed.tool = tool;
    true
}

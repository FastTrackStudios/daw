//! Drag state and the pointer/keyboard logic that drives it.
//!
//! Kept out of the rsx so it stays readable and testable: every handler
//! here is a plain function over `&mut Editor`, and the component just
//! routes events into them.

use expression_editor_core::doc::{Lane, NoteId, Point, Target};
use expression_editor_core::edit::Edit;
use expression_editor_core::mouse::{Action, Context, Gesture};
use expression_editor_core::razor::RazorArea;
use expression_editor_core::tools::{self, Hit, Mods};
use expression_editor_core::zoom::ZoomModes;
use expression_editor_core::{Editor, Shape, Tool};

/// What the pointer is currently doing. `None` between gestures.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Drag {
    #[default]
    None,
    /// Right-drag or ctrl+shift-drag.
    Pan { last: (f64, f64) },
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
    SplitDrag { note: NoteId, from: f64 },
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
}

/// Which mouse-modifier context `(x, y)` falls in.
///
/// Razor areas outrank notes: once you have drawn a rectangle, dragging
/// inside it must operate on the region, not on whatever note happens
/// to be under the pointer.
pub fn context_at(ed: &Editor, x: f64, y: f64) -> Context {
    let t = ed.camera.t_at(x);
    let row = ed.camera.pitch_at(y, ed.viewport).round() as i32;

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
    // CC edit mode takes the roll: while it is on, the surface belongs
    // to that controller, not to the notes behind it.
    if let Some(number) = ed.cc_edit {
        if gesture == Gesture::Drag && !mods.ctrl && !mods.shift {
            ed.begin_gesture();
            let mut drag = Drag::CcPen { number, last: None };
            cc_draw(ed, &mut drag, x, y);
            return drag;
        }
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
        Action::RazorMoveContents
        | Action::RazorMoveContentsNoSnap
        | Action::RazorCopyContents => {
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
            let factor = if action == Action::DoubleNoteLength { 2.0 } else { 0.5 };
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
        // Expression tools and anything unmapped stay with the
        // tool-driven path.
        Action::ActiveTool | Action::PenOverride => None,
        _ => {
            let _ = context;
            None
        }
    }
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
    for lane in Lane::ALL {
        let v = lane.default_value();
        note.lane_mut(lane).set(start, v);
        note.lane_mut(lane).set(end, v);
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
    for lane in Lane::ALL {
        let v = lane.default_value();
        note.lane_mut(lane).set(start, v);
        note.lane_mut(lane).set(end, v);
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
        Drag::None => {}
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
                ed.apply_live(&Edit::EraseLane {
                    note: id,
                    lane: ed.lane,
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
            let raw = ed.camera.pitch_at(y, ed.viewport) - ed.camera.pitch_at(origin.1, ed.viewport);
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
            let rows =
                (ed.camera.pitch_at(y, ed.viewport) - ed.camera.pitch_at(origin.1, ed.viewport))
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
                    ed.apply_live(&Edit::ScaleLane {
                        note: id,
                        lane: ed.lane,
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
                    if let Some(&s) = n
                        .splits
                        .iter()
                        .min_by(|a, b| {
                            (*a - to)
                                .abs()
                                .partial_cmp(&(*b - to).abs())
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                    {
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
            expression_editor_core::razor::move_contents(
                &mut ed.doc,
                *area,
                dt,
                rows,
                *copy,
            );
            *applied_t = dt;
            *applied_rows = rows;
            let moved = area.translated(dt, rows);
            if let Some(slot) = ed.razor.areas.get_mut(*index) {
                *slot = moved;
            }
        }
        Drag::Paint { .. } => paint_at(ed, drag, x, y),
        Drag::CcPen { .. } => cc_draw(ed, drag, x, y),
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
                sel.marquee(
                    &ed.doc,
                    &ed.camera,
                    ed.viewport,
                    origin,
                    current,
                    additive,
                );
                ed.selection = sel;
            } else if !mods.shift {
                ed.selection.clear();
            }
            Drag::None
        }
        // The Curve gesture survives release.
        live @ Drag::Curve { .. } => live,
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
fn write_pen(ed: &mut Editor, notes: &[NoteId], start: (f64, f64), samples: &[(f64, f64)], mods: Mods) {
    let lane = ed.lane;
    let start_t = ed.camera.t_at(start.0);
    for &id in notes {
        let Some(n) = ed.doc.note(id) else { continue };
        let (span0, span1) = n.target_span();
        let row = n.row;

        let points: Vec<Point> = samples
            .iter()
            .map(|&(x, y)| {
                let t = ed.camera.t_at(x).clamp(span0, span1);
                let value = match lane {
                    // Pitch drawing snaps to semitones unless shift is
                    // held for continuous pitch.
                    Lane::Pitch => {
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
                    value: lane.clamp(value),
                }
            })
            .collect();

        let (t0, t1) = gesture_bounds(&points);
        let (t0, t1) = tools::clamp_gesture((span0, span1), start_t, t0, t1);
        ed.apply_live(&Edit::DrawLane {
            note: id,
            lane,
            t0,
            t1,
            points,
        });
    }
}

/// A shaped ramp from the gesture's start to its current position.
fn write_curve(ed: &mut Editor, notes: &[NoteId], start: (f64, f64), end: (f64, f64), shape: Shape) {
    const SAMPLES: usize = 48;
    let lane = ed.lane;
    let start_t = ed.camera.t_at(start.0);
    for &id in notes {
        let Some(n) = ed.doc.note(id) else { continue };
        let (span0, span1) = n.target_span();
        let row = n.row;

        let value_at = |y: f64| -> f64 {
            match lane {
                Lane::Pitch => ed.camera.pitch_at(y, ed.viewport) - row as f64,
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
                    value: lane.clamp(from + (to - from) * shape.amount(f)),
                }
            })
            .collect();
        ed.apply_live(&Edit::DrawLane {
            note: id,
            lane,
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
    if lo > hi { (0.0, 0.0) } else { (lo, hi) }
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
            ed.apply_live(&Edit::ReshapeLane {
                note: id,
                lane: ed.lane,
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
        ed.apply_live(&Edit::ReshapeLane {
            note: id,
            lane: ed.lane,
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
        ("r", false, _) => {
            let notes = ed.selection.notes.clone();
            !notes.is_empty() && ed.apply(&Edit::AssignChannels { notes, seed: 0x5EED })
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

//! `expression-editor-core` — the portable engine behind the
//! expression editor.
//!
//! One editor, two products:
//!
//! - **an MPE editor**, where per-note pitch bend, channel pressure,
//!   and CC74 are edited as properties *of a note* instead of in raw
//!   controller lanes;
//! - **a Melodyne competitor**, where an analyzed audio clip's notes
//!   carry a tracked f0 contour that edits by exactly the same
//!   gestures.
//!
//! They unify because a note in either domain is the same thing: an
//! integer pitch row plus a continuous curve measured in semitones
//! relative to it. See [`doc`] for why that framing is the whole trick,
//! and [`blob`] for the center/drift/vibrato decomposition that makes
//! Melodyne's two headline sliders work on hand-drawn MIDI too.
//!
//! This crate holds no UI, no DSP, and no DAW. It is deliberately
//! dependency-free so it compiles for wasm, Blitz-native, and plugin
//! builds alike, and so the interaction model can be tested headlessly
//! — the split MPElodyne got right, kept.
//!
//! The Dioxus surface lives in `expression-editor-ui`; domain adapters
//! (MIDI takes, `tune_dsp::PitchDoc`) live with their domains.

pub mod blob;
pub mod camera;
pub mod cc;
pub mod chord;
pub mod clipboard;
pub mod doc;
pub mod draft;
pub mod edit;
pub mod handles;
pub mod menu;
pub mod mode;
pub mod modulation;
pub mod mouse;
pub mod multitool;
pub mod razor;
pub mod reference;
pub mod rows;
pub mod shape;
pub mod timing;
pub mod tools;
pub mod tracks;
pub mod tuning;
pub mod zoom;

pub use camera::{Bounds, Camera, Content, VerticalCamera, Viewport};
pub use cc::{CcDisplay, CcLane, CcSet};
pub use chord::Chord;
pub use doc::{Curve, CurveShape, ExpressionDoc, Dimension, Marker, Note, NoteId, Point, Target, TimeBase};
pub use draft::PitchDraft;
pub use edit::{Edit, History};
pub use handles::{Handle, Scope};
pub use mode::{Mode, ModeFamily};
pub use mouse::{Action, MouseMap};
pub use multitool::{Bend, Steepness, Zone};
pub use razor::{RazorArea, RazorSet};
pub use reference::{MidiReference, RefNote, SnapSource};
pub use rows::{Articulation, DrumMap, NoteShape, RowSpace, StringTuning};
pub use shape::Shape;
pub use timing::{Separator, StretchLaw};
pub use tools::{Grid, Hit, Mods, Selection, Tool};
pub use tracks::{RefColor, Track, Workspace};
pub use tuning::{Temperament, Tuning};
pub use zoom::{HorizontalMode, SmartZoom, VerticalMode, ZoomModes};

/// Everything the canvas needs to render and interact, in one place.
///
/// The UI owns one of these and reads it; it never mutates the document
/// except through [`Editor::apply`], which keeps undo honest.
///
/// `PartialEq` so it can ride in a Dioxus prop without a wrapper.
#[derive(Clone, Debug, PartialEq)]
pub struct Editor {
    pub doc: ExpressionDoc,
    /// The shared camera. Its horizontal half is authoritative for every
    /// lane; its vertical half serves the single-track roll.
    pub camera: Camera,
    /// One vertical camera per lane, parallel to the workspace layout.
    ///
    /// Ephemeral — never persisted, re-fitted on load. Time stays on
    /// `camera` because two instruments doubling a line are only
    /// comparable on a common time axis; vertical is per lane because
    /// that is what makes twenty lanes readable at once.
    ///
    /// Fitting happens on load and on an explicit Reset View, and
    /// **never automatically**. Re-fitting when content changes would
    /// rescale a lane under the cursor mid-gesture — drag one note up an
    /// octave and the whole lane moves. The accepted cost is that an
    /// edit can push content out of view, with Reset View one key away.
    pub lane_cameras: Vec<camera::VerticalCamera>,
    pub viewport: Viewport,
    pub tool: Tool,
    /// The dimension being edited. Others may still be drawn as overlays.
    pub dimension: Dimension,
    /// Lanes drawn behind the active one.
    pub overlays: Vec<Dimension>,
    pub selection: Selection,
    /// Which product this editor is being: MIDI, MPE, Drums, Guitar,
    /// Vocals or Audio. Decides which controls are on screen.
    pub mode: Mode,
    /// What the vertical axis means — pitch, drum lanes, or strings.
    pub row_space: RowSpace,
    /// Active razor areas.
    pub razor: RazorSet,
    /// How pinned controller lanes are drawn. The lanes themselves are
    /// document data (`doc.cc`); this is view policy.
    pub cc_display: CcDisplay,
    /// The controller being edited, when CC edit mode is on. The roll
    /// becomes that dimension's editing surface and the notes recede.
    pub cc_edit: Option<u8>,
    /// Velocity / CC lane strip height in pixels, 0 when hidden.
    pub lane_strip_h: f64,
    /// Which dimension the strip shows.
    pub strip_lane: StripLane,
    /// Context × modifier → action. Editable, so a host can ship a
    /// REAPER-matching profile or its own.
    pub mouse: MouseMap,
    pub tuning: Tuning,
    pub grid: Grid,
    pub shape: Shape,
    /// Tempo used to place the grid; a real tempo map lives in the
    /// host adapter.
    pub bpm: f64,
    /// Contextual-zoom tuning.
    pub smart_zoom: SmartZoom,
    /// Beats per bar, for the ruler's bar numbering.
    pub beats_per_bar: f64,
    /// Transport position, when the host supplies one.
    pub playhead: Option<f64>,
    /// Cut/copy/paste buffer. Editor-local rather than system-wide —
    /// see [`clipboard::Clipboard`].
    pub clipboard: clipboard::Clipboard,
    /// The other tracks. `doc` above is always the *active* one; the
    /// workspace holds the rest, each with its own undo stack.
    pub tracks: Workspace,
    /// `R` held: reference tracks come forward. A momentary key rather
    /// than a toggle, because it is used to *check* something mid-edit
    /// and a mode you can forget you are in is worse than a held key.
    pub refs_to_front: bool,
    /// Whether the coarse pitch handle snaps to the tuning. Shift
    /// reverses it per-gesture, as everywhere else on this surface.
    pub snap_pitch: bool,
    /// Whether the amplitude handle edits only the *unvoiced* spans
    /// inside a note rather than the whole thing.
    ///
    /// A consonant carries no pitch, so it is invisible on the pitch
    /// track and untouched by any edit that works on notes — yet a
    /// harsh "s" is exactly the thing that needs riding down. Shift
    /// reverses it per-gesture, so the other scope is always one key
    /// away. When armed, the sibilant spans shade in the waveform and
    /// the amplitude handle draws hollow.
    pub sibilant_scope: bool,
    /// Timing mode: separators draw and take the pointer.
    ///
    /// A mode rather than always-on, because a full-height line at
    /// every note join is a picket fence across a screen that is
    /// otherwise about pitch.
    pub timing_mode: bool,
    /// Show every track stacked on one timeline instead of the single
    /// roll.
    ///
    /// A view flag rather than a mode: the document, selection and
    /// history are untouched, and switching back leaves the roll exactly
    /// where it was. The stack is how you find *which* track needs work
    /// — the roll is where you do it.
    pub stacked: bool,
    /// A MIDI part loaded as a tuning target, drawn behind the notes.
    pub reference: Option<MidiReference>,
    /// `M` held: the reference comes forward, as with `Shift+R` for
    /// reference tracks. Momentary for the same reason.
    pub reference_to_front: bool,
    /// The temporary note: a range inside a note that the handles
    /// address instead of the whole thing. `None` is the ordinary case.
    ///
    /// Not document data — it is a view onto a note, discarded the
    /// moment another range is drawn.
    pub temp_note: Option<(NoteId, f64, f64)>,
    history: History,
}

impl Editor {
    pub fn new(doc: ExpressionDoc, viewport: Viewport) -> Self {
        let content = content_of(&doc);
        let camera = camera::reset_view(content, viewport, CUSHION, PAD);
        let tracks = Workspace::single("Track 1", doc.clone());
        let mut editor = Self {
            doc,
            tracks,
            camera,
            lane_cameras: Vec::new(),
            viewport,
            tool: Tool::Curve,
            dimension: Dimension::Pitch,
            overlays: Vec::new(),
            selection: Selection::default(),
            mode: Mode::default(),
            row_space: RowSpace::Pitch,
            razor: RazorSet::default(),
            cc_display: CcDisplay::default(),
            cc_edit: None,
            lane_strip_h: 96.0,
            strip_lane: StripLane::Velocity,
            mouse: MouseMap::default(),
            tuning: Tuning::default(),
            grid: Grid::default(),
            shape: Shape::Linear,
            bpm: 120.0,
            smart_zoom: SmartZoom::default(),
            beats_per_bar: 4.0,
            playhead: None,
            clipboard: clipboard::Clipboard::default(),
            refs_to_front: false,
            snap_pitch: true,
            sibilant_scope: false,
            timing_mode: false,
            stacked: false,
            reference: None,
            reference_to_front: false,
            temp_note: None,
            history: History::new(tracks::HISTORY_LIMIT),
        };
        // Fit once, up front. Every later fit is an explicit Reset View.
        editor.fit_lanes();
        editor
    }

    /// Add a track and return its index. It does not become active.
    ///
    /// The guid is generated. Hosts that have one should use
    /// [`Editor::add_track_with_guid`] instead, so that anything
    /// persisted against this track still resolves next session.
    pub fn add_track(&mut self, name: impl Into<String>, doc: ExpressionDoc) -> usize {
        self.tracks.push(tracks::Track::new(name, doc))
    }

    /// Add a track carrying the host's identity.
    pub fn add_track_with_guid(
        &mut self,
        guid: impl Into<String>,
        name: impl Into<String>,
        doc: ExpressionDoc,
    ) -> usize {
        self.tracks.push(tracks::Track::with_guid(guid, name, doc))
    }

    /// Rows of headroom a lane leaves around its content before fitting.
    ///
    /// Without it, a track whose notes are all on one row fits to zero
    /// height and draws as a hairline, and a melody touching its own
    /// extremes has notes flush against the lane edge where they read as
    /// clipped.
    pub const LANE_FIT_PAD: f64 = 1.0;

    /// The vertical camera for a lane, if it has been fitted.
    pub fn lane_camera(&self, lane: usize) -> Option<camera::VerticalCamera> {
        self.lane_cameras.get(lane).copied()
    }

    /// Fit every lane to its own content.
    ///
    /// Called on load and from Reset View — **never** in response to an
    /// edit. See [`Editor::lane_cameras`] for why.
    pub fn fit_lanes(&mut self) {
        let count = self.tracks.layout().len();
        let rows = self
            .tracks
            .stack(self.viewport.h as f32, 1.0, 0.0)
            .into_iter()
            .map(|r| (r.lane, r.height as f64))
            .collect::<Vec<_>>();

        self.lane_cameras = (0..count)
            .map(|lane| {
                let height = rows
                    .iter()
                    .find(|(l, _)| *l == lane)
                    .map(|(_, h)| *h)
                    .unwrap_or(self.viewport.h.max(1.0));
                match self
                    .tracks
                    .lane_row_range(lane, &self.doc, Self::LANE_FIT_PAD)
                {
                    Some((lo, hi)) => camera::VerticalCamera::fitted(lo, hi, height),
                    None => camera::VerticalCamera::default(),
                }
            })
            .collect();
    }

    /// Fit one lane, leaving the others where the user put them.
    pub fn fit_lane(&mut self, lane: usize) {
        if self.lane_cameras.len() != self.tracks.layout().len() {
            self.fit_lanes();
            return;
        }
        let height = self
            .tracks
            .stack(self.viewport.h as f32, 1.0, 0.0)
            .into_iter()
            .find(|r| r.lane == lane)
            .map(|r| r.height as f64)
            .unwrap_or(self.viewport.h.max(1.0));
        if let Some((lo, hi)) = self
            .tracks
            .lane_row_range(lane, &self.doc, Self::LANE_FIT_PAD)
        {
            if let Some(slot) = self.lane_cameras.get_mut(lane) {
                *slot = camera::VerticalCamera::fitted(lo, hi, height);
            }
        }
    }

    /// Give the active track the host's identity.
    ///
    /// `Editor::new` cannot know it — the document arrives before the
    /// adapter has resolved which track it came from — so the adapter
    /// hands it over immediately afterwards. Without this, an editor
    /// opened on a REAPER take would key its persisted state on a
    /// generated guid that means nothing next session.
    pub fn adopt_track_identity(&mut self, guid: impl Into<String>, name: Option<String>) {
        let active = self.tracks.active();
        if let Some(track) = self.tracks.track_mut(active) {
            track.guid = guid.into();
            if let Some(name) = name {
                track.name = name;
            }
        }
    }

    /// Switch to track `i`, parking the current document and history.
    ///
    /// The camera does not move: switching tracks changes what you are
    /// editing, not where you are looking. The selection *is* cleared,
    /// because note ids are per-document and a selection carried across
    /// would point at whatever happened to share those ids.
    /// Move to the next track in the active lane, wrapping.
    ///
    /// The escape hatch for the rule that only the active track takes
    /// gestures: with a vocal and its reference MIDI superimposed, this
    /// is how you reach the other one. Never leaves the lane — moving
    /// between lanes is a click.
    ///
    /// Goes through [`Editor::switch_track`] rather than moving the
    /// active index directly, so the live document and its history are
    /// parked exactly as they are on any other track change.
    pub fn cycle_track_in_lane(&mut self) -> bool {
        let Some(lane) = self.tracks.active_lane() else {
            return false;
        };
        match self.tracks.next_in_lane(lane, self.tracks.active()) {
            Some(next) => self.switch_track(next),
            None => false,
        }
    }

    pub fn switch_track(&mut self, i: usize) -> bool {
        // Validate before moving anything out of the editor, so a
        // rejected switch cannot leave `doc` and `history` stranded.
        if i == self.tracks.active() || i >= self.tracks.len() {
            return false;
        }
        let history = core::mem::replace(&mut self.history, History::new(tracks::HISTORY_LIMIT));
        let Some((doc, history)) = self.tracks.swap_active(i, self.doc.clone(), history) else {
            return false;
        };
        self.doc = doc;
        self.history = history;
        // Note ids are per-document, so a carried-over selection would
        // point at whatever happens to share those ids on the new track.
        self.selection.clear();
        self.razor = RazorSet::default();
        self.cc_edit = None;
        // The new track brings its own surface with it. Without this a
        // vocal switched to from a kit would be edited on a slice strip
        // — the document changed and nothing else did.
        let mode = self.tracks.mode();
        if mode != self.mode {
            self.set_mode(mode);
        }
        true
    }

    /// The track being edited.
    pub fn active_track(&self) -> usize {
        self.tracks.active()
    }

    /// Show a pitch drawing without recording it.
    pub fn preview_draft(&mut self, draft: &mut draft::PitchDraft) -> bool {
        let mut any = false;
        for e in draft.preview_edits() {
            any |= self.apply_live(&e);
        }
        any
    }

    /// Commit a pitch drawing as **one** step of history.
    ///
    /// Rewinds to the captured curve first, without recording, so the
    /// snapshot the history takes is the state *before* drawing began.
    /// Skipping that would snapshot the live preview instead, and undo
    /// would return to the drawing rather than to what was sung — which
    /// is the whole promise of an explicit apply.
    pub fn apply_draft(&mut self, draft: &draft::PitchDraft) -> bool {
        let Some(commit) = draft.apply_edit() else {
            return false;
        };
        if let Some(rewind) = draft.cancel_edit() {
            self.apply_live(&rewind);
        }
        self.apply(&commit)
    }

    /// Throw a pitch drawing away, restoring exactly what was captured.
    pub fn dismiss_draft(&mut self, draft: &draft::PitchDraft) -> bool {
        match draft.cancel_edit() {
            Some(e) => self.apply_live(&e),
            None => false,
        }
    }

    /// The scope handles on `note` currently address.
    ///
    /// The temporary note if one is open on *this* note, the whole note
    /// otherwise. Resolving it here rather than at each call site is
    /// what makes temporary notes free: every handle already takes a
    /// scope, so nothing else has to know the feature exists.
    pub fn scope_for(&self, note: NoteId) -> handles::Scope {
        match self.temp_note {
            Some((id, t0, t1)) if id == note => handles::Scope::Range { t0, t1 },
            _ => handles::Scope::Note,
        }
    }

    /// Open a temporary note over `[t0, t1]` of `note`.
    ///
    /// Dragging a new range always replaces the previous one — there is
    /// only ever one, and it is discarded rather than accumulated.
    /// Ranges narrower than a pixel or two are rejected, so a stray
    /// click does not leave an invisible scope armed on the note.
    pub fn set_temp_note(&mut self, note: NoteId, t0: f64, t1: f64) -> bool {
        let Some(n) = self.doc.note(note) else {
            return false;
        };
        let scope = handles::Scope::Range { t0, t1 };
        if !scope.is_valid(n) {
            return false;
        }
        let (lo, hi) = scope.span(n);
        if (hi - lo) < self.camera.units_per_px * 3.0 {
            return false;
        }
        self.temp_note = Some((note, lo, hi));
        true
    }

    pub fn clear_temp_note(&mut self) {
        self.temp_note = None;
    }

    /// Write a handle drag at pointer height `y`.
    ///
    /// Always rebuilt from the drag's captured lanes, never from what
    /// is currently on screen — see [`handles::HandleDrag`]. Call inside
    /// a gesture opened with [`Editor::begin_gesture`]; this uses
    /// `apply_live` so the whole drag is one undo step.
    /// `snap` applies only to the coarse pitch handle. The UI resolves
    /// it as `ed.snap_pitch != mods.shift`, the same shift-reverses
    /// rule every other snap on this surface follows.
    pub fn drag_handle(&mut self, drag: &mut handles::HandleDrag, y: f64, snap: bool) -> bool {
        use handles::Handle as H;
        let amount = drag.amount(y, self.viewport.h);
        drag.applied = amount;

        let Some(note) = self.doc.note(drag.note) else {
            return false;
        };
        let (t0, t1) = drag.scope.span(note);
        if t1 <= t0 {
            return false;
        }
        let id = drag.note;

        // Restore the captured dimension first, so the edit below always
        // sees the same input it saw on the previous frame.
        let restore = |ed: &mut Self, dimension: Dimension| {
            let points = drag.base_of(dimension).points().to_vec();
            ed.apply_live(&Edit::RestoreDimension {
                note: id,
                dimension,
                t0,
                t1,
                points,
            });
        };

        match drag.handle {
            H::Pitch | H::FinePitch => {
                restore(self, Dimension::Pitch);
                // Coarse pitch snaps, fine pitch never does — that is
                // the whole distinction between the two handles. The
                // row is left alone during the drag and normalized on
                // release.
                //
                // Snapping goes through the temperament rather than
                // rounding to a semitone, so in a microtonal tuning the
                // handle lands on the tuning's degrees and not on 12-TET
                // ones that are not in the scale.
                let delta = if drag.handle == H::Pitch && snap {
                    // The note's pitch is its contour's *centre*, not
                    // its value at the midpoint — that reading carries
                    // whatever drift and vibrato are passing through,
                    // and snapping against it lands the note wherever
                    // the wobble happened to be.
                    let base = drag.base_row as f64
                        + blob::decompose(
                            drag.base_of(Dimension::Pitch),
                            t0,
                            t1,
                            edit::DEFAULT_SAMPLES,
                            self.doc.time_base.units_per_second(self.bpm),
                            Dimension::Pitch.default_value(),
                        )
                        .center;
                    self.tuning.snap(base + amount).pitch - base
                } else {
                    amount
                };
                self.apply_live(&Edit::ShiftDimension {
                    note: id,
                    dimension: Dimension::Pitch,
                    t0,
                    t1,
                    delta,
                })
            }
            H::LeftSlope | H::RightSlope => {
                restore(self, Dimension::Pitch);
                self.apply_live(&Edit::TiltDimension {
                    note: id,
                    dimension: Dimension::Pitch,
                    t0,
                    t1,
                    amount,
                    from_start: drag.handle == H::LeftSlope,
                })
            }
            H::Formant | H::Amplitude => {
                let dimension = if drag.handle == H::Formant {
                    Dimension::Timbre
                } else {
                    Dimension::Pressure
                };
                // A level, so it reads off the captured value at the
                // scope's midpoint rather than restoring and shifting.
                let mid = (t0 + t1) * 0.5;
                let base = drag.base_of(dimension).sample(mid, dimension.default_value());

                // Sibilant scope: the amplitude handle addresses only
                // the unvoiced spans inside the scope. Each is written
                // separately rather than as one range, because the
                // voiced singing between them must not move.
                if drag.handle == H::Amplitude && drag.sibilants {
                    let spans: Vec<(f64, f64)> = self
                        .doc
                        .unvoiced
                        .iter()
                        .filter(|(a, b)| *b >= t0 && *a <= t1)
                        .map(|(a, b)| (a.max(t0), b.min(t1)))
                        .filter(|(a, b)| b > a)
                        .collect();
                    // A hairline either side of each span, holding the
                    // level the note already had.
                    //
                    // Without these the edit leaks: a curve holds its
                    // endpoint value outside the authored range, so on
                    // a note whose Pressure was never authored, writing
                    // only the consonant would raise the *whole* note
                    // to that level — the singing included, which is
                    // precisely what this scope exists to avoid.
                    let eps = (t1 - t0) * 1e-3;
                    let mut any = false;
                    for (a, b) in spans {
                        // Restore first: the level is absolute, so a
                        // span already written this frame has to go
                        // back before it is written again.
                        let points = drag.base_of(dimension).points().to_vec();
                        self.apply_live(&Edit::RestoreDimension {
                            note: id,
                            dimension,
                            t0: a,
                            t1: b,
                            points,
                        });
                        for (g0, g1) in [(a - eps * 2.0, a - eps), (b + eps, b + eps * 2.0)] {
                            if g0 > t0 && g1 < t1 {
                                let held = drag.base_of(dimension).sample(g0, dimension.default_value());
                                self.apply_live(&Edit::SetDimensionLevel {
                                    note: id,
                                    dimension,
                                    t0: g0,
                                    t1: g1,
                                    value: held,
                                });
                            }
                        }
                        any |= self.apply_live(&Edit::SetDimensionLevel {
                            note: id,
                            dimension,
                            t0: a,
                            t1: b,
                            value: base + amount,
                        });
                    }
                    return any;
                }

                self.apply_live(&Edit::SetDimensionLevel {
                    note: id,
                    dimension,
                    t0,
                    t1,
                    value: base + amount,
                })
            }
            H::Vibrato => {
                restore(self, Dimension::Pitch);
                // 1.0 is as sung; 0 is robotic; above 1 exaggerates.
                // Drift is held at full so the vibrato handle changes
                // only the vibrato, which is what it says it does.
                self.apply_live(&Edit::ReblendPitch {
                    note: id,
                    t0,
                    t1,
                    drift_amount: 1.0,
                    modulation_amount: (1.0 + amount).max(0.0),
                })
            }
        }
    }

    /// Finish a handle drag.
    ///
    /// Folds whole semitones back into the row, restoring the invariant
    /// the surface depends on. Only the pitch handles can break it, so
    /// only they pay for it.
    pub fn end_handle_drag(&mut self, drag: &handles::HandleDrag) -> bool {
        use handles::Handle as H;
        if !matches!(drag.handle, H::Pitch | H::FinePitch) {
            return false;
        }
        self.apply_live(&Edit::NormalizeRow {
            notes: vec![drag.note],
        })
    }

    /// Apply an edit through the undo stack.
    pub fn apply(&mut self, edit: &Edit) -> bool {
        self.history.apply(&mut self.doc, edit)
    }

    /// Snapshot before a drag that will stream many edits, so the whole
    /// gesture collapses into one undo step.
    pub fn begin_gesture(&mut self) {
        self.history.begin_gesture(&self.doc);
    }

    /// Apply without recording — for the streaming edits inside a
    /// gesture already opened with [`Editor::begin_gesture`].
    pub fn apply_live(&mut self, edit: &Edit) -> bool {
        edit.apply(&mut self.doc)
    }

    pub fn undo(&mut self) -> bool {
        self.history.undo(&mut self.doc)
    }

    pub fn redo(&mut self) -> bool {
        self.history.redo(&mut self.doc)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn content(&self) -> Content {
        content_of(&self.doc)
    }

    /// The Reset View camera for the current content.
    pub fn reset_camera(&self) -> Camera {
        camera::reset_view(self.content(), self.viewport, CUSHION, PAD)
    }

    /// `V` — snap directly to Reset View, no interpolation, no magnets.
    pub fn reset_view(&mut self) {
        self.camera = self.reset_camera();
        // Reset View is the one gesture that re-fits lanes. Everything
        // else leaves them exactly where they are, including edits that
        // push content out of view.
        self.fit_lanes();
    }

    pub fn bounds(&self) -> Bounds {
        let c = self.content();
        let span = (c.t_end - c.t_start).max(1.0);
        Bounds {
            t_min: c.t_start - span * CUSHION,
            t_max: c.t_end + span * CUSHION,
            ..Bounds::default()
        }
    }

    /// Zoom in around the pointer, with the edge magnet applied in the
    /// same pass — never as a second mutation.
    pub fn zoom_in_at(&mut self, mouse_x: f64, mouse_y: f64, factor: f64) {
        let content = self.content();
        let anchor_t = self.camera.t_at(mouse_x);
        let anchor_pitch = self.camera.pitch_at(mouse_y, self.viewport);

        let mut base = self.camera;
        base.zoom_time_about(anchor_t, factor);
        base.zoom_pitch_about(anchor_pitch, factor, self.viewport);

        let mut influences = Vec::new();
        if let Some(edge) = camera::edge_magnet(
            base,
            anchor_t,
            content,
            self.viewport,
            EDGE_DEAD_ZONE,
            EDGE_WHITESPACE,
        ) {
            influences.push(edge);
        }
        influences.extend(camera::pitch_focus(
            base,
            self.local_pitch(anchor_t),
            anchor_pitch,
            LOCAL_PITCH_WEIGHT,
            MOUSE_PITCH_WEIGHT,
        ));
        if let Some(deep) = camera::deep_zoom_center(
            base,
            content,
            DEEP_ZOOM_ONSET,
            Bounds::default().max_px_per_semitone,
        ) {
            influences.push(deep);
        }

        self.camera = camera::blend(base, &influences);
        self.camera.constrain(self.bounds(), self.viewport);
    }

    /// Zoom out around the pointer. The reset magnet engages only in
    /// the final stretch — early engagement is what makes a zoom-out
    /// feel like it is being taken away from you.
    pub fn zoom_out_at(&mut self, mouse_x: f64, mouse_y: f64, factor: f64) {
        let anchor_t = self.camera.t_at(mouse_x);
        let anchor_pitch = self.camera.pitch_at(mouse_y, self.viewport);

        let mut base = self.camera;
        base.zoom_time_about(anchor_t, 1.0 / factor.max(1e-6));
        base.zoom_pitch_about(anchor_pitch, 1.0 / factor.max(1e-6), self.viewport);

        let reset = self.reset_camera();
        let mut influences = Vec::new();
        influences.extend(camera::pitch_focus(
            base,
            self.local_pitch(anchor_t),
            anchor_pitch,
            LOCAL_PITCH_WEIGHT,
            MOUSE_PITCH_WEIGHT,
        ));
        if let Some(tail) = camera::reset_tail(base, reset, RESET_TAIL_START) {
            influences.push(tail);
        }

        self.camera = camera::blend(base, &influences);
        self.camera.constrain(self.bounds(), self.viewport);
    }

    pub fn pan_px(&mut self, dx: f64, dy: f64) {
        self.camera.pan_px(dx, dy);
        self.camera.constrain(self.bounds(), self.viewport);
    }

    pub fn resize(&mut self, viewport: Viewport) {
        self.viewport = viewport;
        self.camera.constrain(self.bounds(), self.viewport);
    }

    /// Weighted pitch center of notes near `t` — what the vertical
    /// magnet aims at.
    pub fn local_pitch(&self, t: f64) -> Option<f64> {
        let window = self.viewport.w * self.camera.units_per_px * 0.25;
        let mut sum = 0.0;
        let mut weight = 0.0;
        for n in &self.doc.notes {
            let distance = if t < n.start {
                n.start - t
            } else if t > n.end {
                t - n.end
            } else {
                0.0
            };
            if distance > window {
                continue;
            }
            let w = (1.0 - distance / window.max(1e-9)) * n.weight.max(0.05);
            sum += n.row as f64 * w;
            weight += w;
        }
        (weight > 1e-9).then(|| sum / weight)
    }

    /// Document units per beat at the current tempo.
    pub fn units_per_beat(&self) -> f64 {
        self.doc.time_base.units_per_beat(self.bpm)
    }

    /// Document units per bar.
    pub fn units_per_bar(&self) -> f64 {
        self.units_per_beat() * self.beats_per_bar.max(1.0)
    }

    /// `(bar, beat)` at `t`, both 1-based — what the ruler prints.
    pub fn bar_beat(&self, t: f64) -> (i64, i64) {
        let beats = (t - self.doc.start) / self.units_per_beat();
        let bpb = self.beats_per_bar.max(1.0);
        let bar = (beats / bpb).floor();
        (bar as i64 + 1, (beats - bar * bpb).floor() as i64 + 1)
    }

    /// Snap a time to the local grid.
    pub fn snap_time(&self, t: f64) -> f64 {
        self.grid.snap(t, self.doc.start, self.units_per_beat())
    }

    /// Notes a discrete command acts on.
    ///
    /// A right-click on an unselected note targets *that* note. Menus
    /// that quietly act on a selection somewhere else on screen are how
    /// the wrong bar gets deleted.
    pub fn command_targets(&self, under: Option<NoteId>) -> Vec<NoteId> {
        match under {
            Some(id) if !self.selection.notes.contains(&id) => vec![id],
            Some(id) => {
                if self.selection.notes.is_empty() {
                    vec![id]
                } else {
                    self.selection.notes.clone()
                }
            }
            None => self.selection.notes.clone(),
        }
    }

    /// Note ids inside the bar containing `t`.
    pub fn notes_in_measure(&self, t: f64) -> Vec<NoteId> {
        let bar = self.units_per_bar();
        let i = ((t - self.doc.start) / bar).floor();
        let (lo, hi) = (self.doc.start + i * bar, self.doc.start + (i + 1.0) * bar);
        self.doc
            .notes
            .iter()
            .filter(|n| n.start < hi && n.end > lo)
            .map(|n| n.id)
            .collect()
    }

    /// Run a context-menu command.
    ///
    /// Commands the core cannot complete on its own — the ones that
    /// need a text field or a submenu — return `false` so the UI knows
    /// to open something rather than assuming the edit happened.
    pub fn run_command(&mut self, cmd: &menu::Command, under: Option<NoteId>) -> bool {
        use menu::Command as C;
        let targets = self.command_targets(under);
        match cmd {
            C::Copy => self.clipboard.copy_from(&self.doc, &targets),
            C::Cut => {
                if !self.clipboard.copy_from(&self.doc, &targets) {
                    return false;
                }
                self.apply(&Edit::DeleteNotes(targets))
            }
            C::Paste => {
                // Paste lands at the playhead when there is one, and
                // back where it came from otherwise — never silently at
                // zero, which puts the phrase off-screen.
                let t = self.playhead.unwrap_or(self.doc.start);
                let notes = self.clipboard.placed(t, self.clipboard.origin_row());
                self.apply(&Edit::PasteNotes(notes))
            }
            C::Delete => self.apply(&Edit::DeleteNotes(targets)),
            C::SelectAll => {
                self.selection.notes = self.doc.notes.iter().map(|n| n.id).collect();
                true
            }
            C::SelectMeasure => {
                let t = self
                    .playhead
                    .or_else(|| {
                        targets
                            .first()
                            .and_then(|id| self.doc.note(*id))
                            .map(|n| n.start)
                    })
                    .unwrap_or(self.doc.start);
                self.selection.notes = self.notes_in_measure(t);
                !self.selection.notes.is_empty()
            }
            C::CopyMeasure => {
                let t = self
                    .playhead
                    .or_else(|| {
                        targets
                            .first()
                            .and_then(|id| self.doc.note(*id))
                            .map(|n| n.start)
                    })
                    .unwrap_or(self.doc.start);
                let ids = self.notes_in_measure(t);
                self.clipboard.copy_from(&self.doc, &ids)
            }
            C::ClearExpression => {
                let dimension = self.dimension;
                let spans: Vec<(NoteId, f64, f64)> = targets
                    .iter()
                    .filter_map(|id| self.doc.note(*id).map(|n| (*id, n.start, n.end)))
                    .collect();
                let mut any = false;
                for (id, t0, t1) in spans {
                    any |= self.apply(&Edit::EraseDimension {
                        note: id,
                        dimension,
                        t0,
                        t1,
                    });
                }
                any
            }
            C::ToggleMute => self.apply(&Edit::ToggleMuted { notes: targets }),
            C::AssignChannels => self.apply(&Edit::AssignChannels {
                notes: targets,
                seed: 0,
            }),
            C::CycleString(id) => {
                let Some(n) = self.doc.note(*id) else {
                    return false;
                };
                self.apply(&Edit::SetString {
                    note: *id,
                    string: n.row + 1,
                })
            }
            C::ToggleLegato(id) => {
                let Some(n) = self.doc.note(*id) else {
                    return false;
                };
                let gap = if n.legato { 0.0 } else { 1.0 };
                self.apply(&Edit::Legato {
                    notes: vec![*id],
                    gap,
                })
            }
            C::SplitNote(id, t) => self.apply(&Edit::SplitNote { note: *id, t: *t }),
            C::MergeNotes(id) => self.merge_with_next(*id),
            // These need UI: a text field, a submenu, a panel.
            C::EditLyric(_) | C::SetArticulation(_) | C::Properties => false,
        }
    }

    /// Absorb the next note on the same row into `id`.
    ///
    /// The audio editor's note-assignment merge, and the same operation
    /// a MIDI editor wants for a note split by mistake. The survivor
    /// keeps its own expression and simply extends — re-deriving a
    /// merged curve from two would discard whichever was edited.
    fn merge_with_next(&mut self, id: NoteId) -> bool {
        let Some(n) = self.doc.note(id) else {
            return false;
        };
        let (row, end) = (n.row, n.end);
        let next = self
            .doc
            .notes
            .iter()
            .filter(|o| o.row == row && o.start >= end && o.id != id)
            .min_by(|a, b| a.start.total_cmp(&b.start))
            .map(|o| (o.id, o.end));
        let Some((next_id, next_end)) = next else {
            return false;
        };
        let Some(n) = self.doc.note(id) else {
            return false;
        };
        let start = n.start;
        self.apply(&Edit::Resize {
            note: id,
            start,
            end: next_end,
        }) && self.apply(&Edit::DeleteNotes(vec![next_id]))
    }

    /// Switch mode, re-applying its preset.
    ///
    /// A preset, not a lock: everything it sets can be changed
    /// afterwards. Switching back re-applies.
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        // The track owns the mode; `Editor::mode` is the active one's.
        // Writing both keeps a stack drawn from the workspace agreeing
        // with the surface the user is looking at.
        if let Some(track) = self.tracks.track_mut(self.tracks.active()) {
            track.set_mode(mode);
        }
        // A mode preset supplies a row space, but never overwrites one
        // of the same kind: the document's own may have been tuned —
        // band splits moved, a guitar retuned — and re-applying a preset
        // (which switching tracks does) must not silently undo that.
        if self.doc.row_space.same_kind(&mode.default_row_space()) {
            self.row_space = self.doc.row_space.clone();
        } else {
            self.row_space = mode.default_row_space();
            self.doc.row_space = self.row_space.clone();
        }
        self.mouse = mode.default_mouse();
        self.overlays = mode.default_overlays();
        self.strip_lane = mode.default_strip();
        if !mode.has_expression_lanes() {
            // Leaving the active dimension on Pressure in plain MIDI would
            // point every gesture at something the format cannot carry.
            self.dimension = Dimension::Pitch;
        }
        self.reset_view();
    }

    /// Whether CC edit mode is on.
    pub fn cc_editing(&self) -> bool {
        self.cc_edit.is_some()
    }

    /// Enter CC edit mode on a controller, pinning it if it was not
    /// already visible — editing something invisible is a trap.
    pub fn edit_cc(&mut self, number: u8) {
        self.doc.cc.ensure(number);
        if let Some(l) = self.doc.cc.get_mut(number) {
            l.pinned = true;
        }
        self.cc_edit = Some(number);
    }

    pub fn exit_cc_edit(&mut self) {
        self.cc_edit = None;
    }

    /// Sounding pitches of the selected notes — what the chord box
    /// reads.
    ///
    /// Falls back to the notes under the playhead when nothing is
    /// selected, so the box says something useful while you navigate
    /// rather than going blank.
    pub fn chord_pitches(&self) -> Vec<i32> {
        let space = &self.row_space;
        if !self.selection.notes.is_empty() {
            let mut v: Vec<i32> = self
                .selection
                .notes
                .iter()
                .filter_map(|id| self.doc.note(*id))
                .map(|n| space.pitch_of(n))
                .collect();
            v.sort_unstable();
            v.dedup();
            return v;
        }
        let Some(t) = self.playhead else {
            return Vec::new();
        };
        let mut v: Vec<i32> = self
            .doc
            .notes
            .iter()
            .filter(|n| n.start <= t && n.end > t && !n.muted)
            .map(|n| space.pitch_of(n))
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// The chord the box shows, if the current pitches form one.
    pub fn current_chord(&self) -> Option<Chord> {
        chord::identify(&self.chord_pitches())
    }

    /// Notes reduced to what zoom cares about.
    pub fn zoom_spans(&self) -> Vec<zoom::Span> {
        self.doc
            .notes
            .iter()
            .map(|n| zoom::Span {
                start: n.start,
                end: n.end,
                row: n.row,
            })
            .collect()
    }

    /// Contextual zoom: one gesture, and where the pointer is decides
    /// what "zoom" means. See [`zoom`].
    pub fn smart_zoom(&mut self, modes: ZoomModes, anchor_t: f64, anchor_row: f64) {
        let spans = self.zoom_spans();
        let content = self.content();
        let bar = self.units_per_bar();
        self.camera = zoom::apply_horizontal(
            self.camera,
            modes.horizontal,
            &spans,
            anchor_t,
            content,
            self.viewport,
            bar,
            self.smart_zoom,
        );
        // Vertical runs against the *new* horizontal span, so
        // "notes in view" means notes in the view we just produced —
        // not the one we started from.
        let view = self.camera.time_span(self.viewport);
        self.camera = zoom::apply_vertical(
            self.camera,
            modes.vertical,
            &spans,
            anchor_row,
            self.viewport,
            view,
            self.smart_zoom,
        );
        self.camera.constrain(self.bounds(), self.viewport);
    }

    pub fn hit_test(&self, x: f64, y: f64) -> Hit {
        tools::hit_test(
            &self.doc,
            &self.camera,
            self.viewport,
            self.dimension,
            x,
            y,
            tools::HitConfig::default(),
        )
    }

    /// Lanes to draw, back to front: overlays first, active dimension last.
    pub fn draw_order(&self) -> Vec<Dimension> {
        let mut lanes: Vec<Dimension> = self
            .overlays
            .iter()
            .copied()
            .filter(|&l| l != self.dimension)
            .collect();
        lanes.push(self.dimension);
        lanes
    }
}

/// Horizontal cushion around the item, as a fraction of its span.
pub const CUSHION: f64 = 0.03;
/// Vertical headroom above and below the content in Reset View.
pub const PAD: f64 = 0.35;
/// Edge magnet stays out of the inner 35% of the item's half-span.
const EDGE_DEAD_ZONE: f64 = 0.35;
/// Whitespace the edge magnet leaves past a framed edge.
const EDGE_WHITESPACE: f64 = 0.2;
/// The reset magnet is inert until 80% of the way to Reset View.
const RESET_TAIL_START: f64 = 0.8;
const LOCAL_PITCH_WEIGHT: f64 = 0.45;
const MOUSE_PITCH_WEIGHT: f64 = 0.22;
/// Deep-zoom center pull begins at 72% of the vertical range.
const DEEP_ZOOM_ONSET: f64 = 0.72;

/// The content box a camera should frame.
pub fn content_of(doc: &ExpressionDoc) -> Content {
    let (pitch_lo, pitch_hi) = doc.pitch_extent().unwrap_or((48.0, 72.0));
    Content {
        t_start: doc.start,
        t_end: doc.end.max(doc.start + 1.0),
        pitch_lo,
        pitch_hi: pitch_hi.max(pitch_lo + 1.0),
    }
}

/// What the bottom lane strip displays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StripLane {
    /// Per-note velocity, drawn as stems.
    Velocity,
    /// Per-note release velocity.
    OffVelocity,
    /// A continuous expression dimension, drawn as a curve.
    Expression(Dimension),
}

impl StripLane {
    pub const ALL: [StripLane; 5] = [
        StripLane::Velocity,
        StripLane::OffVelocity,
        StripLane::Expression(Dimension::Pitch),
        StripLane::Expression(Dimension::Pressure),
        StripLane::Expression(Dimension::Timbre),
    ];

    pub fn label(&self) -> &'static str {
        match self {
            StripLane::Velocity => "Velocity",
            StripLane::OffVelocity => "Release",
            StripLane::Expression(Dimension::Pitch) => "Pitch",
            StripLane::Expression(Dimension::Pressure) => "Pressure",
            StripLane::Expression(Dimension::Timbre) => "Timbre",
        }
    }

    /// Per-note lanes draw as stems; continuous ones draw as a curve.
    pub fn is_per_note(&self) -> bool {
        matches!(self, StripLane::Velocity | StripLane::OffVelocity)
    }
}

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
pub mod chord;
pub mod doc;
pub mod edit;
pub mod modulation;
pub mod mouse;
pub mod razor;
pub mod rows;
pub mod shape;
pub mod tools;
pub mod tuning;
pub mod zoom;

pub use camera::{Bounds, Camera, Content, Viewport};
pub use chord::Chord;
pub use doc::{Curve, ExpressionDoc, Lane, Marker, Note, NoteId, Point, Target, TimeBase};
pub use edit::{Edit, History};
pub use shape::Shape;
pub use mouse::{Action, MouseMap};
pub use razor::{RazorArea, RazorSet};
pub use rows::{Articulation, DrumMap, RowSpace, StringTuning};
pub use tools::{Grid, Hit, Mods, Selection, Tool};
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
    pub camera: Camera,
    pub viewport: Viewport,
    pub tool: Tool,
    /// The lane being edited. Others may still be drawn as overlays.
    pub lane: Lane,
    /// Lanes drawn behind the active one.
    pub overlays: Vec<Lane>,
    pub selection: Selection,
    /// What the vertical axis means — pitch, drum lanes, or strings.
    pub row_space: RowSpace,
    /// Active razor areas.
    pub razor: RazorSet,
    /// Velocity / CC lane strip height in pixels, 0 when hidden.
    pub lane_strip_h: f64,
    /// Which lane the strip shows.
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
    history: History,
}

impl Editor {
    pub fn new(doc: ExpressionDoc, viewport: Viewport) -> Self {
        let content = content_of(&doc);
        let camera = camera::reset_view(content, viewport, CUSHION, PAD);
        Self {
            doc,
            camera,
            viewport,
            tool: Tool::Curve,
            lane: Lane::Pitch,
            overlays: Vec::new(),
            selection: Selection::default(),
            row_space: RowSpace::Pitch,
            razor: RazorSet::default(),
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
            history: History::new(10),
        }
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
        if let Some(edge) =
            camera::edge_magnet(base, anchor_t, content, self.viewport, EDGE_DEAD_ZONE, EDGE_WHITESPACE)
        {
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
            self.lane,
            x,
            y,
            tools::HitConfig::default(),
        )
    }

    /// Lanes to draw, back to front: overlays first, active lane last.
    pub fn draw_order(&self) -> Vec<Lane> {
        let mut lanes: Vec<Lane> = self
            .overlays
            .iter()
            .copied()
            .filter(|&l| l != self.lane)
            .collect();
        lanes.push(self.lane);
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
    /// A continuous expression lane, drawn as a curve.
    Expression(Lane),
}

impl StripLane {
    pub const ALL: [StripLane; 5] = [
        StripLane::Velocity,
        StripLane::OffVelocity,
        StripLane::Expression(Lane::Pitch),
        StripLane::Expression(Lane::Pressure),
        StripLane::Expression(Lane::Timbre),
    ];

    pub fn label(&self) -> &'static str {
        match self {
            StripLane::Velocity => "Velocity",
            StripLane::OffVelocity => "Release",
            StripLane::Expression(Lane::Pitch) => "Pitch",
            StripLane::Expression(Lane::Pressure) => "Pressure",
            StripLane::Expression(Lane::Timbre) => "Timbre",
        }
    }

    /// Per-note lanes draw as stems; continuous ones draw as a curve.
    pub fn is_per_note(&self) -> bool {
        matches!(self, StripLane::Velocity | StripLane::OffVelocity)
    }
}

//! ArrangeView — the main timeline, Reaper-style.
//!
//! Composes a [`TrackControlPanel`] sidebar on the left with a scrollable
//! timeline on the right: region/marker/tempo lanes + a time ruler across the
//! top and one lane per track (height `track.height`, aligned with the TCP
//! rows) carrying its clips, plus an envelope lane per visible envelope.
//!
//! Themed by [`crate::theming::ArrangeTheme`] — REAPER's palette-driven
//! arrange vocabulary, rendered the way REAPER composites it:
//! - ruler: `col_tl_bg/fg/fg2`, loop-point band (`col_tl_bgsel2`), the
//!   time-selection band (`col_tl_bgsel`) + its `timesel_drawmode` shading
//!   over the arrange body; the tempo lane (`ts_lane_*`/`col_tsigmark`)
//!   sits at the bottom of the ruler;
//! - grid: the `col_gridlines2/3/''` measure→beat→sub hierarchy (musical
//!   when a tempo is supplied, time-based otherwise), with REAPER's
//!   zoom-gating (levels drop out when their spacing gets too dense);
//! - lanes: alternating `col_tr1/2_bg` (+ `selcol_*` when selected),
//!   `arrange_vgrid` shading in the empty area below the last track;
//! - items: per-parity `col_mi_bg/2` bodies tinted by the item colour at
//!   `itembg_drawmode` strength, waveform peaks (`col_tr*_peaks`),
//!   `col_mi_label(_sel)` text, selected bodies (`col_tr*_itembgsel` +
//!   `selitem_tag` bar), fade triangles (`fadezone_color` fill,
//!   `col_mi_fades` line) and the mute overlay;
//! - envelopes: one lane per visible envelope (`col_env*` curve over a
//!   dimmed row, `col_envlane*_divline` dividers);
//! - marker/region lanes: `marker*`/`region*` flags and bands.
//!
//! The ruler shares the lanes' horizontal scroll: the lane scroller's
//! `onscroll` mirrors `scroll_left` into the ruler content's offset.

use input::{InputCommand, KeymapConfig};
use input_dioxus::use_input_processor;

/// Hover affordance for the track-height / strip-width resize grips: subtle by
/// default, brighter on hover so they're discoverable (embedded so it applies
/// in every render context — plugin, standalone, browser).
const RESIZE_CSS: &str = ".fts-rz{transition:background .08s ease;}\
.fts-rz:hover{background:rgba(120,170,255,0.45)!important;}";

use crate::panels::model::{
    EnvelopeView, LaneDisplay, MarkerView, RegionView, TempoMarkerView, TrackView,
};
use crate::panels::track_control_panel::TrackControlPanel;
use crate::prelude::*;
use crate::theming::{Color, use_theme};

/// Keymap for the arrange view's wheel gestures, routed through the
/// `input`/`input-dioxus` action system (the same gesture→action layer the
/// rest of FTS uses — and the same set as reaper-input's scroll-zoom config).
/// The processor resolves the gesture to an action string; the handler reads
/// the raw wheel delta for direction/amount (the processor drops the delta).
///
/// - plain wheel → vertical scroll
/// - Shift+wheel → horizontal scroll
/// - Alt+wheel → vertical zoom (track height)
/// - Alt+Shift+wheel → horizontal zoom (timeline)
fn arrange_keymap() -> KeymapConfig {
    use std::collections::HashMap;
    let normal = HashMap::from([
        ("Scroll".to_string(), "view.vscroll".to_string()),
        ("Shift+Scroll".to_string(), "view.hscroll".to_string()),
        ("Alt+Scroll".to_string(), "view.zoom_v".to_string()),
        ("Alt+Shift+Scroll".to_string(), "view.zoom_h".to_string()),
    ]);
    KeymapConfig {
        scroll: HashMap::from([("normal".to_string(), normal)]),
        ..Default::default()
    }
}

/// A user edit gesture from the arrange view, reported to the host (via
/// [`ArrangeView`]'s `on_edit`) so it can drive the engine and update the
/// view-model for immediate feedback. All times are seconds.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ArrangeEdit {
    /// Move the edit cursor / transport to this time.
    Seek(f64),
    /// Select a clip — `additive` extends the current selection (shift/ctrl).
    SelectClip {
        track: usize,
        clip: usize,
        additive: bool,
    },
    /// Clear the clip selection (click on empty timeline).
    ClearSelection,
    /// Commit a clip move to a new start time.
    MoveClip {
        track: usize,
        clip: usize,
        start: f64,
    },
    /// Commit a clip trim to a new start + length.
    ResizeClip {
        track: usize,
        clip: usize,
        start: f64,
        length: f64,
    },
}

/// Which part of a clip a drag is manipulating.
#[derive(Clone, Copy, PartialEq, Debug)]
enum DragMode {
    Move,
    ResizeStart,
    ResizeEnd,
}

/// In-flight clip drag (move or trim). Owned by [`ArrangeView`] in a `Signal`;
/// clips read it to render the optimistic position, and the lanes scroller's
/// move/up handlers advance and commit it.
#[derive(Clone, Copy, PartialEq, Debug)]
struct DragState {
    track: usize,
    clip: usize,
    mode: DragMode,
    /// Client-space x at mousedown (px).
    start_x: f64,
    orig_start: f64,
    orig_len: f64,
    /// Current drag delta (seconds), already applied to the rendered clip.
    delta: f64,
    /// Whether the pointer moved past the click threshold (drag vs bare click).
    moved: bool,
}

impl DragState {
    /// The clip's (start, length) with the current drag delta applied.
    fn applied(&self, min_len: f64) -> (f64, f64) {
        match self.mode {
            DragMode::Move => ((self.orig_start + self.delta).max(0.0), self.orig_len),
            DragMode::ResizeEnd => (self.orig_start, (self.orig_len + self.delta).max(min_len)),
            DragMode::ResizeStart => {
                // Left edge moves; the right edge (end) stays put.
                let end = self.orig_start + self.orig_len;
                let start = (self.orig_start + self.delta).clamp(0.0, end - min_len);
                (start, end - start)
            }
        }
    }
}

/// Shared arrange interaction state, provided by [`ArrangeView`] and consumed
/// by the [`Lane`] clips (avoids prop-drilling through `TrackLanes`).
#[derive(Clone, Copy)]
struct ArrangeCtx {
    drag: Signal<Option<DragState>>,
    /// In-flight track-height resize (bottom-edge drag).
    resize: Signal<Option<ResizeState>>,
    /// Vertical-zoom multiplier (the view's global `row_scale`), so lanes +
    /// TCP scale identically and drag deltas convert px→base correctly.
    row_scale: Signal<f32>,
    on_edit: Option<EventHandler<ArrangeEdit>>,
    pps: f64,
    /// Min clip length (seconds) when trimming.
    min_len: f64,
    /// Edge grab zone (px) for trim vs move at a clip's left/right border.
    edge_px: f64,
}

/// An in-flight track-height resize: grab the row's bottom edge and drag.
/// Snapshot the base height + pointer y on mousedown; each move sets the
/// track's height Signal to `start_h + (dy / row_scale)` (the drag is in
/// screen px, the base is pre-zoom), clamped to a sane range. Ports the
/// `slip_drag` begin/move/end shape from reaper-input. `pub(crate)` so the
/// TCP-side handle ([`TrackControlPanel`]) can start one too.
#[derive(Clone, Copy)]
pub struct ResizeState {
    /// The track's base-height signal being resized.
    height: Signal<u32>,
    start_y: f64,
    start_h: u32,
    scale: f32,
}

impl ResizeState {
    /// Begin a resize: `height` is the track's base-height signal, `start_y`
    /// the pointer y, `start_h` the base height, `scale` the current vertical
    /// zoom. For the TCP-side handle in another module.
    pub fn new(height: Signal<u32>, start_y: f64, start_h: u32, scale: f32) -> Self {
        Self {
            height,
            start_y,
            start_h,
            scale,
        }
    }
}

/// Adaptive tick spacing: the smallest "nice" step whose px spacing at this
/// zoom clears `min_px`. Steps follow ruler conventions (1/2/5/10/15/30s,
/// then minutes).
fn pick_step(pps: f64, min_px: f64) -> f64 {
    const STEPS: [f64; 12] = [
        0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0,
    ];
    for s in STEPS {
        if s * pps >= min_px {
            return s;
        }
    }
    600.0
}

/// Ruler label for a time in seconds (`m:ss` / `m:ss.t` under a second-level
/// zoom).
fn time_label(t: f64, step: f64) -> String {
    let m = (t / 60.0).floor() as i64;
    let s = t - m as f64 * 60.0;
    if step < 1.0 {
        format!("{m}:{s:04.1}")
    } else {
        format!("{m}:{s:02.0}")
    }
}

/// The grid/ruler step hierarchy: REAPER's measure → beat → sub-beat levels.
/// Musical when a tempo is known; "nice" time steps otherwise. Levels are
/// zoom-gated: a level collapses into the one above when its px spacing gets
/// too dense (REAPER stops drawing in-between lines at low zoom).
struct GridSteps {
    /// Labeled level (measure / major time step), seconds.
    major: f64,
    /// Beat level, seconds (== major when gated out).
    beat: f64,
    /// Sub-beat level, seconds (== beat when gated out).
    sub: f64,
    /// Whether `major` is a bar (label = bar number).
    musical: bool,
}

/// Adaptive musical grid (the technique the Reaper-Tools "gridbox" uses): the
/// visible subdivision tracks the zoom so lines stay at a comfortable on-screen
/// density — finer divisions (1/8, 1/16, 1/32…) fade in as you zoom in, coarser
/// (2, 4, 8… bars) as you zoom out — always snapped to real musical divisions
/// (halving/doubling the beat), never arbitrary pixel steps. Three weighted
/// tiers: `sub` (finest, faint), `beat` (mid), `major` (labeled bars).
///
/// Density targets (px between lines): the finest tier keeps ≥ `FINE_PX`, beats
/// only draw while ≥ `BEAT_PX` (else fold into the bar), bar labels get
/// ≥ `LABEL_PX` (else label every 2ⁿ bars).
fn grid_steps(pps: f64, bpm: Option<f64>, beats_per_measure: u32) -> GridSteps {
    const FINE_PX: f64 = 9.0;
    const BEAT_PX: f64 = 12.0;
    const LABEL_PX: f64 = 54.0;
    match bpm {
        Some(bpm) if bpm > 1.0 => {
            let beat = 60.0 / bpm;
            let measure = beat * beats_per_measure.max(1) as f64;

            // Finest subdivision: start at the beat and halve while the next
            // finer line would still clear `FINE_PX`; if even the beat is too
            // dense, climb by doubling until it clears. Pure ×½ / ×2 keeps every
            // line on a real division.
            let mut sub = beat;
            while sub * 0.5 * pps >= FINE_PX {
                sub *= 0.5;
            }
            while sub * pps < FINE_PX {
                sub *= 2.0;
            }

            // Bar labels: one bar, doubled to 2/4/8… bars until a label fits.
            let mut major = measure;
            while major * pps < LABEL_PX {
                major *= 2.0;
            }

            // Beat tier: the true beat while readable, else fold up to the bar
            // line. Kept within [sub, major] so the tiers stay ordered.
            let beat = if beat * pps >= BEAT_PX { beat } else { major };
            let beat = beat.clamp(sub, major);

            GridSteps {
                major,
                beat,
                sub,
                musical: true,
            }
        }
        _ => {
            let major = pick_step(pps, 90.0);
            let beat = pick_step(pps, 18.0).min(major);
            let sub = pick_step(pps, 7.0).min(beat);
            GridSteps {
                major,
                beat,
                sub,
                musical: false,
            }
        }
    }
}

/// The arrange view. `pps` = pixels per second (horizontal zoom); `tcp_width`
/// is the control-sidebar width; `seconds` is the visible timeline length.
///
/// Optional project data: `playhead`/`cursor` (s) draw the cursors,
/// `markers`/`regions`/`tempo_markers` add their ruler lanes,
/// `time_sel`/`loop_range` draw the selection/loop bands, and `bpm`
/// (+ `beats_per_measure`) switches the grid + ruler to musical bars.
#[component]
pub fn ArrangeView(
    tracks: Vec<TrackView>,
    #[props(default = 12.0)] pps: f64,
    #[props(default = 380)] tcp_width: u32,
    #[props(default = 120.0)] seconds: f64,
    #[props(default)] playhead: Option<Signal<f64>>,
    #[props(default)] cursor: Option<f64>,
    #[props(default)] markers: Vec<MarkerView>,
    #[props(default)] regions: Vec<RegionView>,
    #[props(default)] tempo_markers: Vec<TempoMarkerView>,
    #[props(default)] time_sel: Option<(f64, f64)>,
    #[props(default)] loop_range: Option<(f64, f64)>,
    #[props(default)] bpm: Option<f64>,
    #[props(default = 4)] beats_per_measure: u32,
    /// User edit gestures (seek / select / move / resize). `None` = read-only.
    #[props(default)]
    on_edit: Option<EventHandler<ArrangeEdit>>,
) -> Element {
    // Horizontal zoom lives here: `pps` (pixels-per-second) is seeded from the
    // prop, then Ctrl+wheel mutates this signal. Shadow the prop with the
    // signal's value so every downstream `pps` use (grid, clips, cursors) picks
    // up the current zoom. (Same gesture set as reaper-input's scrolling
    // actions; a later pass can route these through input-dioxus once that
    // crate is wasm-clean.)
    let mut zoom = use_signal(|| pps);
    let pps = zoom();
    let content_w = (seconds * pps).max(600.0) as u32;

    // Input processor: resolves wheel gestures to action strings via the
    // `input` keymap (config-driven, wasm-clean).
    let input = use_input_processor(arrange_keymap());

    // Every view axis is a signal the wheel + middle-drag handlers drive:
    // `scroll_x`/`scroll_y` pan (translation), `zoom` (pps) is horizontal zoom,
    // `row_scale` is vertical zoom (track height).
    // Horizontal scroll (shared with the ruler) + vertical scroll (translation).
    let mut scroll_x = use_signal(|| 0.0f64);
    let mut scroll_y = use_signal(|| 0.0f64);
    let mut row_scale = use_signal(|| 1.0f32);
    // Measured viewport size (blitz `get_client_rect`): `view_w` (lanes width)
    // lets the grid + ruler extend to the visible right edge; `view_h` (body
    // height) clamps vertical scroll to the content (no infinite vertical
    // scroll — horizontal is unbounded).
    let mut view_w = use_signal(|| 0.0f64);
    let mut view_h = use_signal(|| 0.0f64);
    // Middle-mouse pan anchor: (start client x, y, start scroll_x, scroll_y).
    let mut pan = use_signal(|| None::<(f64, f64, f64, f64)>);
    // In-flight track-height resize (row bottom-edge drag).
    let resize = use_signal(|| None::<ResizeState>);

    // Vertical zoom is a render-time multiplier (`row_scale`) applied to each
    // track's base height signal — so the TCP rows and arrange lanes (which
    // must line up) grow/shrink together, and drag-to-resize (which sets the
    // per-track base height Signal) composes with it. Provided to the lanes +
    // TCP so they scale identically.
    let rs = row_scale();

    // Shared clip-drag state + edit callback, provided to the lane clips.
    let drag = use_signal(|| None::<DragState>);
    use_context_provider(|| ArrangeCtx {
        drag,
        resize,
        row_scale,
        on_edit,
        pps,
        min_len: 0.05,
        edge_px: 6.0,
    });
    let emit = move |edit: ArrangeEdit| {
        if let Some(cb) = on_edit {
            cb.call(edit);
        }
    };
    // Commit an in-flight drag (or fire selection if it never moved), then
    // clear it. Shared by the scroller's mouseup + mouseleave.
    let mut commit_drag = move || {
        let mut drag = drag;
        let cur = *drag.peek();
        if let Some(d) = cur {
            if d.moved {
                let (start, length) = d.applied(0.05);
                match d.mode {
                    DragMode::Move => emit(ArrangeEdit::MoveClip {
                        track: d.track,
                        clip: d.clip,
                        start,
                    }),
                    DragMode::ResizeStart | DragMode::ResizeEnd => emit(ArrangeEdit::ResizeClip {
                        track: d.track,
                        clip: d.clip,
                        start,
                        length,
                    }),
                }
            }
            drag.set(None);
        }
    };

    let g = grid_steps(pps, bpm, beats_per_measure);
    // Draw the grid + ruler out to whichever is longer: the project length or
    // the visible right edge (`scroll_x + measured viewport width`), so lines +
    // bar numbers fill the whole view instead of stopping at the content. Pad a
    // little past the edge to avoid a visible seam while scrolling. `view_w` is
    // 0 until the first measurement, so this falls back to `seconds`.
    let horizon = ((scroll_x() + view_w()) / pps).max(seconds) + g.major * 2.0;
    let n_at = move |step: f64| (horizon / step).ceil() as i64;
    // Total lane height — track rows + visible envelope lanes (the grid
    // covers the tracks; `arrange_vgrid` shades the empty area below).
    let lanes_h: u32 =
        (tracks.iter().map(|t| t.total_height()).sum::<u32>() as f32 * rs).round() as u32;

    // Convert a click's window-x to a timeline time. `element_coordinates()`
    // is a no-op in this blitz pin (and `get_client_rect` reports x=0 for the
    // scroller), so we use the known layout origin: the timeline starts right
    // of the `tcp_width` control sidebar, and the arrange fills from window
    // x=0. `time = (client_x − tcp_width + scroll_x) / pps`.
    let origin = tcp_width as f64;
    let seek_time = move |client_x: f64| ((client_x - origin + scroll_x()) / pps).max(0.0);

    let theme = use_theme().theme;
    let ar = theme.arrange;
    let border = theme.tokens.border.css();

    // Ruler lanes: regions on top, markers under them, the time scale, and
    // the tempo/time-signature lane at the bottom (REAPER's stacking).
    let region_lane_h = if regions.is_empty() { 0 } else { 14u32 };
    let marker_lane_h = if markers.is_empty() { 0 } else { 14u32 };
    let ts_lane_h = if tempo_markers.is_empty() { 0 } else { 13u32 };
    let scale_h = 26u32;
    let ruler_h = region_lane_h + marker_lane_h + scale_h + ts_lane_h;
    let scale_top = region_lane_h + marker_lane_h;

    let ruler_bg = ar.ruler_bg.css();
    let ruler_fg = ar.ruler_fg.css();
    let ruler_fg2 = ar.ruler_fg2.css();
    let empty_bg = ar.empty_bg.css();
    let arrange_bg = ar.bg.css();
    let grid_measure = ar.grid_measure.css();
    let grid_beat = ar.grid_beat.css();
    let grid_sub = ar.grid_sub.css();
    let edit_cursor = ar.edit_cursor.css();
    let play_cursor = ar.play_cursor.css();

    let span_px = |range: (f64, f64)| {
        let (a, b) = (range.0.min(range.1), range.0.max(range.1));
        (a * pps, ((b - a) * pps).max(1.0))
    };

    rsx! {
        // Resize-handle affordance: brighten on hover so track/strip resize
        // grips are discoverable (blitz honours `:hover` in an embedded sheet).
        document::Style { {RESIZE_CSS} }
        div {
            style: format!(
                "display:flex; flex-direction:column; height:100%; min-height:0; background:{empty_bg};"
            ),

            // ── Ruler block: spacer over the TCP, lanes + time scale right ──
            div {
                style: format!(
                    "flex:0 0 {ruler_h}px; height:{ruler_h}px; display:flex; \
                     border-bottom:1px solid {border}; background:{ruler_bg};"
                ),
                div { style: format!("flex:0 0 {tcp_width}px; border-right:1px solid {border};") }
                div {
                    style: "flex:1 1 0; position:relative; overflow:hidden;",
                    div {
                        // Mirrors the lane scroller's horizontal offset.
                        // `min-width:100%` extends the ruler lanes + ticks to
                        // fill the full arrange width (matching the lanes body).
                        style: format!(
                            "position:relative; width:{content_w}px; min-width:100%; height:100%; \
                             left:{x:.1}px; cursor:text;",
                            x = -scroll_x(),
                        ),
                        // Left-click the ruler to move the edit cursor / transport
                        // (middle button is reserved for panning).
                        onmousedown: move |evt: MouseEvent| {
                            if evt.trigger_button()
                                != Some(dioxus_elements::input_data::MouseButton::Primary)
                            {
                                return;
                            }
                            emit(ArrangeEdit::Seek(seek_time(evt.client_coordinates().x)));
                        },

                        // Region lane.
                        if region_lane_h > 0 {
                            div {
                                style: format!(
                                    "position:absolute; left:0; right:0; top:0; height:{region_lane_h}px; \
                                     background:{bg};",
                                    bg = ar.region_lane_bg.css(),
                                ),
                                for r in regions.iter() {
                                    {
                                        let (x, w) = span_px((r.start, r.end));
                                        let fill = r.color.as_deref().and_then(Color::hex).unwrap_or(ar.region);
                                        rsx! {
                                            div {
                                                key: "r{r.idx}",
                                                title: "{r.name}",
                                                style: format!(
                                                    "position:absolute; left:{x:.1}px; width:{w:.1}px; top:0; bottom:0; \
                                                     background:{fill}; border-left:1px solid {edge}; \
                                                     border-right:1px solid {edge}; color:{fg}; font-size:9px; \
                                                     padding:1px 4px; white-space:nowrap; overflow:hidden;",
                                                    fill = fill.css(),
                                                    edge = ar.region_edge.css(),
                                                    fg = ar.region_lane_text.css(),
                                                ),
                                                "{r.name}"
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Marker lane.
                        if marker_lane_h > 0 {
                            div {
                                style: format!(
                                    "position:absolute; left:0; right:0; top:{region_lane_h}px; \
                                     height:{marker_lane_h}px; background:{bg};",
                                    bg = ar.marker_lane_bg.css(),
                                ),
                                for m in markers.iter() {
                                    {
                                        let fill = m.color.as_deref().and_then(Color::hex).unwrap_or(ar.marker);
                                        rsx! {
                                            div {
                                                key: "m{m.idx}",
                                                title: "{m.name}",
                                                style: format!(
                                                    "position:absolute; left:{x:.1}px; top:0; bottom:0; \
                                                     border-left:2px solid {edge}; background:{fill}; \
                                                     color:{fg}; font-size:9px; font-weight:700; \
                                                     padding:1px 4px 1px 3px; white-space:nowrap;",
                                                    x = m.time * pps,
                                                    edge = ar.marker_edge.css(),
                                                    fill = fill.css(),
                                                    fg = ar.marker_lane_text.css(),
                                                ),
                                                "{m.idx} {m.name}"
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Time scale (loop band under everything, then ticks).
                        div {
                            style: format!(
                                "position:absolute; left:0; right:0; top:{scale_top}px; height:{scale_h}px;"
                            ),
                            if let Some(range) = loop_range {
                                {
                                    let (x, w) = span_px(range);
                                    rsx! { div { style: format!(
                                        "position:absolute; left:{x:.1}px; width:{w:.1}px; top:0; bottom:0; \
                                         background:{bg};", bg = ar.ruler_loop_bg.css()) } }
                                }
                            }
                            if let Some(range) = time_sel {
                                {
                                    let (x, w) = span_px(range);
                                    rsx! { div { style: format!(
                                        "position:absolute; left:{x:.1}px; width:{w:.1}px; top:0; bottom:0; \
                                         background:{bg};", bg = ar.ruler_sel_bg.css()) } }
                                }
                            }
                            // Minor ticks along the bottom edge.
                            if g.beat < g.major {
                                for i in 0..n_at(g.beat) {
                                    if (i as f64 * g.beat / g.major).fract() > 1e-9 {
                                        div {
                                            key: "n{i}",
                                            style: format!(
                                                "position:absolute; bottom:0; height:7px; left:{x:.1}px; \
                                                 width:1px; background:{ruler_fg2};",
                                                x = i as f64 * g.beat * pps,
                                            ),
                                        }
                                    }
                                }
                            }
                            // Major ticks + labels (bar numbers when musical).
                            for i in 0..n_at(g.major) {
                                div {
                                    key: "M{i}",
                                    style: format!(
                                        "position:absolute; top:0; bottom:0; left:{x:.1}px; \
                                         border-left:1px solid {ruler_fg2}; padding:2px 0 0 4px; \
                                         font-size:9px; color:{ruler_fg}; \
                                         font-variant-numeric:tabular-nums; white-space:nowrap;",
                                        x = i as f64 * g.major * pps,
                                    ),
                                    if g.musical {
                                        "{i as f64 * g.major * bpm.unwrap_or(120.0) / 60.0 / beats_per_measure.max(1) as f64 + 1.0:.0}"
                                    } else {
                                        "{time_label(i as f64 * g.major, g.major)}"
                                    }
                                }
                            }
                        }

                        // Tempo / time-signature lane (`ts_lane_*`).
                        if ts_lane_h > 0 {
                            div {
                                style: format!(
                                    "position:absolute; left:0; right:0; bottom:0; height:{ts_lane_h}px; \
                                     background:{bg};",
                                    bg = ar.ts_lane_bg.css(),
                                ),
                                for (i, t) in tempo_markers.iter().enumerate() {
                                    div {
                                        key: "t{i}",
                                        title: "Tempo {t.bpm} BPM, {t.num}/{t.den}",
                                        style: format!(
                                            "position:absolute; left:{x:.1}px; top:0; bottom:0; \
                                             border-left:2px solid {mark}; color:{fg}; \
                                             background:{mark_bg}; font-size:8px; font-weight:700; \
                                             padding:1px 4px 0 3px; white-space:nowrap;",
                                            x = t.time * pps,
                                            mark = ar.tsig.css(),
                                            mark_bg = ar.tsig.with_alpha(48).css(),
                                            fg = ar.ts_lane_text.css(),
                                        ),
                                        "{t.bpm:.0} {t.num}/{t.den}"
                                    }
                                }
                            }
                        }

                        // Cursors carry into the ruler, REAPER-style.
                        if let Some(t) = cursor {
                            div { style: format!(
                                "position:absolute; top:0; bottom:0; left:{x:.1}px; width:1px; \
                                 background:{edit_cursor};", x = t * pps) }
                        }
                        if let Some(pos) = playhead {
                            PlayCursor { pos, pps, color: play_cursor.clone() }
                        }
                    }
                }
            }

            // ── Body: TCP sidebar + lanes, sharing one vertical scroll ──
            // The inner row is content-height (`align-items:flex-start`), so
            // the lanes wrapper never overflows vertically — `overflow-x:auto`
            // would otherwise make it an independent vertical scroller too
            // (CSS computes overflow-y:auto alongside it) and the TCP and
            // lanes would scroll apart.
            div {
                // Vertical scroll is a signal too (`scroll_y`): the inner
                // TCP+lanes row is translated by `-scroll_y` under
                // `overflow:hidden`, so plain wheel + middle-drag can drive it.
                style: "flex:1 1 0; min-height:0; overflow:hidden;",
                onmounted: move |evt| {
                    spawn(async move {
                        if let Ok(rect) = evt.get_client_rect().await {
                            view_h.set(rect.size.height);
                        }
                    });
                },
                // Apply / release a track-height resize here (wraps both TCP and
                // lanes), so a drag started from EITHER the TCP row edge or the
                // lane bottom edge keeps tracking.
                onmousemove: move |evt: MouseEvent| {
                    if let Some(r) = *resize.peek() {
                        let mut h = r.height;
                        let dy = (evt.client_coordinates().y - r.start_y) / r.scale as f64;
                        h.set(((r.start_h as f64 + dy).round() as i64).clamp(16, 800) as u32);
                    }
                },
                onmouseup: move |_| {
                    let mut resize = resize;
                    resize.set(None);
                },
                onmouseleave: move |_| {
                    let mut resize = resize;
                    resize.set(None);
                },
                div {
                    // `min-height:100%` + `align-items:stretch` make the row (and
                    // its lanes scroller) fill the whole arrange viewport even
                    // when the tracks are shorter — so the empty area below the
                    // last track is still live arrange (wheel-zoom / click-seek
                    // work there) and the grid runs all the way down to the mixer.
                    style: format!(
                        "display:flex; align-items:stretch; position:relative; \
                         min-height:100%; top:{ty:.1}px;",
                        ty = -scroll_y(),
                    ),

                    TrackControlPanel { tracks: tracks.clone(), width: tcp_width, scroll: false, height_scale: rs, resize }

                // Timeline lanes. Horizontal position is driven by `scroll_x`
                // (single source of truth, shared with the ruler): the inner
                // content is translated by `-scroll_x` under `overflow:hidden`.
                // The wheel drives zoom + horizontal scroll; plain vertical
                // wheel falls through to the outer vertical scroller.
                div {
                    style: format!(
                        "flex:1 1 0; min-width:0; overflow:hidden; position:relative; \
                         background:{empty_bg};"
                    ),
                    // Measure the lanes viewport width so the grid + ruler draw
                    // out to the visible right edge.
                    onmounted: move |evt| {
                        spawn(async move {
                            if let Ok(rect) = evt.get_client_rect().await {
                                view_w.set(rect.size.width);
                            }
                        });
                    },
                    // Wheel gestures via the input keymap (the handler reads the
                    // raw delta since the processor drops it). Horizontal scroll
                    // is unbounded (scroll right forever); vertical scroll is
                    // clamped to the content height (`lanes_h - view_h`).
                    onwheel: move |evt: WheelEvent| {
                        let d = evt.data().delta().strip_units();
                        let max_sy = (lanes_h as f64 - view_h()).max(0.0);
                        for cmd in input.handle_wheel(&evt) {
                            let action = match &cmd {
                                InputCommand::Action(a) => Some(a.as_str()),
                                InputCommand::ActionWithArgs { action, .. } => Some(action.as_str()),
                                _ => None,
                            };
                            match action {
                                Some("view.vscroll") => {
                                    scroll_y.set((scroll_y() + d.y).clamp(0.0, max_sy));
                                }
                                Some("view.hscroll") => {
                                    scroll_x.set((scroll_x() + d.y + d.x).max(0.0));
                                }
                                Some("view.zoom_v") => {
                                    let f = if d.y < 0.0 { 1.1 } else { 1.0 / 1.1 };
                                    row_scale.set((row_scale() * f).clamp(0.3, 4.0));
                                }
                                Some("view.zoom_h") => {
                                    // Zoom about the pointer: keep the time under
                                    // the cursor fixed as `pps` changes.
                                    let cx = evt.data().client_coordinates().x;
                                    let t = ((cx - origin + scroll_x()) / pps).max(0.0);
                                    let factor = if d.y < 0.0 { 1.15 } else { 1.0 / 1.15 };
                                    let new_pps = (pps * factor).clamp(2.0, 400.0);
                                    zoom.set(new_pps);
                                    scroll_x.set((t * new_pps - (cx - origin)).max(0.0));
                                }
                                _ => {}
                            }
                        }
                        // Trackpad horizontal swipe (no modifier) → hscroll.
                        if d.x.abs() > d.y.abs() {
                            scroll_x.set((scroll_x() + d.x).max(0.0));
                        }
                    },
                    // Middle-mouse-button drag pans both axes (REAPER's pan).
                    onmousedown: move |evt: MouseEvent| {
                        if evt.trigger_button()
                            == Some(dioxus_elements::input_data::MouseButton::Auxiliary)
                        {
                            let c = evt.client_coordinates();
                            pan.set(Some((c.x, c.y, scroll_x(), scroll_y())));
                        }
                    },
                    // Pan (if middle-dragging) or advance an in-flight clip drag.
                    // Handlers live on the scroller so the gesture keeps tracking
                    // even when the pointer leaves the clip it grabbed.
                    onmousemove: move |evt: MouseEvent| {
                        let c = evt.client_coordinates();
                        // Track-height resize takes priority (a bottom-edge drag
                        // is in flight): new base = start_h + Δy/zoom, clamped.
                        if let Some(r) = *resize.peek() {
                            let mut h = r.height;
                            let dy = (c.y - r.start_y) / r.scale as f64;
                            h.set(((r.start_h as f64 + dy).round() as i64).clamp(16, 800) as u32);
                            return;
                        }
                        if let Some((sx0, sy0, scx, scy)) = *pan.peek() {
                            let max_sy = (lanes_h as f64 - view_h()).max(0.0);
                            scroll_x.set((scx - (c.x - sx0)).max(0.0));
                            scroll_y.set((scy - (c.y - sy0)).clamp(0.0, max_sy));
                            return;
                        }
                        let mut drag = drag;
                        let cur = *drag.peek();
                        if let Some(mut dd) = cur {
                            if (c.x - dd.start_x).abs() > 3.0 {
                                dd.moved = true;
                            }
                            dd.delta = (c.x - dd.start_x) / pps;
                            drag.set(Some(dd));
                        }
                    },
                    onmouseup: move |_| {
                        let mut resize = resize;
                        resize.set(None);
                        pan.set(None);
                        commit_drag();
                    },
                    onmouseleave: move |_| {
                        let mut resize = resize;
                        resize.set(None);
                        pan.set(None);
                        commit_drag();
                    },
                    // Translated content: `-scroll_x` pans it horizontally under
                    // the `overflow:hidden` scroller (blitz now honours
                    // `min-height:100%` here, so the grid fills the full height).
                    div {
                        // `min-width:100%` makes the arrange body (and every
                        // track-row lane inside it) fill the scroller's width
                        // even when the timeline content (`content_w`) is
                        // narrower, so empty arrange fills the panel instead of
                        // showing bare background; `width:content_w` still wins
                        // (→ horizontal scroll) once the timeline is longer.
                        // `min-width:100%`/`min-height:100%` fill the scroller so
                        // grid + lanes cover the whole arrange view; `left` pans
                        // it by the shared `scroll_x`.
                        style: format!(
                            "position:relative; width:{content_w}px; min-width:100%; \
                             min-height:100%; left:{sx:.1}px; background:{arrange_bg};",
                            sx = -scroll_x(),
                        ),
                        // Empty-timeline click: move the edit cursor and drop
                        // the clip selection. Clips stop propagation, so this
                        // only fires off-clip.
                        onmousedown: move |evt: MouseEvent| {
                            if evt.trigger_button()
                                != Some(dioxus_elements::input_data::MouseButton::Primary)
                            {
                                return;
                            }
                            emit(ArrangeEdit::Seek(seek_time(evt.client_coordinates().x)));
                            emit(ArrangeEdit::ClearSelection);
                        },
                        // Lane rows (the grid draws over them, like REAPER).
                        for (idx, track) in tracks.iter().enumerate() {
                            TrackLanes { key: "{track.id}", track: track.clone(), pps, alt: idx % 2 == 1 }
                        }

                        // Grid hierarchy over the tracks: sub, beat, measure.
                        if g.sub < g.beat {
                            for i in 0..n_at(g.sub) {
                                if (i as f64 * g.sub / g.beat).fract() > 1e-9 {
                                    div {
                                        key: "s{i}",
                                        style: format!(
                                            "position:absolute; top:0; bottom:0; left:{x:.1}px; \
                                             width:1px; background:{grid_sub}; pointer-events:none;",
                                            x = i as f64 * g.sub * pps,
                                        ),
                                    }
                                }
                            }
                        }
                        if g.beat < g.major {
                            for i in 0..n_at(g.beat) {
                                if (i as f64 * g.beat / g.major).fract() > 1e-9 {
                                    div {
                                        key: "b{i}",
                                        style: format!(
                                            "position:absolute; top:0; bottom:0; left:{x:.1}px; \
                                             width:1px; background:{grid_beat}; pointer-events:none;",
                                            x = i as f64 * g.beat * pps,
                                        ),
                                    }
                                }
                            }
                        }
                        for i in 0..n_at(g.major) {
                            div {
                                key: "g{i}",
                                style: format!(
                                    "position:absolute; top:0; bottom:0; left:{x:.1}px; \
                                     width:1px; background:{grid_measure}; pointer-events:none;",
                                    x = i as f64 * g.major * pps,
                                ),
                            }
                        }

                        // Time-selection shading over the arrange body.
                        if let Some(range) = time_sel {
                            {
                                let (x, w) = span_px(range);
                                rsx! { div { style: format!(
                                    "position:absolute; left:{x:.1}px; width:{w:.1}px; top:0; \
                                     height:{lanes_h}px; background:{bg}; pointer-events:none;",
                                    bg = ar.timesel.css()) } }
                            }
                        }

                        // Cursors over everything.
                        if let Some(t) = cursor {
                            div { style: format!(
                                "position:absolute; top:0; bottom:0; left:{x:.1}px; width:1px; \
                                 background:{edit_cursor}; pointer-events:none;", x = t * pps) }
                        }
                        if let Some(pos) = playhead {
                            PlayCursor { pos, pps, color: play_cursor.clone() }
                        }
                    }
                }
                }
            }
        }
    }
}

/// The moving play cursor — a leaf component so transport ticks re-render
/// one line, not the whole arrange view (123 lanes × 30 fps killed the CPU).
#[component]
fn PlayCursor(pos: Signal<f64>, pps: f64, color: String) -> Element {
    let x = pos() * pps;
    rsx! {
        div {
            style: format!(
                "position:absolute; top:0; bottom:0; left:{x:.1}px; width:1px; \
                 background:{color}; pointer-events:none;"
            ),
        }
    }
}

/// One track's arrange rows: the clip lane plus an envelope lane per visible
/// envelope (heights match the TCP side via [`TrackView::total_height`]).
#[component]
fn TrackLanes(track: TrackView, pps: f64, alt: bool) -> Element {
    let envelopes: Vec<EnvelopeView> = track
        .envelopes
        .iter()
        .filter(|e| e.visible)
        .cloned()
        .collect();
    rsx! {
        Lane { track: track.clone(), pps, alt }
        for (i, env) in envelopes.into_iter().enumerate() {
            EnvelopeLane { key: "e{i}", envelope: env, pps, alt }
        }
    }
}

/// One arrangement lane: alternating row background (`col_tr1/2_bg`,
/// `selcol_*` when the track is selected), the divider line, and the track's
/// items rendered REAPER-style (parity body + colour tint, peaks, label,
/// fades, selection + mute states).
#[component]
fn Lane(track: TrackView, pps: f64, alt: bool) -> Element {
    let theme = use_theme().theme;
    let ar = theme.arrange;
    let i = alt as usize;
    let selected = (track.selected)();
    let row_bg = if selected {
        ar.sel_row_bg[i].css()
    } else {
        ar.row_bg[i].css()
    };
    let divider = ar.row_divider[i].css();
    let item_edge = ar.item_edge.css();
    let track_color = track.color.as_deref().and_then(Color::hex);

    // Interaction: the shared drag state + edit callback (provided by
    // `ArrangeView`). `track_id` indexes the host's track list.
    let ctx = use_context::<ArrangeCtx>();
    let track_id = track.id;

    // Fixed item lanes (REAPER 7 comping). Display modes mirror
    // REAPER's lane-button cycle:
    // - One:   only the playing lane is shown, full height — other
    //          lanes' items don't render at all.
    // - Small: all lanes subdivide the normal row height.
    // - Big:   all lanes at full item height (the view-model already
    //          multiplied `track.height`, so the math here is the
    //          same as Small).
    let lanes_on = track.lane_count > 1;
    let one_lane = lanes_on && track.lane_display == LaneDisplay::One;
    let lane_count = if one_lane { 1 } else { track.lane_count.max(1) };
    // Displayed row height = the track's base-height Signal × the view's
    // vertical zoom. The bottom-edge handle resizes the base Signal.
    let rs = (ctx.row_scale)();
    let height_sig = track.height;
    let row_h = ((height_sig)() as f32 * rs).round().max(8.0) as u32;
    let lane_area_h = row_h.saturating_sub(4) as f64;
    let lane_h = lane_area_h / lane_count as f64;
    let item_h = if lane_count > 1 {
        (lane_h - 1.0).max(4.0)
    } else {
        lane_area_h
    };
    let sk = theme.arrange_skin.clone();

    rsx! {
        div {
            style: format!(
                "position:relative; height:{row_h}px; background:{row_bg}; \
                 border-bottom:1px solid {divider}; box-sizing:border-box;",
            ),
            // Bottom-edge resize handle: drag to set this track's height. Begins
            // the resize (snapshot base height + pointer y + zoom); the scroller's
            // shared onmousemove/up apply + release it (slip_drag shape). The
            // `fts-rz` class brightens it on hover so it's discoverable.
            div {
                class: "fts-rz",
                style: "position:absolute; left:0; right:0; bottom:0; height:7px; \
                        cursor:ns-resize; z-index:5; background:rgba(255,255,255,0.05);",
                onmousedown: move |evt: MouseEvent| {
                    if evt.trigger_button()
                        == Some(dioxus_elements::input_data::MouseButton::Primary)
                    {
                        evt.stop_propagation();
                        let mut resize = ctx.resize;
                        resize.set(Some(ResizeState {
                            height: height_sig,
                            start_y: evt.client_coordinates().y,
                            start_h: (height_sig)(),
                            scale: rs.max(0.05),
                        }));
                    }
                },
            }
            for (ci, clip) in track.clips.iter().enumerate() {
                {
                    // Item body: the item/track colour tinted over the parity
                    // background at `itembg_drawmode` strength; selected items
                    // switch to the `itembgsel` body.
                    let color = clip.color.as_deref().and_then(Color::hex).or(track_color);
                    let body = if clip.selected {
                        ar.item_bg_sel[i]
                    } else {
                        match color {
                            Some(c) => ar.item_bg[i].mix(c, ar.item_blend),
                            None => ar.item_bg[i],
                        }
                    };
                    let label = if clip.selected { ar.item_label_sel } else { ar.item_label };
                    // Optimistic geometry: while this clip is being dragged,
                    // render at the live position/length from the drag state.
                    let (clip_start, clip_len) = match *ctx.drag.read() {
                        Some(d) if d.track == track_id && d.clip == ci => d.applied(ctx.min_len),
                        _ => (clip.start, clip.length),
                    };
                    let dragging = matches!(*ctx.drag.peek(),
                        Some(d) if d.track == track_id && d.clip == ci);
                    let x = clip_start * pps;
                    let w = (clip_len * pps).max(2.0);
                    // Origin geometry for a drag started on this clip (Copy
                    // locals — event handlers must be `'static`, no `&clip`).
                    let c_start = clip.start;
                    let c_len = clip.length;
                    // Lane geometry + playing state for this item.
                    let lane = clip.lane.unwrap_or(0);
                    let lane_playing = !lanes_on
                        || track.lane_play_mask == 0
                        || (lane < 64 && track.lane_play_mask & (1u64 << lane) != 0);
                    // "Show one lane": non-playing lanes are collapsed
                    // away entirely; the playing lane fills the row.
                    let hidden = one_lane && !lane_playing;
                    let slot = if one_lane { 0 } else { lane.min(lane_count - 1) };
                    let top = 2.0 + slot as f64 * lane_h;
                    let fade_in_w = (clip.fade_in * pps).min(w);
                    let fade_out_w = (clip.fade_out * pps).min(w);
                    // Waveform peaks: REAPER's asymmetric model — the top
                    // boundary follows each column's max, the bottom its min,
                    // around the zero line at the lane's vertical centre.
                    // Stereo sources draw split L/R half-lanes
                    // (REAPER's stereo item view).
                    let wave_poly = |peaks: &[(f32, f32)], mid: f64, half: f64| -> String {
                        let n = peaks.len().max(2) as f64;
                        let step = w / (n - 1.0);
                        let mut top = String::new();
                        let mut bottom = String::new();
                        for (pi, (pmax, pmin)) in peaks.iter().enumerate() {
                            let px = pi as f64 * step;
                            let up = (*pmax as f64).clamp(-1.0, 1.0) * half;
                            let dn = (*pmin as f64).clamp(-1.0, 1.0) * half;
                            top.push_str(&format!("{px:.1},{:.1} ", mid - up));
                            bottom.insert_str(0, &format!("{px:.1},{:.1} ", mid - dn));
                        }
                        format!("{top}{bottom}")
                    };
                    let wave_polys: Vec<String> = if !clip.peaks.is_empty()
                        && !clip.peaks_right.is_empty()
                        && item_h >= 12.0
                    {
                        let half = item_h * 0.25 - 1.0;
                        vec![
                            wave_poly(&clip.peaks, item_h * 0.25, half),
                            wave_poly(&clip.peaks_right, item_h * 0.75, half),
                        ]
                    } else if !clip.peaks.is_empty() {
                        vec![wave_poly(&clip.peaks, item_h / 2.0, item_h / 2.0 - 1.0)]
                    } else {
                        Vec::new()
                    };
                    rsx! {
                        if !hidden {
                        div {
                            key: "c{ci}",
                            title: "{clip.name}",
                            style: format!(
                                "position:absolute; top:{top:.1}px; height:{item_h}px; left:{x:.1}px; \
                                 width:{w:.1}px; background:{body}; border:1px solid {item_edge}; \
                                 border-radius:3px; overflow:hidden; box-sizing:border-box; \
                                 font-size:10px; color:{fg}; font-weight:700; cursor:grab; \
                                 white-space:nowrap; text-overflow:ellipsis;{z}{dim}",
                                body = body.css(),
                                fg = label.css(),
                                z = if dragging { " z-index:5;" } else { "" },
                                // REAPER greys out non-playing lanes —
                                // they're alternate takes, not audible.
                                dim = if lane_playing { "" } else { " opacity:0.35;" },
                            ),
                            // Body drag = move; also selects the clip.
                            onmousedown: move |evt: MouseEvent| {
                                evt.stop_propagation();
                                let additive = evt.modifiers().shift() || evt.modifiers().ctrl();
                                if let Some(cb) = ctx.on_edit {
                                    cb.call(ArrangeEdit::SelectClip { track: track_id, clip: ci, additive });
                                }
                                let mut drag = ctx.drag;
                                drag.set(Some(DragState {
                                    track: track_id,
                                    clip: ci,
                                    mode: DragMode::Move,
                                    start_x: evt.client_coordinates().x,
                                    orig_start: c_start,
                                    orig_len: c_len,
                                    delta: 0.0,
                                    moved: false,
                                }));
                            },

                            // Trim handles: thin grab zones on each edge.
                            div {
                                style: format!(
                                    "position:absolute; left:0; top:0; width:{e:.0}px; height:100%; \
                                     cursor:col-resize; z-index:3;",
                                    e = ctx.edge_px,
                                ),
                                onmousedown: move |evt: MouseEvent| {
                                    evt.stop_propagation();
                                    if let Some(cb) = ctx.on_edit {
                                        cb.call(ArrangeEdit::SelectClip { track: track_id, clip: ci, additive: false });
                                    }
                                    let mut drag = ctx.drag;
                                    drag.set(Some(DragState {
                                        track: track_id, clip: ci, mode: DragMode::ResizeStart,
                                        start_x: evt.client_coordinates().x,
                                        orig_start: c_start, orig_len: c_len, delta: 0.0, moved: false,
                                    }));
                                },
                            }
                            div {
                                style: format!(
                                    "position:absolute; right:0; top:0; width:{e:.0}px; height:100%; \
                                     cursor:col-resize; z-index:3;",
                                    e = ctx.edge_px,
                                ),
                                onmousedown: move |evt: MouseEvent| {
                                    evt.stop_propagation();
                                    if let Some(cb) = ctx.on_edit {
                                        cb.call(ArrangeEdit::SelectClip { track: track_id, clip: ci, additive: false });
                                    }
                                    let mut drag = ctx.drag;
                                    drag.set(Some(DragState {
                                        track: track_id, clip: ci, mode: DragMode::ResizeEnd,
                                        start_x: evt.client_coordinates().x,
                                        orig_start: c_start, orig_len: c_len, delta: 0.0, moved: false,
                                    }));
                                },
                            }

                            // Peaks under the label (`col_tr1/2_peaks`);
                            // one polygon per channel lane.
                            if !wave_polys.is_empty() {
                                svg {
                                    width: "{w:.0}",
                                    height: "{item_h:.0}",
                                    style: "position:absolute; left:0; top:0; pointer-events:none;",
                                    for (wi, points) in wave_polys.into_iter().enumerate() {
                                        polygon {
                                            key: "w{wi}",
                                            points,
                                            fill: ar.peaks[i].css(),
                                        }
                                    }
                                }
                            }

                            div { style: "position:relative; padding:1px 5px; pointer-events:none;", "{clip.name}" }

                            // Fade triangles (`fadezone` fill + `col_mi_fades` line).
                            if fade_in_w >= 2.0 {
                                svg {
                                    width: "{fade_in_w:.0}",
                                    height: "{item_h:.0}",
                                    style: "position:absolute; left:0; top:0; pointer-events:none;",
                                    polygon {
                                        points: format!("0,0 {fade_in_w:.1},0 0,{item_h:.1}"),
                                        fill: ar.fadezone.css(),
                                    }
                                    line {
                                        x1: "0", y1: "{item_h:.1}", x2: "{fade_in_w:.1}", y2: "0",
                                        stroke: ar.fade_line.css(),
                                        stroke_width: "1",
                                    }
                                }
                            }
                            if fade_out_w >= 2.0 {
                                svg {
                                    width: "{fade_out_w:.0}",
                                    height: "{item_h:.0}",
                                    style: format!(
                                        "position:absolute; left:{x:.1}px; top:0; pointer-events:none;",
                                        x = w - fade_out_w - 2.0,
                                    ),
                                    polygon {
                                        points: format!("0,0 {fade_out_w:.1},0 {fade_out_w:.1},{item_h:.1}"),
                                        fill: ar.fadezone.css(),
                                    }
                                    line {
                                        x1: "0", y1: "0", x2: "{fade_out_w:.1}", y2: "{item_h:.1}",
                                        stroke: ar.fade_line.css(),
                                        stroke_width: "1",
                                    }
                                }
                            }

                            // Selected-item tag bar (when the theme enables it).
                            if clip.selected && ar.selitem_tag.is_some() {
                                div { style: format!(
                                    "position:absolute; left:0; right:0; bottom:0; height:3px; \
                                     background:{c}; pointer-events:none;",
                                    c = color.unwrap_or(ar.selitem_tag.unwrap()).css()) }
                            }

                            // Mute overlay.
                            if clip.muted {
                                div { style: format!(
                                    "position:absolute; inset:0; background:{c}; pointer-events:none;",
                                    c = ar.mute_overlay.css()) }
                            }
                        }
                        }
                    }
                }
            }

            // Fixed-lane chrome: separator lines between lanes + the
            // lane-name chip at each lane's left edge (REAPER 7 style).
            if lane_count > 1 {
                for ln in 1..lane_count {
                    div {
                        key: "ls{ln}",
                        style: format!(
                            "position:absolute; left:0; right:0; top:{y:.1}px; height:1px; \
                             background:{divider}; opacity:0.6; pointer-events:none;",
                            y = 2.0 + ln as f64 * lane_h,
                        ),
                    }
                }
                for ln in 0..lane_count {
                    {
                        let name = track
                            .lane_names
                            .get(ln as usize)
                            .cloned()
                            .unwrap_or_else(|| (ln + 1).to_string());
                        let playing = track.lane_play_mask == 0
                            || (ln < 64 && track.lane_play_mask & (1u64 << ln) != 0);
                        let chip_h = (lane_h - 1.0).max(4.0);
                        // Lane-play button art: REAPER's `lane_solo_*`
                        // strips — the full button when the lane is
                        // tall enough (big lanes), the tiny
                        // `*_indicator` variant on small lanes.
                        let art = if track.lane_display == LaneDisplay::Big && chip_h >= 18.0 {
                            if playing { sk.lane_solo_on.as_ref() } else { sk.lane_solo_off.as_ref() }
                        } else if playing {
                            sk.lane_solo_on_indicator.as_ref()
                        } else {
                            sk.lane_solo_off_indicator.as_ref()
                        };
                        rsx! {
                            div {
                                key: "ln{ln}",
                                style: format!(
                                    "position:absolute; left:0; top:{y:.1}px; height:{h:.1}px; \
                                     display:flex; align-items:center; gap:2px; padding:0 3px; \
                                     font-size:8px; color:{fg}; background:{bg}; \
                                     border-right:1px solid {divider}; \
                                     border-bottom:1px solid {divider}; \
                                     opacity:{op}; pointer-events:none;",
                                    y = 2.0 + ln as f64 * lane_h,
                                    h = chip_h,
                                    fg = ar.item_label.css(),
                                    bg = ar.row_bg[i].css(),
                                    op = if playing { "0.9" } else { "0.4" },
                                ),
                                if let Some(img) = art {
                                    img {
                                        src: "{img.url}",
                                        style: format!(
                                            "width:{w}px; height:{h}px; flex:0 0 auto;",
                                            w = img.w.min(chip_h as u32 * img.w / img.h.max(1)),
                                            h = img.h.min(chip_h as u32),
                                        ),
                                    }
                                }
                                "{name}"
                            }
                        }
                    }
                }
            }

            // "Show one lane" tab: a single chip naming the playing
            // lane (REAPER shows which comp lane you're hearing).
            if one_lane {
                {
                    let playing_lane = (0..track.lane_count)
                        .find(|ln| {
                            track.lane_play_mask == 0
                                || (*ln < 64 && track.lane_play_mask & (1u64 << *ln) != 0)
                        })
                        .unwrap_or(0);
                    let name = track
                        .lane_names
                        .get(playing_lane as usize)
                        .cloned()
                        .unwrap_or_else(|| (playing_lane + 1).to_string());
                    let art = sk.lane_solo_on_indicator.as_ref();
                    rsx! {
                        div {
                            style: format!(
                                "position:absolute; left:0; top:2px; height:12px; \
                                 display:flex; align-items:center; gap:2px; padding:0 3px; \
                                 font-size:8px; color:{fg}; background:{bg}; \
                                 border-right:1px solid {divider}; \
                                 border-bottom:1px solid {divider}; \
                                 opacity:0.9; pointer-events:none;",
                                fg = ar.item_label.css(),
                                bg = ar.row_bg[i].css(),
                            ),
                            if let Some(img) = art {
                                img {
                                    src: "{img.url}",
                                    style: format!(
                                        "width:{w}px; height:{h}px; flex:0 0 auto;",
                                        w = img.w,
                                        h = img.h,
                                    ),
                                }
                            }
                            "{name}"
                        }
                    }
                }
            }
        }
    }
}

/// One envelope lane: a dimmed row with the envelope curve drawn over it
/// (`col_env*` colour, filled below the line REAPER-style).
#[component]
fn EnvelopeLane(envelope: EnvelopeView, pps: f64, alt: bool) -> Element {
    let theme = use_theme().theme;
    let ar = theme.arrange;
    let i = alt as usize;
    // Scale with the view's vertical zoom so the lane lines up with its TCP row.
    let rs = (use_context::<ArrangeCtx>().row_scale)();
    let h = ((envelope.height as f32 * rs).round() as u32).max(6);
    let curve = envelope
        .color
        .as_deref()
        .and_then(Color::hex)
        .unwrap_or(ar.env_default);

    // Curve polyline + the area fill beneath it.
    let inner_h = h.saturating_sub(2) as f64;
    let xy =
        |t: f64, v: f32| -> (f64, f64) { (t * pps, (1.0 - v.clamp(0.0, 1.0)) as f64 * inner_h) };
    let mut line_pts = String::new();
    for (t, v) in &envelope.points {
        let (x, y) = xy(*t, *v);
        line_pts.push_str(&format!("{x:.1},{y:.1} "));
    }
    // Close the curve down to the lane floor for the area fill.
    let fill_pts = match (envelope.points.first(), envelope.points.last()) {
        (Some(first), Some(last)) => {
            let (fx, _) = xy(first.0, first.1);
            let (lx, _) = xy(last.0, last.1);
            format!("{line_pts}{lx:.1},{inner_h:.1} {fx:.1},{inner_h:.1}")
        }
        _ => String::new(),
    };
    let svg_w = envelope
        .points
        .last()
        .map(|p| (p.0 * pps).ceil().max(2.0))
        .unwrap_or(2.0);

    rsx! {
        div {
            style: format!(
                "position:relative; height:{h}px; background:{bg}; \
                 border-bottom:1px solid {divider}; box-sizing:border-box; overflow:hidden;",
                bg = ar.row_bg[i].darken(0.25).css(),
                divider = ar.envlane_divider[i].css(),
            ),
            if !envelope.points.is_empty() {
                svg {
                    width: "{svg_w:.0}",
                    height: "{inner_h:.0}",
                    style: "position:absolute; left:0; top:1px; pointer-events:none;",
                    if !fill_pts.is_empty() {
                        polygon { points: fill_pts, fill: curve.with_alpha(40).css() }
                    }
                    polyline {
                        points: line_pts,
                        fill: "none",
                        stroke: curve.css(),
                        stroke_width: "1.5",
                    }
                    // Envelope points, REAPER-style square handles.
                    for (pi, (t, v)) in envelope.points.iter().enumerate() {
                        {
                            let (x, y) = xy(*t, *v);
                            rsx! {
                                rect {
                                    key: "p{pi}",
                                    x: "{x - 2.0:.1}",
                                    y: "{y - 2.0:.1}",
                                    width: "4",
                                    height: "4",
                                    fill: curve.css(),
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod drag_tests {
    use super::*;

    fn d(mode: DragMode, orig_start: f64, orig_len: f64, delta: f64) -> DragState {
        DragState {
            track: 0,
            clip: 0,
            mode,
            start_x: 0.0,
            orig_start,
            orig_len,
            delta,
            moved: true,
        }
    }

    #[test]
    fn move_shifts_start_keeps_length() {
        let (s, l) = d(DragMode::Move, 4.0, 8.0, 2.5).applied(0.05);
        assert!((s - 6.5).abs() < 1e-9);
        assert!((l - 8.0).abs() < 1e-9);
    }

    #[test]
    fn move_clamps_start_at_zero() {
        let (s, l) = d(DragMode::Move, 1.0, 8.0, -5.0).applied(0.05);
        assert_eq!(s, 0.0);
        assert!((l - 8.0).abs() < 1e-9);
    }

    #[test]
    fn resize_end_grows_length_keeps_start() {
        let (s, l) = d(DragMode::ResizeEnd, 4.0, 8.0, 3.0).applied(0.05);
        assert!((s - 4.0).abs() < 1e-9);
        assert!((l - 11.0).abs() < 1e-9);
    }

    #[test]
    fn resize_end_floors_at_min_len() {
        let (_, l) = d(DragMode::ResizeEnd, 4.0, 8.0, -100.0).applied(0.05);
        assert!((l - 0.05).abs() < 1e-9);
    }

    #[test]
    fn resize_start_moves_left_edge_keeps_end() {
        // end = 12.0; drag left edge right by 2s → start 6, len 6.
        let (s, l) = d(DragMode::ResizeStart, 4.0, 8.0, 2.0).applied(0.05);
        assert!((s - 6.0).abs() < 1e-9);
        assert!((l - 6.0).abs() < 1e-9);
        assert!((s + l - 12.0).abs() < 1e-9, "end stays put");
    }

    #[test]
    fn resize_start_cannot_cross_end() {
        // Dragging the left edge past the end clamps to end - min_len.
        let (s, l) = d(DragMode::ResizeStart, 4.0, 8.0, 100.0).applied(0.05);
        assert!((s - (12.0 - 0.05)).abs() < 1e-9);
        assert!((l - 0.05).abs() < 1e-9);
    }
}

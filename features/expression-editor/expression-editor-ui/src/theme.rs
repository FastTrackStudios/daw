//! Colors and inline-style helpers.
//!
//! Every style in this crate is inline. Blitz does not load external
//! stylesheets reliably, and the same components have to render
//! identically standalone, as a VST3/CLAP editor, and in the browser —
//! so there is nowhere to put a stylesheet that all three would agree
//! on. Tailwind classes may be added on top, but never depended on.

/// Canvas background.
pub const BG: &str = "#0d0d11";
/// Piano-roll white-key row.
pub const ROW_WHITE: &str = "#1d1d26";
/// Piano-roll black-key row.
pub const ROW_BLACK: &str = "#131319";
/// Octave (C) divider.
pub const OCTAVE_LINE: &str = "#3a3a4d";
/// Beat gridline.
pub const GRID_BEAT: &str = "#2b2b3a";
/// Subdivision gridline.
pub const GRID_SUB: &str = "#20202a";

pub const PANEL: &str = "#15151c";
pub const PANEL_BORDER: &str = "#2b2b38";
pub const TEXT: &str = "#c8cede";
pub const TEXT_DIM: &str = "#7b8397";
pub const ACCENT: &str = "#38bdf8";
pub const SELECTED: &str = "#f0f9ff";

/// Q-zone structure and the ambiguity warning share red on purpose:
/// both mean "this region has boundaries you must be aware of".
pub const ZONE: &str = "#ef4444";
/// Microtonal centers and off-ET targets.
pub const GOLD: &str = "#eab308";
/// Transport position.
pub const PLAYHEAD: &str = "#f8fafc";
/// Piano-key gutter.
pub const KEY_WHITE: &str = "#d8dce6";
pub const KEY_BLACK: &str = "#26262f";
pub const GUTTER_BG: &str = "#101016";
/// Razor areas. Distinct from ZONE red — a razor is a region you are
/// about to operate on, not a warning.
pub const RAZOR: &str = "#22d3ee";

/// Twelve pitch-class hues. Notes are colored by pitch class so a
/// melodic shape is readable at a glance without reading the piano
/// keys; selection adds a bright outline rather than a fill change, so
/// the hue survives selection.
pub const PITCH_CLASS: [&str; 12] = [
    "#ef4444", // C
    "#f97316", // C#
    "#f59e0b", // D
    "#eab308", // D#
    "#84cc16", // E
    "#22c55e", // F
    "#14b8a6", // F#
    "#06b6d4", // G
    "#3b82f6", // G#
    "#6366f1", // A
    "#a855f7", // A#
    "#ec4899", // B
];

pub fn pitch_class_color(row: i32) -> &'static str {
    PITCH_CLASS[row.rem_euclid(12) as usize]
}

/// Per-lane curve color.
pub fn lane_color(lane: expression_editor_core::Lane) -> &'static str {
    use expression_editor_core::Lane;
    match lane {
        Lane::Pitch => "#7dd3fc",
        Lane::Pressure => "#fda4af",
        Lane::Timbre => "#a7f3d0",
    }
}

pub fn lane_label(lane: expression_editor_core::Lane) -> &'static str {
    use expression_editor_core::Lane;
    match lane {
        Lane::Pitch => "Pitch",
        Lane::Pressure => "Pressure",
        Lane::Timbre => "Timbre",
    }
}

pub fn is_black_key(row: i32) -> bool {
    matches!(row.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

/// A toolbar button, active or not.
pub fn button_style(active: bool) -> String {
    let (bg, border, fg) = if active {
        ("#1e3a5f", ACCENT, SELECTED)
    } else {
        ("#1c1c25", PANEL_BORDER, TEXT)
    };
    format!(
        "display: flex; align-items: center; justify-content: center; \
         gap: 4px; min-width: 26px; height: 26px; padding: 0 7px; \
         background: {bg}; border: 1px solid {border}; border-radius: 5px; \
         color: {fg}; font-size: 11px; line-height: 1; cursor: pointer; \
         user-select: none;"
    )
}

/// A labelled group in the toolbar.
pub fn group_style() -> String {
    format!(
        "display: flex; align-items: center; gap: 4px; padding: 0 8px; \
         border-right: 1px solid {PANEL_BORDER};"
    )
}

pub fn group_label_style() -> String {
    format!(
        "color: {TEXT_DIM}; font-size: 9px; letter-spacing: 0.08em; \
         text-transform: uppercase; margin-right: 2px; user-select: none;"
    )
}

pub fn select_style() -> String {
    format!(
        "height: 26px; background: #1c1c25; border: 1px solid {PANEL_BORDER}; \
         border-radius: 5px; color: {TEXT}; font-size: 11px; padding: 0 6px;"
    )
}

/// Mode icons, as inline SVG path data on a 16×16 grid.
///
/// Drawn rather than typed: the glyphs a font would give us
/// (🎸 🥁 🎤) render inconsistently across Blitz, wasm and a plugin
/// window, and emoji colour fights the bar's palette. These inherit
/// `currentColor`, so an active mode's icon lights with its label.
pub fn mode_icon(mode: expression_editor_core::Mode) -> &'static str {
    use expression_editor_core::Mode;
    match mode {
        // Piano keys.
        Mode::Midi => "M1 4h14v8H1z M4.5 4v5 M7 4v5 M10.5 4v5 M13 4v5",
        // Keys with a bend arrow over them — per-note expression.
        Mode::Mpe => "M1 7h14v5H1z M4.5 7v3 M7 7v3 M10.5 7v3 M13 7v3 \
                      M2 4.5c3-3 5 1 7-1s3-1 4 0",
        // Kick drum: a circle with lugs and a beater line.
        Mode::Drums => "M8 2.5a5.5 5.5 0 100 11 5.5 5.5 0 100-11z \
                        M8 5.5a2.5 2.5 0 100 5 2.5 2.5 0 100-5z \
                        M2.6 5.3l2.2 1 M13.4 5.3l-2.2 1 \
                        M4.8 12.6l1-2.2 M11.2 12.6l-1-2.2",
        // Guitar: body, neck, and strings.
        Mode::Guitar => "M5 14a3 3 0 100-6 3 3 0 100 6z M6.6 9.6l6-6 \
                         M11.4 2.2l2.4 2.4 M12.2 5.4l-1.6-1.6",
        // Microphone.
        Mode::Vocals => "M8 2a2 2 0 012 2v4a2 2 0 01-4 0V4a2 2 0 012-2z \
                         M4.5 7.5a3.5 3.5 0 007 0 M8 11v3 M6 14h4",
        // Waveform.
        Mode::Audio => "M1 8h1.5 M2.5 8v0 M3.5 5v6 M5.5 3v10 M7.5 6v4 \
                        M9.5 2v12 M11.5 5v6 M13.5 7v2 M15 8h0",
    }
}

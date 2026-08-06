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
pub const ROW_WHITE: &str = "#17171e";
/// Piano-roll black-key row.
pub const ROW_BLACK: &str = "#111117";
/// Octave (C) divider.
pub const OCTAVE_LINE: &str = "#2f2f3d";
/// Beat gridline.
pub const GRID_BEAT: &str = "#23232e";
/// Subdivision gridline.
pub const GRID_SUB: &str = "#1a1a23";

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

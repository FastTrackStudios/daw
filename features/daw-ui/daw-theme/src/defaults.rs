//! The authored FastTrackStudio palette, as hex.
//!
//! **This module is the source of truth.** [`crate::Theme::default`] parses
//! these, and consumers that need compile-time `&'static str` colours — the
//! expression editor renders every style inline, because Blitz will not load
//! a stylesheet reliably — use them directly. Change a colour here and both
//! the panels and the generated REAPER theme follow.
//!
//! Hex rather than [`crate::Color`] because a `const` can't call a parser,
//! and because these strings are what a designer reads and edits.

// ── chrome ───────────────────────────────────────────────────────────────

/// The window behind everything.
pub const SURFACE: &str = "#0d0d11";
/// A panel or strip sitting on `SURFACE`.
pub const SURFACE_RAISED: &str = "#15151c";
/// Wells, lanes and troughs cut into a panel.
pub const SURFACE_SUNKEN: &str = "#101016";
/// Hairlines between regions.
pub const BORDER: &str = "#2b2b38";
/// Primary label text.
pub const TEXT: &str = "#c8cede";
/// Secondary text — units, inactive labels.
pub const TEXT_DIM: &str = "#7b8397";
/// Tertiary text — watermark level.
pub const TEXT_FAINT: &str = "#4a5062";
/// The one colour meaning "live / selected / yours".
pub const ACCENT: &str = "#38bdf8";
/// A control's resting surface — buttons, selects, entry fields.
pub const CONTROL: &str = "#1c1c25";
/// A control that is engaged: the resting surface pulled toward the accent.
pub const CONTROL_ACTIVE: &str = "#1e3a5f";
/// An inset well inside a panel — one step below `SURFACE_RAISED`.
pub const SURFACE_INSET: &str = "#1a1a22";
/// A divider or handle that needs to read above `BORDER`.
pub const BORDER_STRONG: &str = "#4a4a58";
/// Emphasised text, above `TEXT`.
pub const TEXT_BRIGHT: &str = "#cfd6e4";
/// Selection highlight, brighter than the accent.
pub const SELECTED: &str = "#f0f9ff";

// ── signal ───────────────────────────────────────────────────────────────

pub const METER_SAFE: &str = "#22c55e";
pub const METER_WARN: &str = "#eab308";
pub const METER_DANGER: &str = "#ef4444";
pub const SOLO: &str = "#eab308";
pub const MUTE: &str = "#ef4444";
pub const REC: &str = "#e13a53";
/// Waveform / peak body.
pub const PEAKS: &str = "#5b8def";
/// Playhead and edit cursor — the same idea on both surfaces.
pub const PLAYHEAD: &str = "#f8fafc";
/// A track with no colour assigned.
pub const NEUTRAL_TRACK: &str = "#828282";

// ── editor ───────────────────────────────────────────────────────────────

/// White-key row in the piano roll.
pub const ROW_WHITE: &str = "#1d1d26";
/// Black-key row.
pub const ROW_BLACK: &str = "#131319";
/// The octave (C) divider — heavier than a beat line.
pub const OCTAVE_LINE: &str = "#3a3a4d";
/// Beat gridline.
pub const GRID_BEAT: &str = "#2b2b3a";
/// Subdivision gridline.
pub const GRID_SUB: &str = "#20202a";
/// Piano-key gutter background.
pub const GUTTER: &str = "#101016";
pub const KEY_WHITE: &str = "#d8dce6";
pub const KEY_BLACK: &str = "#26262f";
/// Structural zones and ambiguity warnings — both mean "there are
/// boundaries here you must be aware of".
pub const ZONE: &str = "#ef4444";
/// Microtonal centres and off-ET targets.
pub const MICROTONAL: &str = "#eab308";
/// Razor areas. Deliberately distinct from [`ZONE`]: a razor is a region you
/// are about to operate on, not a warning about one.
pub const RAZOR: &str = "#22d3ee";

/// Twelve pitch-class hues, C first.
///
/// Notes are coloured by pitch class so a melodic shape is readable without
/// reading the keys. Selection adds an outline rather than changing the fill,
/// so the hue survives being selected.
pub const PITCH_CLASSES: [&str; 12] = [
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

// ── per-lane expression curves ───────────────────────────────────────────

pub const LANE_PITCH: &str = "#7dd3fc";
pub const LANE_PRESSURE: &str = "#fda4af";
pub const LANE_TIMBRE: &str = "#a7f3d0";

// ── metrics ──────────────────────────────────────────────────────────────

/// Corner radius, px.
pub const RADIUS: f32 = 5.0;
/// How far a panel surface blends toward its track colour (0–1).
pub const TRACK_TINT: f32 = 0.12;
/// Extra dim on unselected surfaces (0–1).
pub const DIM_UNSELECTED: f32 = 0.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    /// Every colour in this module, so a malformed one can't reach a
    /// consumer as a silently-broken CSS string.
    fn all() -> Vec<(&'static str, &'static str)> {
        let mut v = vec![
            ("SURFACE", SURFACE),
            ("SURFACE_RAISED", SURFACE_RAISED),
            ("SURFACE_SUNKEN", SURFACE_SUNKEN),
            ("BORDER", BORDER),
            ("TEXT", TEXT),
            ("TEXT_DIM", TEXT_DIM),
            ("TEXT_FAINT", TEXT_FAINT),
            ("ACCENT", ACCENT),
            ("CONTROL", CONTROL),
            ("CONTROL_ACTIVE", CONTROL_ACTIVE),
            ("SURFACE_INSET", SURFACE_INSET),
            ("BORDER_STRONG", BORDER_STRONG),
            ("TEXT_BRIGHT", TEXT_BRIGHT),
            ("SELECTED", SELECTED),
            ("METER_SAFE", METER_SAFE),
            ("METER_WARN", METER_WARN),
            ("METER_DANGER", METER_DANGER),
            ("SOLO", SOLO),
            ("MUTE", MUTE),
            ("REC", REC),
            ("PEAKS", PEAKS),
            ("PLAYHEAD", PLAYHEAD),
            ("NEUTRAL_TRACK", NEUTRAL_TRACK),
            ("ROW_WHITE", ROW_WHITE),
            ("ROW_BLACK", ROW_BLACK),
            ("OCTAVE_LINE", OCTAVE_LINE),
            ("GRID_BEAT", GRID_BEAT),
            ("GRID_SUB", GRID_SUB),
            ("GUTTER", GUTTER),
            ("KEY_WHITE", KEY_WHITE),
            ("KEY_BLACK", KEY_BLACK),
            ("ZONE", ZONE),
            ("MICROTONAL", MICROTONAL),
            ("RAZOR", RAZOR),
            ("LANE_PITCH", LANE_PITCH),
            ("LANE_PRESSURE", LANE_PRESSURE),
            ("LANE_TIMBRE", LANE_TIMBRE),
        ];
        for (i, p) in PITCH_CLASSES.iter().enumerate() {
            v.push((Box::leak(format!("PITCH_CLASSES[{i}]").into_boxed_str()), p));
        }
        v
    }

    #[test]
    fn every_default_is_parseable_hex() {
        for (name, hex) in all() {
            assert!(
                Color::hex(hex).is_some(),
                "{name} = {hex:?} is not valid hex"
            );
        }
    }

    #[test]
    fn pitch_classes_are_all_distinct() {
        // Two identical hues means two pitch classes are indistinguishable,
        // which defeats colouring notes by pitch class at all.
        let mut seen = PITCH_CLASSES.to_vec();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "duplicate pitch-class hues");
    }

    #[test]
    fn row_colours_are_dark_enough_to_sit_under_notes() {
        // Piano-roll rows are a backdrop; if one drifts bright, every note
        // on it loses contrast.
        for row in [ROW_WHITE, ROW_BLACK, GUTTER] {
            let l = Color::hex(row).unwrap().luminance();
            assert!(l < 0.1, "{row} is too light for a row backdrop ({l})");
        }
    }

    #[test]
    fn zone_and_razor_are_not_the_same_colour() {
        // They mean different things — a warning versus a pending edit — and
        // the code comments say so; keep them visually separable.
        assert_ne!(ZONE, RAZOR);
    }
}

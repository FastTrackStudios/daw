//! The authored palette — one vocabulary for every FTS surface.
//!
//! Three groups, because they answer to different masters:
//!
//! - [`Chrome`] — surfaces, text, borders, accent. Everything has these, and
//!   they are what makes two panels look like the same product.
//! - [`Signal`] — meters, transport and track states. REAPER owns most of
//!   these; the panels mirror them so a mute button reads the same in both.
//! - [`Editor`] — the note/curve surface: pitch classes, gridlines,
//!   playhead, zones, razor. **REAPER has no palette slot for any of it**,
//!   which is precisely why the canonical theme cannot be a REAPER theme.
//!
//! Everything is a plain colour. Derived values (hover, pressed, dimmed) are
//! computed by consumers from these via [`Color::shade`] / [`Color::mix`],
//! so a theme author sets a dozen colours rather than a hundred.

use facet::Facet;

use crate::color::Color;

/// Surfaces, text and the one accent every surface shares.
#[derive(Clone, Copy, PartialEq, Debug, Facet)]
pub struct Chrome {
    /// The window behind everything.
    pub surface: Color,
    /// A panel or strip sitting on `surface`.
    pub surface_raised: Color,
    /// Wells, lanes and troughs cut into a panel.
    pub surface_sunken: Color,
    /// Hairlines between regions.
    pub border: Color,
    /// Primary label text.
    pub text: Color,
    /// Secondary text — units, inactive labels.
    pub text_dim: Color,
    /// Tertiary text — watermark-level.
    pub text_faint: Color,
    /// The one colour that means "this is live / selected / yours".
    pub accent: Color,
    /// Selection highlight, brighter than accent.
    pub selected: Color,
}

/// Audio-domain state: meters, transport, track buttons.
#[derive(Clone, Copy, PartialEq, Debug, Facet)]
pub struct Signal {
    pub meter_safe: Color,
    pub meter_warn: Color,
    pub meter_danger: Color,
    pub solo: Color,
    pub mute: Color,
    pub rec: Color,
    /// Waveform / peak body.
    pub peaks: Color,
    /// Playhead and edit cursor — the same idea in both surfaces.
    pub playhead: Color,
    /// A track with no colour assigned.
    pub neutral_track: Color,
}

/// The note/curve editing surface.
///
/// None of this maps onto REAPER's palette; it exists so the expression
/// editor and any future editor panel share one set of decisions.
#[derive(Clone, PartialEq, Debug, Facet)]
pub struct Editor {
    /// White-key row in the piano roll.
    pub row_white: Color,
    /// Black-key row.
    pub row_black: Color,
    /// The octave (C) divider — heavier than a beat line.
    pub octave_line: Color,
    /// Beat gridline.
    pub grid_beat: Color,
    /// Subdivision gridline.
    pub grid_sub: Color,
    /// Piano-key gutter background.
    pub gutter: Color,
    pub key_white: Color,
    pub key_black: Color,
    /// Structural zones and ambiguity warnings — both mean "there are
    /// boundaries here you must be aware of".
    pub zone: Color,
    /// Microtonal centres and off-ET targets.
    pub microtonal: Color,
    /// Razor areas. Deliberately distinct from `zone`: a razor is a region
    /// you are about to operate on, not a warning about one.
    pub razor: Color,
    /// Twelve pitch-class hues, C first.
    ///
    /// Notes are coloured by pitch class so a melodic shape is readable
    /// without reading the keys. Selection adds an outline rather than
    /// changing the fill, so the hue survives being selected.
    pub pitch_classes: Vec<Color>,
}

/// Non-colour metrics.
#[derive(Clone, Copy, PartialEq, Debug, Facet)]
pub struct Metrics {
    /// Corner radius, px.
    pub radius: f32,
    /// How far a panel surface blends toward its track colour (0–1).
    pub track_tint: f32,
    /// Extra dim on unselected surfaces (0–1).
    pub dim_unselected: f32,
}

/// A complete FTS theme.
#[derive(Clone, PartialEq, Debug, Facet)]
pub struct Theme {
    /// Display name.
    pub name: String,
    pub chrome: Chrome,
    pub signal: Signal,
    pub editor: Editor,
    pub metrics: Metrics,
}

impl Theme {
    /// Parse a theme from styx.
    pub fn from_styx(text: &str) -> Result<Self, String> {
        facet_styx::from_str(text).map_err(|e| e.to_string())
    }

    /// Serialize back to styx.
    pub fn to_styx(&self) -> Result<String, String> {
        facet_styx::to_string(self).map_err(|e| e.to_string())
    }

    /// The pitch-class hue for a MIDI note (or any row index).
    ///
    /// Wraps rather than indexing blindly: a theme with a short list is a
    /// bad theme, not a panic.
    pub fn pitch_class(&self, note: i32) -> Color {
        if self.editor.pitch_classes.is_empty() {
            return self.chrome.accent;
        }
        let i = note.rem_euclid(self.editor.pitch_classes.len() as i32) as usize;
        self.editor.pitch_classes[i]
    }
}

impl Default for Theme {
    /// The FastTrackStudio dark default — the tuned values the expression
    /// editor and the REAPER theme were both already using, reconciled.
    fn default() -> Self {
        let hex = |s: &str| Color::hex(s).expect("literal hex");
        Self {
            name: "FastTrackStudio".into(),
            chrome: Chrome {
                surface: hex("#0d0d11"),
                surface_raised: hex("#15151c"),
                surface_sunken: hex("#101016"),
                border: hex("#2b2b38"),
                text: hex("#c8cede"),
                text_dim: hex("#7b8397"),
                text_faint: hex("#4a5062"),
                accent: hex("#38bdf8"),
                selected: hex("#f0f9ff"),
            },
            signal: Signal {
                meter_safe: hex("#22c55e"),
                meter_warn: hex("#eab308"),
                meter_danger: hex("#ef4444"),
                solo: hex("#eab308"),
                mute: hex("#ef4444"),
                rec: hex("#e13a53"),
                peaks: hex("#5b8def"),
                playhead: hex("#f8fafc"),
                neutral_track: hex("#828282"),
            },
            editor: Editor {
                row_white: hex("#1d1d26"),
                row_black: hex("#131319"),
                octave_line: hex("#3a3a4d"),
                grid_beat: hex("#2b2b3a"),
                grid_sub: hex("#20202a"),
                gutter: hex("#101016"),
                key_white: hex("#d8dce6"),
                key_black: hex("#26262f"),
                zone: hex("#ef4444"),
                microtonal: hex("#eab308"),
                razor: hex("#22d3ee"),
                pitch_classes: [
                    "#ef4444", "#f97316", "#f59e0b", "#eab308", "#84cc16", "#22c55e", "#14b8a6",
                    "#06b6d4", "#3b82f6", "#6366f1", "#a855f7", "#ec4899",
                ]
                .into_iter()
                .map(hex)
                .collect(),
            },
            metrics: Metrics {
                radius: 5.0,
                track_tint: 0.12,
                dim_unselected: 0.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_round_trips_through_styx() {
        let theme = Theme::default();
        let text = theme.to_styx().expect("serialize");
        let back = Theme::from_styx(&text).expect("parse");
        assert_eq!(theme, back);
    }

    #[test]
    fn pitch_class_wraps_by_octave() {
        let t = Theme::default();
        // C in any octave is the same hue — that's the whole point.
        assert_eq!(t.pitch_class(0), t.pitch_class(12));
        assert_eq!(t.pitch_class(60), t.pitch_class(0));
        // And negatives don't panic or mirror.
        assert_eq!(t.pitch_class(-12), t.pitch_class(0));
        assert_eq!(t.pitch_class(-1), t.pitch_class(11));
    }

    #[test]
    fn pitch_class_falls_back_rather_than_panicking() {
        let mut t = Theme::default();
        t.editor.pitch_classes.clear();
        assert_eq!(t.pitch_class(5), t.chrome.accent);
    }

    #[test]
    fn default_has_twelve_pitch_classes() {
        assert_eq!(Theme::default().editor.pitch_classes.len(), 12);
    }

    #[test]
    fn default_text_is_readable_on_its_surface() {
        // A theme whose text vanishes into the background is broken; this
        // catches a careless edit to the defaults.
        let t = Theme::default();
        let contrast = t.chrome.text.luminance() - t.chrome.surface.luminance();
        assert!(contrast > 0.3, "text/surface contrast too low: {contrast}");
    }
}

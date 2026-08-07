//! [`Theme`] → REAPER `[color theme]` keys.
//!
//! The inverse of `daw_ui::theming::reaper_import`, and deliberately built
//! from that module's documented choices so a theme survives a round trip:
//! `col_main_bg2` is the app surface, `col_tr1_bg` the raised strip,
//! `col_main_3dsh` the border, `col_cursor` the accent (REAPER's edit cursor
//! is its de-facto accent), and `col_vutop`/`col_vumid`/`col_vubot`/
//! `col_vuclip` the meter ramp.
//!
//! This is what makes the FTS theme canonical rather than derived: REAPER
//! becomes an output, so "theme REAPER to match the editor" is one call.
//!
//! Two things are deliberately *not* emitted:
//!
//! - **Anything that isn't a colour.** `*_drawmode` words are blend modes;
//!   writing a colour over one silently changes how a layer composites.
//! - **Keys the theme has no opinion about.** Emitting a guess for all ~400
//!   would overwrite hand-tuned values in the theme being written to. Only
//!   what the palette actually determines is returned, so applying an export
//!   is a merge, not a replacement.

use crate::color::Color;
use crate::palette::Theme;

/// One REAPER palette assignment.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Assignment {
    /// `[color theme]` key.
    pub key: &'static str,
    pub color: Color,
}

impl Theme {
    /// Every REAPER palette key this theme determines.
    ///
    /// Apply with `fts_themer::ThemeIni::set_color`, which preserves
    /// REAPER's flag byte and leaves untouched keys alone.
    pub fn reaper_palette(&self) -> Vec<Assignment> {
        let c = &self.chrome;
        let s = &self.signal;
        let e = &self.editor;

        // Track-strip rows alternate; derive the second from the first so a
        // theme author sets one surface and still gets REAPER's banding.
        let strip_a = c.surface_raised;
        let strip_b = c.surface_raised.shade(0.04);

        let mut out = vec![
            // ── window chrome ───────────────────────────────────────────
            a("col_main_bg2", c.surface),
            a("col_main_text", c.text),
            a("col_main_text2", c.text_dim),
            a("col_main_3dsh", c.border),
            a("col_main_3dhl", c.border.shade(0.15)),
            a("col_main_editbk", c.surface_sunken),
            a("col_toolbar_frame", c.border),
            // ── arrange ─────────────────────────────────────────────────
            a("col_arrangebg", c.surface_sunken),
            a("col_tracklistbg", c.surface),
            a("col_mixerbg", c.surface),
            a("col_cursor", s.playhead),
            // The edit cursor's second colour tracks the accent so the two
            // cursors stay distinguishable.
            a("col_cursor2", c.accent),
            a("col_gridlines", e.grid_beat),
            a("col_gridlines2", e.grid_sub),
            a("col_gridlines3", e.octave_line),
            // ── timeline / ruler ────────────────────────────────────────
            a("col_tl_bg", c.surface_raised),
            a("col_tl_bgsel", c.accent),
            a("col_tl_fg", c.text),
            a("col_tl_fg2", c.text_dim),
            a("col_tsigmark", c.accent),
            // ── track panels ────────────────────────────────────────────
            a("col_tr1_bg", strip_a),
            a("col_tr2_bg", strip_b),
            a("col_tr1_divline", c.border),
            a("col_tr2_divline", c.border),
            a("col_tr1_peaks", s.peaks),
            a("col_tr2_peaks", s.peaks),
            a("col_seltrack", c.accent),
            a("col_seltrack2", c.accent),
            // ── media items ─────────────────────────────────────────────
            a("col_mi_bg", strip_a),
            a("col_mi_bg2", strip_b),
            a("col_mi_label", c.text),
            a("col_mi_label_sel", c.selected),
            a("col_mi_fades", c.accent),
            a("col_peaksedge", s.peaks.shade(0.2)),
            a("col_peaksedge2", s.peaks.shade(0.2)),
            // ── meters ──────────────────────────────────────────────────
            a("col_vubot", s.meter_safe),
            a("col_vumid", s.meter_warn),
            a("col_vutop", s.meter_danger),
            a("col_vuclip", s.meter_danger.shade(0.25)),
            // ── transport ───────────────────────────────────────────────
            a("col_trans_bg", c.surface_raised),
            a("col_trans_fg", c.text),
            // ── envelopes ───────────────────────────────────────────────
            a("col_envlane1_divline", c.border),
            a("col_envlane2_divline", c.border),
            // ── MIDI editor ─────────────────────────────────────────────
            // The one place REAPER and the expression editor draw the same
            // thing, so they must agree.
            a("midi_rulerbg", c.surface_raised),
            a("midi_rulerfg", c.text),
            a("midi_griddi", e.grid_sub),
            a("midi_gridhc", e.octave_line),
            a("midi_gridlines2", e.grid_beat),
            a("midi_trackbg1", e.row_white),
            a("midi_trackbg2", e.row_black),
            a("midi_selpitch1", e.row_white.mix(c.accent, 0.2)),
            a("midi_selpitch2", e.row_black.mix(c.accent, 0.2)),
            a("midi_pkey1", e.key_white),
            a("midi_pkey2", e.key_black),
            a("midi_notebg", e.row_white),
            a("midi_notefg", c.text),
            a("midi_editcurs", s.playhead),
        ];

        // Envelope lane colours come from the pitch-class wheel: they are
        // the same problem (N series that must stay tellable apart), and
        // reusing it keeps automation lanes in the theme's hue family.
        for (i, color) in e.pitch_classes.iter().take(16).enumerate() {
            out.push(Assignment {
                key: ENV_KEYS[i],
                color: *color,
            });
        }

        out
    }
}

/// `col_env1..16`, as static strs (the palette API takes `&'static str`).
const ENV_KEYS: [&str; 16] = [
    "col_env1",
    "col_env2",
    "col_env3",
    "col_env4",
    "col_env5",
    "col_env6",
    "col_env7",
    "col_env8",
    "col_env9",
    "col_env10",
    "col_env11",
    "col_env12",
    "col_env13",
    "col_env14",
    "col_env15",
    "col_env16",
];

const fn a(key: &'static str, color: Color) -> Assignment {
    Assignment { key, color }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> Vec<Assignment> {
        Theme::default().reaper_palette()
    }

    fn get(key: &str) -> Color {
        palette()
            .into_iter()
            .find(|x| x.key == key)
            .unwrap_or_else(|| panic!("no assignment for {key}"))
            .color
    }

    #[test]
    fn maps_the_keys_reaper_import_reads() {
        // The import side documents these as its anchors; if the export
        // skips one, a round trip loses that part of the theme.
        for key in [
            "col_main_bg2",
            "col_tr1_bg",
            "col_main_3dsh",
            "col_main_text",
            "col_main_text2",
            "col_cursor",
            "col_vutop",
            "col_vumid",
            "col_vubot",
            "col_vuclip",
            "col_arrangebg",
        ] {
            let _ = get(key);
        }
    }

    #[test]
    fn anchors_carry_the_authored_colour_unchanged() {
        let t = Theme::default();
        assert_eq!(get("col_main_bg2"), t.chrome.surface);
        assert_eq!(get("col_main_text"), t.chrome.text);
        assert_eq!(get("col_main_3dsh"), t.chrome.border);
        assert_eq!(get("col_vubot"), t.signal.meter_safe);
        assert_eq!(get("col_cursor"), t.signal.playhead);
    }

    #[test]
    fn never_emits_a_drawmode_or_other_non_colour() {
        // Writing a colour over a blend word silently changes compositing.
        for x in palette() {
            assert!(
                !x.key.contains("drawmode") && !x.key.ends_with("_mode"),
                "export would clobber the non-colour key {}",
                x.key
            );
        }
    }

    #[test]
    fn emits_no_duplicate_keys() {
        // A duplicate means one assignment silently wins — usually not the
        // one the author expected.
        let mut keys: Vec<&str> = palette().iter().map(|x| x.key).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "duplicate keys in the export");
    }

    #[test]
    fn alternating_track_rows_differ() {
        // If the banding collapses, REAPER's track list reads as one slab.
        assert_ne!(get("col_tr1_bg"), get("col_tr2_bg"));
        assert_ne!(get("col_mi_bg"), get("col_mi_bg2"));
    }

    #[test]
    fn midi_rows_match_the_editor_rows_exactly() {
        // The entire point of the exercise: REAPER's piano roll and the
        // expression editor draw the same rows in the same colours.
        let t = Theme::default();
        assert_eq!(get("midi_trackbg1"), t.editor.row_white);
        assert_eq!(get("midi_trackbg2"), t.editor.row_black);
        assert_eq!(get("midi_pkey1"), t.editor.key_white);
        assert_eq!(get("midi_editcurs"), t.signal.playhead);
    }

    #[test]
    fn envelope_lanes_come_from_the_pitch_wheel() {
        let t = Theme::default();
        assert_eq!(get("col_env1"), t.editor.pitch_classes[0]);
        assert_eq!(get("col_env12"), t.editor.pitch_classes[11]);
        // Only 12 hues exist, so 13-16 must simply be absent rather than
        // indexing past the end.
        assert!(palette().iter().all(|x| x.key != "col_env13"));
    }

    #[test]
    fn a_short_pitch_list_does_not_panic_the_export() {
        let mut t = Theme::default();
        t.editor.pitch_classes.truncate(3);
        let out = t.reaper_palette();
        assert!(out.iter().any(|x| x.key == "col_env3"));
        assert!(out.iter().all(|x| x.key != "col_env4"));
    }
}

//! Live preview — REAPER's arrange and mixer, drawn from the theme being edited.
//!
//! This is why the editor keeps the `.ReaperTheme` as text in the browser
//! rather than as a server-side object: [`ReaperTheme::from_sources`] rebuilds
//! the whole theme (palette → tokens, rtconfig globals/params, and the WALTER
//! program that lays out the strips) from two strings, with no filesystem. So
//! a color edit re-derives the preview on the same frame instead of waiting on
//! a save and a REAPER reload.
//!
//! Images are the one thing `from_sources` can't supply — the PNG atlases need
//! a filesystem — so element art falls back to daw-ui's vector skin. Colours,
//! layout and sizing are faithful; a button's exact bevel is not. Push to
//! REAPER when you need to judge artwork.

use daw_ui::panels::{ClipView, DawWorkspace, TrackView};
use daw_ui::theming::reaper_import::ReaperTheme;
use daw_ui::theming::{ThemeContext, ThemeProvider, theme_from_reaper};
use dioxus::prelude::*;

use crate::editor::Editor;

#[component]
pub fn Preview() -> Element {
    let editor = use_context::<Editor>();

    // Re-derived whenever the ini changes — this is the live half.
    let theme = use_memo(move || {
        let ini = editor.ini.read().to_text();
        let rtconfig = editor.rtconfig.read().clone();
        let rt = ReaperTheme::from_sources(&ini, &rtconfig);
        ThemeContext::new().with_theme(theme_from_reaper(&rt))
    });

    rsx! {
        div { class: "preview-frame",
            ThemeProvider { theme: theme(),
                DawWorkspace { tracks: sample_tracks() }
            }
        }
        p { class: "preview-note",
            "Colours and layout are live. Element artwork falls back to vectors "
            "here — the PNG atlases need a filesystem, so check those in REAPER."
        }
    }
}

/// A small, representative set of tracks: a folder with children, a couple of
/// colored tracks, and clips — enough to exercise the panel colours that
/// actually get edited.
fn sample_tracks() -> Vec<TrackView> {
    let clip = |start: f64, len: f64, name: &str, amp: f32, seed: f32| {
        let mut c = ClipView::new(start, len, name, None);
        c.peaks = wave(120, amp, seed);
        c.fade_in = 0.15;
        c.fade_out = 0.35;
        c
    };

    vec![
        TrackView::new(0, "DRUMS", Some("#e0567a"))
            .folder()
            .depth(0)
            .fader(0.82),
        TrackView::new(1, "Kick", Some("#e0567a"))
            .depth(1)
            .fader(0.75)
            .clips(vec![clip(0.0, 8.0, "Kick", 0.9, 0.0)]),
        TrackView::new(2, "Snare", Some("#e0567a"))
            .depth(1)
            .fader(0.7)
            .clips(vec![clip(0.0, 8.0, "Snare", 0.7, 1.3)]),
        TrackView::new(3, "Bass", Some("#5b8def"))
            .depth(0)
            .fader(0.68)
            .clips(vec![clip(0.0, 8.0, "Bass", 0.8, 2.1)]),
        TrackView::new(4, "Keys", Some("#f2b134"))
            .depth(0)
            .fader(0.6)
            .clips(vec![clip(2.0, 6.0, "Keys", 0.5, 3.7)]),
        TrackView::new(5, "Lead Vox", Some("#3ddc97"))
            .depth(0)
            .fader(0.74)
            .clips(vec![clip(1.0, 6.5, "Lead Vox", 0.85, 4.2)]),
    ]
}

/// A decaying-sine waveform so clips draw like audio, not flat blocks.
fn wave(cols: usize, amp: f32, seed: f32) -> Vec<(f32, f32)> {
    (0..cols)
        .map(|i| {
            let t = i as f32 / cols as f32;
            let env = (1.0 - t).powf(0.35) * amp;
            let s = ((t * 37.0 + seed).sin() * 0.6 + (t * 91.0 + seed * 2.0).sin() * 0.4) * env;
            let m = s.abs().min(1.0);
            (m, -m * 0.85)
        })
        .collect()
}

//! DAW-UI playground — the vector-themeable REAPER-style panels rendered in
//! the browser with fake sample data (no DAW backend).
//!
//! Run:
//! ```sh
//! dx serve --example playground --platform web --features playground
//! ```
//!
//! It mounts [`DawWorkspace`] (arrange-over-mixer) under a [`ThemeProvider`],
//! fed a hand-built `Vec<TrackView>` — a small Praise-style setlist with a
//! folder, clips (with waveforms + fades), markers/regions, and static meter
//! levels. This is the decoupled surface for iterating the theming system:
//! everything renders from props/`Signal`s, so it needs neither `daw-control`
//! nor a live engine.

use dioxus::prelude::*;

use daw_ui::panels::{
    ClipView, DawWorkspace, LaneDisplay, MarkerView, RegionView, TempoMarkerView, TrackView,
};
use daw_ui::theming::{ThemeContext, ThemeProvider};

fn main() {
    dioxus::launch(App);
}

/// A decaying-sine waveform: `cols` normalized `(max, min)` peak pairs, so the
/// arrange lanes draw a real REAPER-style item instead of a flat block.
fn wave(cols: usize, amp: f32, seed: f32) -> Vec<(f32, f32)> {
    (0..cols)
        .map(|i| {
            let t = i as f32 / cols as f32;
            // A couple of partials + an envelope so it looks like audio.
            let env = (1.0 - t).powf(0.35) * amp;
            let s = ((t * 37.0 + seed).sin() * 0.6 + (t * 91.0 + seed * 2.0).sin() * 0.4) * env;
            let m = s.abs().min(1.0);
            (m, -m * 0.85)
        })
        .collect()
}

/// Build the sample setlist: a DRUMS folder over its stems, plus bass, gtr,
/// keys and a stereo lead vocal. Colours are REAPER-ish track tints.
fn sample_tracks() -> Vec<TrackView> {
    let clip = |start: f64, len: f64, name: &str, amp: f32, seed: f32| {
        let mut c = ClipView::new(start, len, name, None);
        c.peaks = wave(160, amp, seed);
        c.fade_in = 0.15;
        c.fade_out = 0.4;
        c
    };

    vec![
        // ── DRUMS folder ──────────────────────────────────────────────
        TrackView::new(0, "DRUMS", Some("#e0567a"))
            .folder()
            .depth(0)
            .fader(0.82)
            .routing(false, false),
        TrackView::new(1, "Kick", Some("#e0567a"))
            .depth(1)
            .fader(0.8)
            .levels(0.55, 0.55, 0.72)
            .clips(vec![clip(0.0, 26.0, "Kick", 0.9, 1.0)]),
        TrackView::new(2, "Snare", Some("#e07a56"))
            .depth(1)
            .fader(0.74)
            .levels(0.42, 0.44, 0.6)
            .clips(vec![clip(0.0, 26.0, "Snare", 0.75, 5.0)]),
        {
            let mut oh = TrackView::new(3, "OH", Some("#e0c256"))
                .depth(1)
                .fader(0.65)
                .levels(0.3, 0.34, 0.5)
                .clips(vec![clip(0.0, 26.0, "OH ⋅L/R", 0.55, 9.0)]);
            oh.lane_display = LaneDisplay::Small;
            oh
        },
        // ── melodic ───────────────────────────────────────────────────
        TrackView::new(4, "BASS", Some("#56a0e0"))
            .depth(0)
            .fader(0.78)
            .levels(0.48, 0.48, 0.66)
            .clips(vec![clip(0.0, 26.0, "Bass DI", 0.8, 13.0)]),
        TrackView::new(5, "GTR", Some("#7be056"))
            .depth(0)
            .fader(0.6)
            .routing(true, false)
            .levels(0.36, 0.4, 0.55)
            .clips(vec![
                clip(2.0, 10.0, "Gtr Vs", 0.6, 17.0),
                clip(14.0, 10.0, "Gtr Ch", 0.72, 19.0),
            ]),
        TrackView::new(6, "KEYS", Some("#b07be0"))
            .depth(0)
            .fader(0.58)
            .levels(0.28, 0.3, 0.45)
            .clips(vec![clip(0.0, 26.0, "Keys Pad", 0.5, 23.0)]),
        {
            let mut vox = TrackView::new(7, "LEAD VOX", Some("#e0d456"))
                .depth(0)
                .fader(0.85)
                .stereo()
                .levels(0.62, 0.58, 0.8)
                .routing(true, false)
                .clips(vec![
                    clip(2.0, 8.0, "Verse", 0.7, 29.0),
                    clip(12.0, 12.0, "Chorus", 0.88, 31.0),
                ]);
            vox.selected = Signal::new(true);
            vox
        },
    ]
}

#[component]
fn App() -> Element {
    let tracks = use_signal(sample_tracks);
    let playhead = use_signal(|| 6.0_f64);

    let markers = vec![
        MarkerView { time: 0.0, name: "Intro".into(), color: None, idx: 1 },
        MarkerView { time: 8.0, name: "Verse".into(), color: None, idx: 2 },
        MarkerView { time: 14.0, name: "Chorus".into(), color: None, idx: 3 },
    ];
    let regions = vec![
        RegionView { start: 0.0, end: 8.0, name: "A".into(), color: Some("#3a5a8a".into()), idx: 1 },
        RegionView { start: 8.0, end: 14.0, name: "B".into(), color: Some("#8a3a5a".into()), idx: 2 },
        RegionView { start: 14.0, end: 26.0, name: "C".into(), color: Some("#3a8a5a".into()), idx: 3 },
    ];
    let tempo_markers = vec![TempoMarkerView { time: 0.0, bpm: 72.0, num: 4, den: 4 }];

    rsx! {
        // Full-viewport dark shell so the panels fill the browser window.
        div {
            style: "position:fixed; inset:0; display:flex; flex-direction:column; \
                    background:#0b0b0d; color:#e4e4e7; \
                    font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;",
            div {
                style: "flex:0 0 auto; display:flex; align-items:center; gap:10px; \
                        padding:7px 14px; border-bottom:1px solid #27272a; background:#0e0e11;",
                span {
                    style: "font-size:11px; font-weight:700; letter-spacing:0.08em; \
                            text-transform:uppercase; color:#a1a1aa;",
                    "DAW-UI Playground"
                }
                span {
                    style: "font-size:11px; color:#52525b;",
                    "DawWorkspace · sample data · FTS dark theme"
                }
            }
            div {
                style: "flex:1; min-height:0;",
                ThemeProvider {
                    theme: ThemeContext::new(),
                    DawWorkspace {
                        tracks: tracks(),
                        markers,
                        regions,
                        tempo_markers,
                        bpm: 72.0,
                        playhead,
                        pps: 34.0,
                        seconds: 28.0,
                        beats_per_measure: 4,
                    }
                }
            }
        }
    }
}

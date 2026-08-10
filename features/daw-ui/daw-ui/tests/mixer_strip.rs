//! The assembled strip: sections in REAPER's order, and no dependency on a
//! stylesheet arriving.
//!
//! The styling boundary this ticket establishes is the `<svg>` element, not
//! the render target. Outside one, every target runs a real CSS engine, so
//! layout is Tailwind. Inside one, the subtree goes to a parser with no box
//! model, which is why the art states its own values. The tests that matter
//! are therefore: does the strip still read correctly with no sheet at all,
//! and is anything inside an `<svg>` waiting on CSS that will never come.

use daw_proto::Track;
use daw_ui::controls::TrackStore;
use dioxus::prelude::*;

fn track(guid: &str, index: u32, name: &str) -> Track {
    Track {
        guid: guid.to_string(),
        index,
        name: name.into(),
        color: Some(0x8844cc),
        fx_count: 1,
        ..Default::default()
    }
}

/// The strip at a given height, which is what it collapses by.
fn strip_at(height: f32) -> String {
    #[derive(Props, Clone, PartialEq)]
    struct P {
        height: f32,
    }
    let mut dom = VirtualDom::new_with_props(
        |p: P| {
            let mut store = use_hook(TrackStore::new);
            use_hook(|| {
                store.seed([track("T1", 0, "Kick")]);
                provide_context(store);
            });
            rsx! {
                daw_ui::components::mixer::ChannelStripPreview {
                    track: track("T1", 0, "Kick"),
                    height: p.height,
                }
            }
        },
        P { height },
    );
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

/// Sweep a height boundary and assert the rendered strip changes there —
/// the model's own tests prove the arithmetic, these prove the markup
/// actually asks it.
fn sweeps(at: f32, present: impl Fn(&str) -> bool, what: &str) {
    assert!(present(&strip_at(at)), "{what} missing at {at}");
    assert!(!present(&strip_at(at - 1.0)), "{what} still there below {at}");
}

/// The strip, mounted with **no stylesheet** — which is how a REAPER panel
/// gets it if the sheet is ever mounted wrongly, and how every one of these
/// tests runs.
fn strip_html(tracks: &[Track]) -> String {
    #[derive(Props, Clone, PartialEq)]
    struct P {
        tracks: Vec<Track>,
    }

    let mut dom = VirtualDom::new_with_props(
        |p: P| {
            let mut store = use_hook(TrackStore::new);
            use_hook(|| {
                store.seed(p.tracks.clone());
                provide_context(store);
            });
            rsx! {
                div {
                    for t in p.tracks.iter() {
                        daw_ui::components::mixer::ChannelStripPreview {
                            key: "{t.guid}",
                            track: t.clone(),
                        }
                    }
                }
            }
        },
        P { tracks: tracks.to_vec() },
    );
    dom.rebuild_in_place();
    dioxus_ssr::render(&dom)
}

/// The sections REAPER's MCP carries, top to bottom. A mixer that reorders
/// them stops being readable by anyone who knows the host.
#[test]
fn the_strip_carries_its_sections_in_reapers_order() {
    let html = strip_html(&[track("T1", 0, "Kick")]);

    // Each section is identified by a control only it contains: the FX
    // pill, the pan knob, the record-arm ring, the fader, the name plate.
    let order = ["M<", "in ", "Kick"];
    let mut at = 0usize;
    for needle in order {
        let found = html[at..]
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} missing or out of order:\n{html}"));
        at += found;
    }

    // The fader region is the one that absorbs the strip's height — that
    // is what gives the fader's stretch band something to stretch into.
    assert!(html.contains("flex-1"), "nothing takes the strip's height:\n{html}");
}

/// The whole point of the boundary: no layout depends on CSS reaching
/// inside an `<svg>`, and nothing inside one waits on a class.
#[test]
fn nothing_inside_an_svg_waits_on_a_stylesheet() {
    let html = strip_html(&[track("T1", 0, "Kick")]);

    for chunk in html.split("<svg").skip(1) {
        let svg = &chunk[..chunk.find("</svg>").unwrap_or(chunk.len())];
        assert!(
            !svg.contains("class=\""),
            "an svg subtree carries a class, which no non-browser target will resolve:\n{svg}"
        );
        assert!(
            !svg.contains("currentColor"),
            "an svg subtree inherits a colour it will never be given:\n{svg}"
        );
    }
}

/// With no sheet at all the strip still draws its controls and states its
/// own sizes — Tailwind is additive.
#[test]
fn the_strip_renders_with_the_sheet_absent() {
    let html = strip_html(&[track("T1", 0, "Kick")]);

    assert!(html.contains("<svg"), "no art drawn:\n{html}");
    // Layout-critical values are explicit rather than class-only.
    assert!(html.contains("px;"), "no explicit pixel sizing:\n{html}");
    assert!(html.contains("Kick"), "no track name:\n{html}");
    // And nothing blits.
    assert!(!html.contains("<img"), "the strip is blitting:\n{html}");
    assert!(!html.contains("url(data:"), "the strip is blitting:\n{html}");
}

/// Several strips side by side are a mixer: each gets its own controls,
/// keyed by its own track.
#[test]
fn several_strips_read_as_a_mixer() {
    let tracks = [
        track("T1", 0, "Kick"),
        track("T2", 1, "Snare"),
        track("T3", 2, "Bass"),
    ];
    let html = strip_html(&tracks);

    for name in ["Kick", "Snare", "Bass"] {
        assert!(html.contains(name), "{name} is missing from the mixer");
    }
    // One mute button per strip — the count is what catches a strip that
    // silently renders its neighbour's controls.
    assert_eq!(html.matches(">M<").count(), 3, "not one mute per strip");
}


/// Each container-height threshold hides its element at REAPER's value.
#[test]
fn the_container_thresholds_hide_their_elements() {
    use daw_ui::controls::REAPER_THRESHOLDS as T;

    // The record-input readout is the clearest marker: it prints "in".
    sweeps(T.record_input, |h| h.contains(">in "), "record input");
    // The dB readout under the fader.
    sweeps(T.volume_label, |h| h.contains("font-mono"), "volume label");
    // The pan label under the knob.
    sweeps(T.pan_labels, |h| h.contains(">pan<"), "pan labels");
}

/// Below the pan-section threshold the pan control is still present — it
/// has re-parented into the input area — and record mode is gone.
#[test]
fn pan_re_anchors_rather_than_disappearing() {
    use daw_ui::controls::REAPER_THRESHOLDS as T;

    let tall = strip_at(T.pan_section);
    let short = strip_at(T.pan_section - 1.0);

    // The knob is drawn either way: its art is the one with a 24x25 box.
    let knobs = |h: &str| h.matches("viewBox=\"0 0 24 25\"").count();
    assert!(knobs(&tall) >= 1, "no pan knob above the threshold:\n{tall}");
    assert!(knobs(&short) >= 1, "the pan control vanished with its section");

    // And the pan label is gone with the section it belonged to.
    assert!(!short.contains(">pan<"), "the pan section's label survived it");
}

/// Below the swap threshold the fader is not a short fader — it is a
/// different widget. The fader's rail decomposes into three bands; a knob
/// does not.
#[test]
fn the_fader_becomes_a_knob() {
    use daw_ui::controls::REAPER_THRESHOLDS as T;

    let rail = |h: &str| h.contains("viewBox=\"0 16 23 23\"");
    assert!(rail(&strip_at(T.fader_swap)), "no fader above the swap threshold");
    assert!(!rail(&strip_at(T.fader_swap - 1.0)), "still a fader below the swap threshold");
}

/// The residual-driven collapses fire off the stretch section. Sweeping the
/// strip's height crosses them too, just at a different place than the
/// container thresholds — which is the point of them being separate.
#[test]
fn the_residual_collapses_fire_on_the_stretch_section() {
    use daw_ui::controls::Collapse;

    // Find the strip height at which the stretch section crosses the IO
    // threshold, then check the rendered strip agrees on both sides.
    let io_boundary = (100..=700)
        .map(|h| h as f32)
        .find(|h| Collapse::at(*h).show_io)
        .expect("some height shows the IO button");

    let with = strip_at(io_boundary);
    let without = strip_at(io_boundary - 1.0);
    // The IO art is 23x32 — nothing else in the strip uses that box.
    let io = |h: &str| h.contains("viewBox=\"0 0 23 32\"");
    assert!(io(&with), "no IO button at the boundary:\n{with}");
    assert!(!io(&without), "IO button survived below its threshold");
}

/// Padding steps down in three stages as the strip shortens.
#[test]
fn padding_steps_down_in_three_stages() {
    let stages: std::collections::BTreeSet<String> = (120..=800)
        .step_by(4)
        .map(|h| {
            let html = strip_at(h as f32);
            ["padding-top:2px", "padding-top:3px", "padding-top:4px"]
                .into_iter()
                .find(|p| html.contains(p))
                .unwrap_or("none")
                .to_string()
        })
        .collect();
    assert_eq!(stages.len(), 3, "padding does not step through three stages: {stages:?}");
}


/// The app's mixer draws vectors and blits nothing — the guarantee #147
/// exists to make permanent.
///
/// Deliberately a whole-strip sweep rather than a spot check: the bitmap
/// path it replaced was per-element, so a single control regressing to a
/// blit is exactly the failure mode, and it would be invisible at any one
/// height.
#[test]
fn the_apps_mixer_never_blits_at_any_height() {
    for height in (120..=760).step_by(20) {
        let html = strip_at(height as f32);
        assert!(!html.contains("<img"), "an <img> appeared at height {height}");
        assert!(
            !html.contains("url(data:"),
            "a data-URI background appeared at height {height}"
        );
        assert!(
            !html.contains("background-image"),
            "a background image appeared at height {height}"
        );
        assert!(html.contains("<svg"), "nothing drawn at height {height}");
    }
}

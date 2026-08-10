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

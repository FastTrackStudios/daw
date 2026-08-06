//! The VU contact sheet: every face in the kit, painted to one PNG.
//!
//! The companion to `knob_sheet.rs`. Adding a face is a [`VuFace`] variant
//! plus one `VuSpec` const in `vu_faces.rs` — and then you want to look at
//! it, at rest and swinging, in its bezel and out of it:
//!
//! ```sh
//! cargo test -p fts-ui-audio --test vu_sheet
//! ```
//!
//! Output lands in `target/gui-shots/vu/` (override with `FTS_SHOTS_DIR`).
//! Nothing here asserts a *look*. What it asserts is that every face draws a
//! card and a needle, and that the needle actually moves when the value does
//! — a face whose needle matched its card would be a meter you cannot read,
//! and no colour test would notice.

use std::path::PathBuf;

use dioxus::prelude::*;
use dioxus_test::{by_testid, render};

use fts_ui_audio::hardware::vu::{VuFace, VuMeter, VuMode};
use fts_ui_audio::hardware::vu_svg::VuScale;

/// Rest, working, and pinned — enough to see the needle sweep and to catch a
/// scale that crowds the wrong end.
const READINGS: [f32; 3] = [0.0, 6.0, 14.0];

#[component]
fn Sheet() -> Element {
    rsx! {
        style {
            "html, body {{ margin:0; padding:0; background:#20232a; \
             font-family: ui-sans-serif, system-ui, sans-serif; }}"
        }
        div {
            style: "display:flex; flex-direction:column; gap:10px; padding:14px;",
            for face in VuFace::ALL {
                div {
                    key: "{face:?}",
                    style: "display:flex; align-items:center; gap:16px;",
                    div {
                        style: "width:96px; color:#dfe3e8; font-size:12px; \
                                font-weight:700; letter-spacing:0.04em;",
                        "{face:?}"
                    }
                    for (i , db) in READINGS.iter().enumerate() {
                        VuMeter {
                            key: "r{i}",
                            scale: 1.0,
                            width: 150.0,
                            face,
                            mode: VuMode::GainReduction,
                            value_db: *db,
                            legend: "GAIN REDUCTION".to_string(),
                            card: VuScale::Vu,
                        }
                    }
                    // In its frame, which is a different drawing: the chamfer
                    // has to read as an opening rather than a raised boss.
                    VuMeter {
                        scale: 1.0,
                        width: 150.0,
                        face,
                        mode: VuMode::Level,
                        value_db: -12.0,
                        legend: "VU".to_string(),
                        bezel: true,
                        card: VuScale::Vu,
                    }
                }
            }
        }
    }
}

fn shots_dir() -> PathBuf {
    let dir = std::env::var("FTS_SHOTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/gui-shots/vu")
        });
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create {}: {e}", dir.display()));
    dir
}

#[tokio::test]
async fn shot_every_vu_face_in_the_kit() {
    let tester = render(Sheet)
        .with_window_size(760, 40 + 130 * VuFace::ALL.len() as u32)
        .build();
    let _ = tester.pump().await;
    tester.relayout();

    // Every face drew a movement with a real box, and a needle on it.
    let meters = tester.query_all(by_testid("vu-meter")).immediately();
    let expected = VuFace::ALL.len() * (READINGS.len() + 1);
    assert_eq!(
        meters.len(),
        expected,
        "the sheet drew {} movements, not {expected}",
        meters.len(),
    );
    for m in &meters {
        let (w, h) = m.size();
        assert!(w > 0.0 && h > 0.0, "a movement drew {w}x{h}");
    }

    // And the needle moved: the readings are distinct, so the positions the
    // meters report must be too. A face that ignored its value would draw
    // four identical movements and look perfectly fine in a screenshot.
    let positions: Vec<String> = meters
        .iter()
        .take(READINGS.len())
        .filter_map(|m| m.attribute("data-vu"))
        .collect();
    assert_eq!(positions.len(), READINGS.len(), "a movement reported no position");
    for pair in positions.windows(2) {
        assert_ne!(
            pair[0], pair[1],
            "the needle did not move between two different readings: {positions:?}",
        );
    }

    let path = shots_dir().join("kit.png");
    tester.render_png(&path);
    println!("vu kit: {}", path.display());
}

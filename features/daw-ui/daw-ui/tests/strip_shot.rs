//! The strip, rasterised, at REAPER's own strip height.
//!
//! The convergence loop needs a picture of the strip on every edit, and
//! booting REAPER for one takes minutes. This paints the same component
//! through blitz-dom — the renderer REAPER embeds — so the picture is
//! honest and arrives in seconds.
//!
//! `MIXER_HEIGHT=228 cargo test -p daw-ui --test strip_shot -- --nocapture`
//! writes `target/theme-shots/strip-dioxus.png`, which
//! `just` (or the comparison script) montages against a crop of the real
//! MCP. 228 is not arbitrary: it is the height of the strip in the
//! reference REAPER screenshot, measured off its coloured band.

use daw_proto::Track;
use daw_ui::components::mixer::ChannelStripPreview;
use daw_ui::controls::TrackStore;
use dioxus::prelude::*;

/// REAPER's strip in the reference shot, measured rather than assumed:
/// its index plate is 20 rows at 1x and 60 in a 3x zoom, which fixes the
/// zoom, and the strip is 684 rows in that zoom.
const REFERENCE_HEIGHT: f32 = 228.0;

fn kick() -> Track {
    Track {
        guid: "kick".into(),
        index: 0,
        name: "Kick".into(),
        color: Some(0xc4_44_6a),
        // Unity, which is where the reference screenshot's tracks are —
        // the cap should land where REAPER's does, not at the top.
        volume: 1.0,
        ..Default::default()
    }
}

#[test]
fn paint_the_strip_for_comparison() {
    let height: f32 = std::env::var("MIXER_HEIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(REFERENCE_HEIGHT);

    fn app() -> Element {
        let mut store = use_hook(TrackStore::new);
        use_hook(|| {
            store.seed([kick()]);
            provide_context(store);
        });
        let height: f32 = std::env::var("MIXER_HEIGHT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(REFERENCE_HEIGHT);
        rsx! {
            // Pinned to the origin rather than reset through a stylesheet:
            // the UA's 8px body margin would inset the whole strip and read
            // as a left gutter the strip does not have.
            div {
                style: "position:absolute; left:0; top:0; \
                        background:#1e1e1e; padding:0; margin:0; width:86px;",
                ChannelStripPreview { track: kick(), index: 0, height }
            }
        }
    }

    // Relative to the manifest, not the CWD: `cargo test` runs a test
    // binary from its package directory, so "target/theme-shots" lands
    // three levels below the one the tools read.
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/theme-shots");
    let out = out.as_path();
    std::fs::create_dir_all(out).unwrap();
    let path = out.join("strip-dioxus.png");

    // The window is the strip plus the UA's 8px body margin, which nothing
    // in a headless document resets — neither a stylesheet nor an absolute
    // position escapes it, so the margin is budgeted for and cropped off.
    dioxus_test::render(app)
        .with_window_size(86 + BODY_MARGIN * 2, height.ceil() as u32 + BODY_MARGIN * 2)
        .build()
        .render_png(&path);

    crop_to_strip(&path, height.ceil() as u32);
    println!("wrote {} at height {height}", path.display());
}

/// The UA body margin, which the headless document applies and no reset
/// this test could write has removed.
const BODY_MARGIN: u32 = 8;

/// Trim the margin off, so the PNG is the strip and nothing else and a
/// column of it can be measured against a crop of REAPER's own.
fn crop_to_strip(path: &std::path::Path, height: u32) {
    let status = std::process::Command::new("magick")
        .arg(path)
        .arg("-crop")
        .arg(format!("86x{height}+{BODY_MARGIN}+{BODY_MARGIN}"))
        .arg("+repage")
        .arg(path)
        .status();
    match status {
        Ok(s) if s.success() => {}
        // No ImageMagick is not a test failure: the PNG is still written,
        // just with its margin on.
        _ => eprintln!("note: `magick` unavailable, {} keeps its margin", path.display()),
    }
}

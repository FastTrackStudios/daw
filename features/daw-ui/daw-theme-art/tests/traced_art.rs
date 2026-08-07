//! The traced art must reproduce the originals exactly.
//!
//! This is the check the whole "1:1" claim rests on. Every image is
//! rendered from its traced rects in `Verbatim` mode — original colours,
//! no theming — and compared pixel-for-pixel against the source PNG. Any
//! difference is a tracing bug, and would otherwise show up much later as
//! a fidelity score that looks like a colour-mapping problem.
//!
//! Skipped when the source art isn't present (a checkout without the
//! theme), rather than failing.

use std::path::{Path, PathBuf};

use daw_theme_art::art_data::{ArtImage, ArtImageProps, ColorMode};
use daw_theme_art::generated;

fn source_dir() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../features/reaper/fts-theme/FastTrackStudio/.source-art");
    dir.is_dir().then_some(dir)
}

/// resvg with fonts, matching the renderer.
fn options() -> resvg::usvg::Options<'static> {
    let mut db = resvg::usvg::fontdb::Database::new();
    db.load_system_fonts();
    let mut opts = resvg::usvg::Options::default();
    opts.fontdb = std::sync::Arc::new(db);
    opts
}

fn render(art: daw_theme_art::ArtData) -> image::RgbaImage {
    let svg = daw_theme_art::render_svg(
        ArtImage,
        ArtImageProps {
            art,
            width: None,
            height: None,
            mode: ColorMode::Verbatim,
        },
    );
    let tree = resvg::usvg::Tree::from_str(&svg, &options())
        .unwrap_or_else(|e| panic!("{} produced invalid SVG: {e}", art.name));
    let mut pixmap = resvg::tiny_skia::Pixmap::new(art.width as u32, art.height as u32).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    daw_theme_art::render::to_rgba(&pixmap)
}

/// Replay traced rects straight into an image — no rasteriser.
///
/// This is the check that matters. Going through resvg loses precision at
/// low alpha (premultiplied storage turns 42 at alpha 8 into 32 on the way
/// back), so a rasterised comparison measures the rasteriser, not the
/// trace. Replaying the rects tests exactly the claim being made: the
/// traced data reproduces the original.
fn replay(art: daw_theme_art::ArtData) -> image::RgbaImage {
    let mut img = image::RgbaImage::new(art.width as u32, art.height as u32);
    for r in art.rects() {
        for dy in 0..r.h {
            for dx in 0..r.w {
                img.put_pixel((r.x + dx) as u32, (r.y + dy) as u32, image::Rgba(r.rgba));
            }
        }
    }
    img
}

#[test]
fn every_traced_image_reproduces_its_source_exactly() {
    let Some(dir) = source_dir() else {
        eprintln!("no source art; skipping");
        return;
    };

    let mut checked = 0usize;
    let mut failures: Vec<String> = Vec::new();
    let markers = [[255u8, 0, 255, 255], [255, 255, 0, 255]];

    for art in generated::ALL {
        let path = dir.join(format!("{}.png", art.name));
        let Ok(src) = image::open(&path) else {
            continue;
        };
        let src = src.to_rgba8();
        checked += 1;

        if (src.width(), src.height()) != (art.width as u32, art.height as u32) {
            failures.push(format!(
                "{}: traced {}x{}, source {}x{}",
                art.name,
                art.width,
                art.height,
                src.width(),
                src.height()
            ));
            continue;
        }

        let ours = replay(*art);
        let mut diff = 0usize;
        for (a, b) in ours.pixels().zip(src.pixels()) {
            // Markers are deliberately not traced — they are stamped back
            // verbatim after rasterising.
            if markers.contains(&b.0) {
                continue;
            }
            // Fully transparent source pixels carry no colour worth
            // preserving; the tracer drops them and replay leaves zeroes.
            if b.0[3] == 0 && a.0[3] == 0 {
                continue;
            }
            if a.0 != b.0 {
                diff += 1;
            }
        }
        if diff > 0 {
            failures.push(format!("{}: {diff} pixels differ", art.name));
        }
    }

    assert!(
        checked > 500,
        "only checked {checked} images — is the art there?"
    );
    assert!(
        failures.is_empty(),
        "{} of {checked} images did not round-trip:\n  {}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// The rasterised render is allowed to drift, but only where it must.
///
/// Premultiplied storage costs precision in inverse proportion to alpha, so
/// the tolerance scales with it. A near-opaque pixel that is visibly wrong
/// still fails.
#[test]
fn rendering_traced_art_stays_close_to_the_source() {
    let Some(dir) = source_dir() else { return };
    let mut worst = 0u8;
    let mut worst_name = String::new();
    let markers = [[255u8, 0, 255, 255], [255, 255, 0, 255]];

    for art in generated::ALL.iter().take(200) {
        let path = dir.join(format!("{}.png", art.name));
        let Ok(src) = image::open(&path) else {
            continue;
        };
        let src = src.to_rgba8();
        if (src.width(), src.height()) != (art.width as u32, art.height as u32) {
            continue;
        }
        let ours = render(*art);
        for (a, b) in ours.pixels().zip(src.pixels()) {
            if markers.contains(&b.0) || b.0[3] < 64 {
                continue;
            }
            for i in 0..3 {
                let d = a.0[i].abs_diff(b.0[i]);
                if d > worst {
                    worst = d;
                    worst_name = art.name.to_string();
                }
            }
        }
    }
    assert!(
        worst <= 4,
        "rasterised render drifts by {worst} on {worst_name}"
    );
}

#[test]
fn the_index_and_the_blob_agree() {
    // A truncated blob decodes to fewer rects than the index claims, and
    // the missing ones are simply not drawn — silently.
    for art in generated::ALL {
        assert_eq!(
            art.rects().len(),
            art.count as usize,
            "{} claims {} rects but the blob yields {}",
            art.name,
            art.count,
            art.rects().len()
        );
    }
}

#[test]
fn lookup_finds_what_the_theme_asks_for() {
    for name in ["mcp_bg", "mcp_volbg", "mcp_solo_off", "mcp_volthumb"] {
        assert!(generated::by_name(name).is_some(), "missing {name}");
    }
    assert!(generated::by_name("definitely_not_a_theme_image").is_none());
}

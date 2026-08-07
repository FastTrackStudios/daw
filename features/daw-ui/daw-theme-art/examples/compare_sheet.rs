//! Traced vs vector, side by side, for every state of every control.
//!
//!     cargo run -p daw-theme-art --example compare_sheet
//!
//! Each pair is the **traced** control (pixel-exact with Reapertips, drawn
//! in our palette) above the **vector** one (redrawn as shapes). Rendered
//! large on purpose: at native size they are hard to tell apart, and the
//! differences that matter — proportions, weight, whether a glyph reads —
//! only show when you zoom, which is the whole reason the vector versions
//! exist.

use daw_theme_art::render::render_svg;
use daw_theme_art::{mixer_controls as traced, vector_controls as vector};

/// Rasterise SVG to a fixed height, preserving aspect.
fn raster(svg: &str, px: u32) -> Option<image::RgbaImage> {
    let mut db = resvg::usvg::fontdb::Database::new();
    db.load_system_fonts();
    let mut opts = resvg::usvg::Options::default();
    opts.fontdb = std::sync::Arc::new(db);
    let tree = resvg::usvg::Tree::from_str(svg, &opts).ok()?;
    let (vw, vh) = (tree.size().width(), tree.size().height());
    if vw <= 0.0 || vh <= 0.0 {
        return None;
    }
    let scale = px as f32 / vh;
    let (w, h) = (((vw * scale) as u32).max(1), px.max(1));
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(daw_theme_art::render::to_rgba(&pixmap))
}

const CELL: u32 = 110;
const PAD: u32 = 14;

fn main() {
    // (label, traced svg, vector svg)
    let mut rows: Vec<(&str, String, String)> = Vec::new();

    let n = (None, None);
    for (label, st, vs) in [
        ("rec off", traced::RecordArm::Off, vector::RecordArm::Off),
        ("rec on", traced::RecordArm::On, vector::RecordArm::On),
        (
            "rec norec",
            traced::RecordArm::NoRecord,
            vector::RecordArm::NoRecord,
        ),
        ("rec auto", traced::RecordArm::Auto, vector::RecordArm::Auto),
    ] {
        rows.push((
            label,
            render_svg(
                traced::RecordArmButton,
                traced::RecordArmProps {
                    state: st,
                    width: n.0,
                    height: n.1,
                },
            ),
            render_svg(
                vector::RecordArmButton,
                vector::RecordArmProps {
                    state: vs,
                    width: n.0,
                    height: n.1,
                },
            ),
        ));
    }

    for (label, on) in [("mute off", false), ("mute on", true)] {
        rows.push((
            label,
            render_svg(
                traced::MuteButton,
                traced::ToggleProps {
                    on,
                    width: n.0,
                    height: n.1,
                },
            ),
            render_svg(
                vector::MuteButton,
                vector::ToggleProps {
                    on,
                    width: n.0,
                    height: n.1,
                },
            ),
        ));
    }

    for (label, st, vs) in [
        ("solo off", traced::Solo::Off, vector::Solo::Off),
        ("solo on", traced::Solo::On, vector::Solo::On),
        ("solo defeat", traced::Solo::Defeat, vector::Solo::Defeat),
    ] {
        rows.push((
            label,
            render_svg(
                traced::SoloButton,
                traced::SoloProps {
                    state: st,
                    width: n.0,
                    height: n.1,
                },
            ),
            render_svg(
                vector::SoloButton,
                vector::SoloProps {
                    state: vs,
                    width: n.0,
                    height: n.1,
                },
            ),
        ));
    }

    for (label, st, vs) in [
        ("fx empty", traced::FxChain::Empty, vector::FxChain::Empty),
        (
            "fx active",
            traced::FxChain::Active,
            vector::FxChain::Active,
        ),
        (
            "fx byp",
            traced::FxChain::Bypassed,
            vector::FxChain::Bypassed,
        ),
    ] {
        rows.push((
            label,
            render_svg(
                traced::FxButton,
                traced::FxProps {
                    state: st,
                    width: n.0,
                    height: n.1,
                },
            ),
            render_svg(
                vector::FxButton,
                vector::FxProps {
                    state: vs,
                    width: n.0,
                    height: n.1,
                },
            ),
        ));
    }

    for (label, s, r) in [
        ("route none", false, false),
        ("route send", true, false),
        ("route both", true, true),
    ] {
        rows.push((
            label,
            render_svg(
                traced::RoutingButton,
                traced::RoutingProps {
                    has_sends: s,
                    has_receives: r,
                    disabled: false,
                    width: n.0,
                    height: n.1,
                },
            ),
            render_svg(
                vector::RoutingButton,
                vector::RoutingProps {
                    has_sends: s,
                    has_receives: r,
                    disabled: false,
                    width: n.0,
                    height: n.1,
                },
            ),
        ));
    }

    for (label, st, vs) in [
        ("mon off", traced::Monitoring::Off, vector::Monitoring::Off),
        ("mon on", traced::Monitoring::On, vector::Monitoring::On),
        (
            "mon auto",
            traced::Monitoring::Auto,
            vector::Monitoring::Auto,
        ),
    ] {
        rows.push((
            label,
            render_svg(
                traced::InputMonitorIndicator,
                traced::MonitoringProps {
                    state: st,
                    width: n.0,
                    height: n.1,
                },
            ),
            render_svg(
                vector::InputMonitorIndicator,
                vector::MonitoringProps {
                    state: vs,
                    width: n.0,
                    height: n.1,
                },
            ),
        ));
    }

    rows.push((
        "pan",
        render_svg(
            traced::PanningKnob,
            traced::PanProps {
                position: 0.0,
                large: false,
                width: n.0,
                height: n.1,
            },
        ),
        render_svg(
            vector::PanningKnob,
            vector::PanProps {
                position: 0.0,
                large: false,
                width: n.0,
                height: n.1,
            },
        ),
    ));
    rows.push((
        "fader cap",
        render_svg(
            traced::VolumeFaderCap,
            traced::FaderCapProps {
                width: n.0,
                height: n.1,
            },
        ),
        render_svg(
            vector::VolumeFaderCap,
            vector::FaderCapProps {
                accent: None,
                width: n.0,
                height: n.1,
            },
        ),
    ));

    // Two bands: traced on top, vector below, aligned column by column so
    // the eye compares like with like.
    let cols = rows.len() as u32;
    let sheet_w = cols * (CELL + PAD) + PAD;
    let sheet_h = CELL * 2 + PAD * 3;
    let mut sheet = image::RgbaImage::from_pixel(sheet_w, sheet_h, image::Rgba([16, 16, 21, 255]));

    for (i, (label, a, b)) in rows.iter().enumerate() {
        let x = PAD + i as u32 * (CELL + PAD);
        for (row, svg) in [(0u32, a), (1, b)] {
            let Some(img) = raster(svg, CELL) else {
                eprintln!("{label} row {row}: nothing rendered");
                continue;
            };
            // Centre each control in its cell; they have different aspects.
            let ox = x + CELL.saturating_sub(img.width()) / 2;
            let oy = PAD + row * (CELL + PAD);
            image::imageops::overlay(&mut sheet, &img, ox as i64, oy as i64);
        }
    }

    let dir = std::path::Path::new("target/compare");
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("traced-vs-vector.png");
    sheet.save(&path).unwrap();
    println!("{} controls compared -> {}", rows.len(), path.display());
    println!("top row: traced (1:1 with Reapertips)   bottom row: vector");
}

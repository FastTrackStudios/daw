//! Writing component-drawn artwork into a REAPER theme.
//!
//! `restyle` recolours the *inherited* art; this replaces it. Each image is
//! rendered from its Dioxus component at 100/150/200 % — from the vector
//! each time, not upscaled — and written over the corresponding PNG.
//!
//! Generated images are the only ones that don't need a pristine snapshot:
//! they have no prior state to preserve, because their source is code.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::theme::{SCALES, ThemeDir};

/// What [`generate`] wrote.
#[derive(Debug, Clone, Default)]
pub struct GenerateReport {
    pub written: Vec<PathBuf>,
    pub failed: Vec<(String, String)>,
}

/// Render every component-drawn image into `theme`.
///
/// Each image's geometry is **measured from the art being replaced** — the
/// pristine snapshot `restyle` keeps, falling back to what's live. Nothing
/// about size or marker layout is authored here, because REAPER blits these
/// where WALTER expects and only reads magenta as geometry when it lands
/// where it expects. Getting that wrong renders the guides as visible
/// magenta in the mixer.
pub fn generate(theme: &ThemeDir, dry_run: bool) -> Result<GenerateReport> {
    let mut report = GenerateReport::default();

    for (name, component) in daw_theme_art::components::registry() {
        for (prefix, _scale) in SCALES {
            let dir = theme.scale_dir(prefix);
            if !dir.is_dir() {
                continue;
            }
            let live = dir.join(format!("{name}.png"));

            // Prefer the pristine snapshot: once restyle has run, the live
            // file is already derived, and measuring a derived file works
            // but means a second source of truth.
            let pristine = theme
                .scale_dir(prefix)
                .join(crate::restyle::SOURCE_DIR)
                .join(format!("{name}.png"));
            let source = if pristine.is_file() { &pristine } else { &live };
            if !source.is_file() {
                continue;
            }

            let measured = match image::open(source) {
                Ok(img) => daw_theme_art::DerivedSpec::from_image(&img.to_rgba8()),
                Err(e) => {
                    report
                        .failed
                        .push((name.into(), format!("read source: {e}")));
                    continue;
                }
            };

            match daw_theme_art::render_for(component, &measured) {
                Ok(img) => {
                    if !dry_run
                        && let Err(e) = img
                            .save(&live)
                            .with_context(|| format!("write {}", live.display()))
                    {
                        report.failed.push((name.into(), format!("{e:#}")));
                        continue;
                    }
                    report.written.push(live);
                }
                Err(e) => report.failed.push((name.into(), format!("{e}"))),
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::path::Path;

    /// A theme whose `mcp_bg` matches REAPER's real shape: 4x4 with two
    /// marker pixels at opposite corners. 100% and 150% exist; 200% does not.
    fn fixture(dir: &Path) {
        std::fs::create_dir_all(dir.join("T").join("150")).unwrap();
        std::fs::write(
            dir.join("T.ReaperTheme"),
            "[color theme]\ncol_main_bg=0\n\n[REAPER]\nui_img=T\n",
        )
        .unwrap();
        std::fs::write(dir.join("T").join("rtconfig.txt"), "version 6.0\n").unwrap();

        let art = RgbaImage::from_fn(4, 4, |x, y| match (x, y) {
            (0, 0) | (3, 3) => Rgba([255, 0, 255, 255]),
            _ => Rgba([0x2a, 0x2a, 0x2a, 255]),
        });
        art.save(dir.join("T").join("mcp_bg.png")).unwrap();
        art.save(dir.join("T").join("150").join("mcp_bg.png"))
            .unwrap();
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fts-gen-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn writes_only_the_scales_and_names_the_theme_has() {
        let d = tmpdir("scales");
        fixture(&d);
        let theme = ThemeDir::open(&d).unwrap();
        let report = generate(&theme, false).unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);

        // mcp_bg at two scales. mcp_volbg is in the registry but absent from
        // this theme, so it is skipped rather than invented.
        assert_eq!(report.written.len(), 2, "{:?}", report.written);
        assert!(!d.join("T").join("mcp_volbg.png").exists());
        assert!(!d.join("T").join("200").exists(), "invented a 200/ folder");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn the_generated_image_keeps_the_sources_geometry() {
        // The failure this replaced: authoring a size produced art REAPER
        // blits wrongly and magenta it draws as visible pixels.
        let d = tmpdir("geometry");
        fixture(&d);
        let theme = ThemeDir::open(&d).unwrap();
        generate(&theme, false).unwrap();

        let img = image::open(d.join("T").join("mcp_bg.png"))
            .unwrap()
            .to_rgba8();
        assert_eq!(img.dimensions(), (4, 4), "size drifted from the source");
        // Markers restored exactly where they were, and nowhere else.
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 255, 255]);
        assert_eq!(img.get_pixel(3, 3).0, [255, 0, 255, 255]);
        assert_ne!(img.get_pixel(3, 0).0, [255, 0, 255, 255]);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn the_art_between_the_markers_is_actually_redrawn() {
        let d = tmpdir("redraw");
        fixture(&d);
        let theme = ThemeDir::open(&d).unwrap();
        generate(&theme, false).unwrap();
        let img = image::open(d.join("T").join("mcp_bg.png"))
            .unwrap()
            .to_rgba8();
        assert_ne!(
            img.get_pixel(1, 1).0,
            [0x2a, 0x2a, 0x2a, 255],
            "component did not replace the source art"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn dry_run_writes_nothing() {
        let d = tmpdir("dry");
        fixture(&d);
        let theme = ThemeDir::open(&d).unwrap();
        let before = std::fs::read(d.join("T").join("mcp_bg.png")).unwrap();
        let report = generate(&theme, true).unwrap();
        assert_eq!(report.written.len(), 2);
        assert_eq!(
            std::fs::read(d.join("T").join("mcp_bg.png")).unwrap(),
            before
        );
        std::fs::remove_dir_all(&d).ok();
    }
}

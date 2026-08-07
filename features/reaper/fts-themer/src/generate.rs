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
pub fn generate(theme: &ThemeDir, dry_run: bool) -> Result<GenerateReport> {
    use daw_theme_art::components as art;

    let items: Vec<(daw_theme_art::ArtSpec, fn() -> dioxus::prelude::Element)> = vec![
        (art::MCP_BG, art::McpBg),
        (art::MCP_VOLBG, art::McpVolBg),
        (art::GEN_BUTTON, art::GenButton),
    ];

    let mut report = GenerateReport::default();
    for (spec, component) in items {
        for (prefix, scale) in SCALES {
            let dir = theme.scale_dir(prefix);
            // Only write a scale the theme actually ships, so generating
            // can't invent a 200/ folder in a theme that has none.
            if !dir.is_dir() {
                continue;
            }
            let path = dir.join(format!("{}.png", spec.name));
            match daw_theme_art::render_png(&spec, scale, component) {
                Ok(img) => {
                    if !dry_run {
                        if let Err(e) = img
                            .save(&path)
                            .with_context(|| format!("write {}", path.display()))
                        {
                            report.failed.push((spec.name.into(), format!("{e:#}")));
                            continue;
                        }
                    }
                    report.written.push(path);
                }
                Err(e) => report.failed.push((spec.name.into(), format!("{e}"))),
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

    fn fixture(dir: &Path) {
        // 100% and 150% exist; 200% deliberately does not.
        std::fs::create_dir_all(dir.join("T").join("150")).unwrap();
        std::fs::write(
            dir.join("T.ReaperTheme"),
            "[color theme]\ncol_main_bg=0\n\n[REAPER]\nui_img=T\n",
        )
        .unwrap();
        std::fs::write(dir.join("T").join("rtconfig.txt"), "version 6.0\n").unwrap();
        RgbaImage::from_fn(1, 1, |_, _| Rgba([0, 0, 0, 0]))
            .save(dir.join("T").join("mcp_bg.png"))
            .unwrap();
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fts-gen-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn writes_each_component_at_the_scales_the_theme_has() {
        let d = tmpdir("scales");
        fixture(&d);
        let theme = ThemeDir::open(&d).unwrap();
        let report = generate(&theme, false).unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);

        // 3 components × 2 present scales; the absent 200/ is skipped.
        assert_eq!(report.written.len(), 6);
        assert!(d.join("T").join("mcp_bg.png").is_file());
        assert!(d.join("T").join("150").join("mcp_bg.png").is_file());
        assert!(!d.join("T").join("200").exists(), "invented a 200/ folder");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn the_generated_image_has_the_specs_size() {
        let d = tmpdir("size");
        fixture(&d);
        let theme = ThemeDir::open(&d).unwrap();
        generate(&theme, false).unwrap();

        let img = image::open(d.join("T").join("150").join("mcp_bg.png"))
            .unwrap()
            .to_rgba8();
        assert_eq!(
            img.dimensions(),
            daw_theme_art::components::MCP_BG.size_at(1.5)
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
        assert_eq!(report.written.len(), 6);
        assert_eq!(
            std::fs::read(d.join("T").join("mcp_bg.png")).unwrap(),
            before
        );
        std::fs::remove_dir_all(&d).ok();
    }
}

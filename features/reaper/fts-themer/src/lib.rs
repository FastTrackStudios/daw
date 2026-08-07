//! REAPER theme editing — the write half.
//!
//! [`daw_theme_reaper`] already *reads* an unpacked theme (palette, rtconfig
//! globals and `define_parameter`s, sliced image atlases) for the FTS UI to
//! render. This crate is the other direction: change a color, generate an
//! accent variant, and push the result into a running REAPER.
//!
//! Everything is library-first; the `fts-themer` binary and the web GUI are
//! both thin shells over these types.
//!
//! ```no_run
//! use fts_themer::{color::Rgb, ThemeDir};
//!
//! let mut theme = ThemeDir::open("features/reaper/fts-theme")?;
//! theme.ini_mut().set_color("col_cursor", Rgb::parse_hex("#00b0f9")?);
//! theme.save_ini()?;
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod color;
pub mod groups;
pub mod ini;
pub mod recolor;
pub mod rtconfig;
pub mod theme;

pub use color::Rgb;
pub use groups::Group;
pub use ini::ThemeIni;
pub use theme::{ThemeDir, ACCENT_IMAGE, SCALES};

use anyhow::Result;
use std::path::PathBuf;

/// Generate an accent variant end to end: recolor the artwork at every DPI
/// scale *and* register the `Layout` blocks that make REAPER offer it.
///
/// Doing only half of this is a silent no-op from the user's point of view —
/// images with no layout never render, a layout with no images renders blank —
/// so the two steps live behind one call.
pub fn add_accent(
    theme: &ThemeDir,
    name: &str,
    color: Rgb,
    source: &str,
    keep_lightness: bool,
) -> Result<AccentReport> {
    let images = theme.generate_accent(name, color, source, keep_lightness)?;

    let path = theme.rtconfig_path();
    let text = std::fs::read_to_string(&path)?;
    let (patched, tiers) = rtconfig::add_accent_layout(&text, name, name)?;
    if tiers > 0 {
        std::fs::write(&path, patched)?;
    }

    Ok(AccentReport {
        images,
        layouts: tiers,
        rtconfig: path,
    })
}

/// What [`add_accent`] wrote.
#[derive(Debug, Clone)]
pub struct AccentReport {
    /// PNGs written, one per DPI scale.
    pub images: Vec<PathBuf>,
    /// `Layout` blocks added (0 if the accent was already registered).
    pub layouts: usize,
    /// The rtconfig that was patched.
    pub rtconfig: PathBuf,
}

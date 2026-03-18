//! Icon generation for REAPER instance launchers.
//!
//! Renders badged application icons from an embedded SVG template using `resvg`.
//! Each instance gets a distinct colored badge (pill shape) with text, rendered
//! at multiple sizes for XDG icon theme installation.
//!
//! Requires the `icon-gen` feature flag (`resvg` dependency).

#[cfg(feature = "icon-gen")]
use std::path::PathBuf;

/// Configuration for generating a badged icon.
pub struct IconConfig {
    /// Badge text, e.g., "TEST", "GUITAR".
    pub badge_text: String,
    /// Badge background color as RGB tuple.
    pub color: (u8, u8, u8),
    /// Pixel sizes to render, e.g., `[48, 128, 256]`.
    pub sizes: Vec<u32>,
}

/// Generate PNGs at the requested sizes and install them to the XDG icon theme
/// directory (`~/.local/share/icons/hicolor/{size}x{size}/apps/{id}.png`).
///
/// Requires the `icon-gen` feature.
#[cfg(feature = "icon-gen")]
pub fn generate_and_install_icons(id: &str, config: &IconConfig) -> Result<(), String> {
    let home =
        std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;

    for &size in &config.sizes {
        let dir =
            PathBuf::from(&home).join(format!(".local/share/icons/hicolor/{size}x{size}/apps"));
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create {}: {e}", dir.display()))?;

        let png_path = dir.join(format!("{id}.png"));
        let png_data = render_icon_png(size, config)?;
        std::fs::write(&png_path, &png_data)
            .map_err(|e| format!("Failed to write {}: {e}", png_path.display()))?;
    }

    // Refresh icon cache if gtk-update-icon-cache is available
    let icon_base = PathBuf::from(&home).join(".local/share/icons/hicolor");
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .args(["-f", "-t"])
        .arg(&icon_base)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    Ok(())
}

/// Render a single icon at the given pixel size to a PNG byte vector.
#[cfg(feature = "icon-gen")]
fn render_icon_png(size: u32, config: &IconConfig) -> Result<Vec<u8>, String> {
    let svg_source = build_svg(size, config);

    let opts = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(&svg_source, &opts)
        .map_err(|e| format!("Failed to parse SVG: {e}"))?;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)
        .ok_or_else(|| "Failed to create pixmap".to_string())?;

    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );

    pixmap
        .encode_png()
        .map_err(|e| format!("Failed to encode PNG: {e}"))
}

/// Build an SVG string for the icon at the given size.
///
/// Draws a REAPER-style rounded square with a colored badge pill at the bottom.
#[cfg(feature = "icon-gen")]
fn build_svg(size: u32, config: &IconConfig) -> String {
    let (r, g, b) = config.color;
    let badge_text = &config.badge_text;
    let s = size as f32;

    // Icon dimensions
    let margin = s * 0.06;
    let icon_size = s - margin * 2.0;
    let corner_radius = icon_size * 0.18;

    // Badge dimensions
    let badge_w = icon_size * 0.55;
    let badge_h = icon_size * 0.16;
    let badge_x = (s - badge_w) / 2.0;
    let badge_y = s - margin - badge_h - icon_size * 0.08;
    let badge_rx = badge_h / 2.0;

    // Font size scales with icon
    let font_size = badge_h * 0.55;

    // Tint overlay (30% opacity)
    let tint_r = r;
    let tint_g = g;
    let tint_b = b;

    // Badge border (darker shade)
    let border_r = (r as f32 * 0.5) as u8;
    let border_g = (g as f32 * 0.5) as u8;
    let border_b = (b as f32 * 0.5) as u8;

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">
  <defs>
    <filter id="badge-shadow" x="-20%" y="-20%" width="140%" height="140%">
      <feDropShadow dx="0" dy="1" stdDeviation="2" flood-color="rgba(0,0,0,0.6)"/>
    </filter>
  </defs>

  <!-- Base icon: dark rounded square -->
  <rect x="{margin}" y="{margin}" width="{icon_size}" height="{icon_size}"
        rx="{corner_radius}" ry="{corner_radius}"
        fill="#2a2a2a"/>

  <!-- Tint overlay -->
  <rect x="{margin}" y="{margin}" width="{icon_size}" height="{icon_size}"
        rx="{corner_radius}" ry="{corner_radius}"
        fill="rgba({tint_r},{tint_g},{tint_b},0.3)"/>

  <!-- REAPER-style waveform hint (subtle) -->
  <line x1="{wf_x1}" y1="{wf_cy}" x2="{wf_x2}" y2="{wf_cy}"
        stroke="rgba(255,255,255,0.15)" stroke-width="{wf_stroke}"/>

  <!-- Badge pill -->
  <rect x="{badge_x}" y="{badge_y}" width="{badge_w}" height="{badge_h}"
        rx="{badge_rx}" ry="{badge_rx}"
        fill="rgba({r},{g},{b},0.95)"
        stroke="rgb({border_r},{border_g},{border_b})" stroke-width="1.5"
        filter="url(#badge-shadow)"/>

  <!-- Badge text -->
  <text x="{text_x}" y="{text_y}"
        font-family="system-ui, -apple-system, sans-serif"
        font-weight="bold" font-size="{font_size}"
        fill="white" text-anchor="middle" dominant-baseline="central">
    {badge_text}
  </text>
</svg>"##,
        size = size,
        margin = margin,
        icon_size = icon_size,
        corner_radius = corner_radius,
        tint_r = tint_r,
        tint_g = tint_g,
        tint_b = tint_b,
        r = r,
        g = g,
        b = b,
        border_r = border_r,
        border_g = border_g,
        border_b = border_b,
        badge_x = badge_x,
        badge_y = badge_y,
        badge_w = badge_w,
        badge_h = badge_h,
        badge_rx = badge_rx,
        font_size = font_size,
        text_x = s / 2.0,
        text_y = badge_y + badge_h / 2.0,
        badge_text = badge_text,
        wf_x1 = s * 0.25,
        wf_x2 = s * 0.75,
        wf_cy = s * 0.42,
        wf_stroke = s * 0.02,
    )
}

/// Known rig appearances for the standard FTS rig types.
///
/// Returns `(color_rgb, badge_text)` for a given rig type identifier.
pub fn rig_appearance(rig_type: &str) -> Option<((u8, u8, u8), &'static str)> {
    Some(match rig_type {
        "guitar" => ((0x3b, 0x82, 0xf6), "GUITAR"),
        "bass" => ((0xea, 0xb3, 0x08), "BASS"),
        "keys" => ((0x22, 0xc5, 0x5e), "KEYS"),
        "drums" => ((0xef, 0x44, 0x44), "DRUMS"),
        "drum-enhancement" => ((0xf9, 0x73, 0x16), "DRUM+"),
        "vocals" => ((0xec, 0x48, 0x99), "VOCALS"),
        "session" => ((0x66, 0x9e, 0xe6), "TRACKS"),
        "testing" => ((0x4d, 0x4d, 0x4d), "TEST"),
        _ => return None,
    })
}

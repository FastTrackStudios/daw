//! Components → REAPER sprite strips, markers and all.
//!
//! [`crate::render::render_for`] already stamps a source image's marker
//! pixels back after rasterising, but it only accepts components taking
//! [`crate::components::ArtProps`]. The vector controls take their own
//! props — a mute button has an `on`, a routing button has sends and
//! receives — so nothing connected them to a themeable PNG.
//!
//! Two things have to happen that do not for a single drawing:
//!
//! **Cells.** REAPER packs a control's interaction states side by side:
//! `mcp_mute_on` is three 21×20 buttons in one 63×20 file. Each cell is
//! rendered separately and composited, because one component draws one
//! button — drawing the strip would mean the component knowing it is
//! being exported, which is exactly the coupling the vector rewrite was
//! for. [`composite_cells`] is shared with the traced path, which needs
//! it for the same reason.
//!
//! **Markers.** The magenta and yellow pixels are WALTER geometry, not
//! art: magenta bounds the region that must not stretch, yellow the outer
//! extent. A rasteriser antialiases, and a marker one shade off pure
//! magenta stops being a marker — REAPER silently gives up on slicing and
//! the image smears when the panel resizes. So they are never drawn and
//! never interpolated: they are copied from the source verbatim, after
//! compositing.

use image::RgbaImage;

use crate::derive::DerivedSpec;
use crate::generated;
use crate::render::{RenderError, rasterise, render_svg};
use crate::vector_controls as v;

/// The pointer state a strip's nth cell shows.
fn interaction(cell: usize) -> v::Interaction {
    match cell {
        1 => v::Interaction::Hover,
        2 => v::Interaction::Pressed,
        _ => v::Interaction::Normal,
    }
}

/// One cell of `name`, as SVG markup, or `None` if no vector control
/// draws that image yet.
///
/// The inverse of the mapping in [`crate::mixer_controls`], which goes
/// from a control's state to the image REAPER blits.
pub fn cell_markup(name: &str, at: v::Interaction) -> Option<String> {
    let n = (None, None);
    let rec = |state| {
        render_svg(
            v::RecordArmButton,
            v::RecordArmProps {
                state,
                width: n.0,
                height: n.1,
                at,
            },
        )
    };
    let solo = |state| {
        render_svg(
            v::SoloButton,
            v::SoloProps {
                state,
                width: n.0,
                height: n.1,
                at,
            },
        )
    };
    let fx = |state| {
        render_svg(
            v::FxButton,
            v::FxProps {
                state,
                width: n.0,
                height: n.1,
                at,
            },
        )
    };
    let mon = |state| {
        render_svg(
            v::InputMonitorIndicator,
            v::MonitoringProps {
                state,
                width: n.0,
                height: n.1,
                at,
            },
        )
    };
    let mute = |on| {
        render_svg(
            v::MuteButton,
            v::ToggleProps {
                on,
                width: n.0,
                height: n.1,
                at,
            },
        )
    };
    let io = |has_sends, has_receives, disabled| {
        render_svg(
            v::RoutingButton,
            v::RoutingProps {
                has_sends,
                has_receives,
                disabled,
                width: n.0,
                height: n.1,
                at,
            },
        )
    };

    Some(match name {
        "mcp_recarm_off" => rec(v::RecordArm::Off),
        "mcp_recarm_on" => rec(v::RecordArm::On),
        "mcp_recarm_norec" => rec(v::RecordArm::NoRecord),
        "mcp_recarm_auto" => rec(v::RecordArm::Auto),
        "mcp_recarm_auto_on" => rec(v::RecordArm::AutoOn),
        "mcp_recarm_auto_norec" => rec(v::RecordArm::AutoNoRecord),

        "mcp_mute_off" => mute(false),
        "mcp_mute_on" => mute(true),

        "mcp_solo_off" => solo(v::Solo::Off),
        "mcp_solo_on" => solo(v::Solo::On),
        "mcp_solodefeat_on" => solo(v::Solo::Defeat),

        "mcp_fx_empty" => fx(v::FxChain::Empty),
        "mcp_fx_norm" => fx(v::FxChain::Active),
        "mcp_fx_dis" => fx(v::FxChain::Bypassed),

        "mcp_io" => io(false, false, false),
        "mcp_io_dis" => io(false, false, true),
        "mcp_io_s" => io(true, false, false),
        "mcp_io_s_dis" => io(true, false, true),
        "mcp_io_r" => io(false, true, false),
        "mcp_io_r_dis" => io(false, true, true),
        "mcp_io_s_r" => io(true, true, false),
        "mcp_io_s_r_dis" => io(true, true, true),

        "mcp_monitor_off" => mon(v::Monitoring::Off),
        "mcp_monitor_on" => mon(v::Monitoring::On),
        "mcp_monitor_auto" => mon(v::Monitoring::Auto),

        "mcp_pan_knob_small" => render_svg(
            v::PanningKnob,
            v::PanProps {
                position: 0.0,
                large: false,
                width: n.0,
                height: n.1,
            },
        ),
        "mcp_pan_knob" => render_svg(
            v::PanningKnob,
            v::PanProps {
                position: 0.0,
                large: true,
                width: n.0,
                height: n.1,
            },
        ),
        "mcp_volthumb" => render_svg(
            v::VolumeFaderCap,
            v::FaderCapProps {
                accent: None,
                width: n.0,
                height: n.1,
            },
        ),
        "mcp_volbg" => render_svg(
            v::VolumeFaderTrack,
            v::FaderCapProps {
                accent: None,
                width: n.0,
                height: n.1,
            },
        ),
        _ => return None,
    })
}

/// Which theme images the vector controls can generate.
pub fn generatable() -> Vec<&'static str> {
    generated::ALL
        .iter()
        .map(|a| a.name)
        .filter(|n| cell_markup(n, v::Interaction::Normal).is_some())
        .collect()
}

/// Lay a strip out cell by cell, then stamp the source's markers back.
///
/// `markup(i, width)` draws cell `i` at `width` x `spec.height`, at the
/// cell positions **measured from the source** — see
/// [`crate::derive::cell_bounds`], because they are not an even division
/// of the file width.
///
/// Callers need this even when they are not vector controls: rendering a
/// strip from one drawing scaled to the full width stretches a single
/// state across every cell, which looks like a blurry button rather than
/// like the wrong cell count, so it survives review.
pub fn composite_cells(
    spec: &DerivedSpec,
    markup: impl Fn(usize, u32) -> Result<String, RenderError>,
) -> Result<RgbaImage, RenderError> {
    let mut out = RgbaImage::new(spec.width, spec.height);
    for (i, &(x, w)) in spec.cells.iter().enumerate() {
        let cell = rasterise(&markup(i, w)?, w, spec.height)?;
        image::imageops::overlay(&mut out, &cell, x as i64, 0);
    }
    // Last, and never drawn: see the module note. Stamping before the
    // composite would let a later cell paint over a marker.
    spec.stamp(&mut out);
    Ok(out)
}

/// Draw `name` as REAPER expects it: every cell, at the source's exact
/// size, with the source's marker pixels stamped back.
///
/// `spec` carries the geometry measured from the image being replaced —
/// never authored, because REAPER only reads magenta as geometry when it
/// lands where the layout expects it.
pub fn render_control(name: &str, spec: &DerivedSpec) -> Result<RgbaImage, RenderError> {
    composite_cells(spec, |i, _w| {
        cell_markup(name, interaction(i))
            .ok_or_else(|| RenderError::Svg(format!("no vector control draws {name}")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derive::MARKERS;

    /// Markers are copied, never rendered — the whole point of the module.
    #[test]
    fn markers_land_exactly_where_the_source_had_them() {
        let spec = DerivedSpec {
            width: 63,
            height: 20,
            markers: vec![
                (0, 0, MARKERS[0]),
                (62, 19, MARKERS[1]),
                // Deliberately mid-button: a stamped marker must win over
                // whatever the component drew there.
                (10, 10, MARKERS[0]),
            ],
            cells: vec![(0, 21), (21, 21), (42, 21)],
        };
        let img = render_control("mcp_mute_on", &spec).expect("render");
        assert_eq!(img.dimensions(), (63, 20));
        for (x, y, rgba) in &spec.markers {
            assert_eq!(
                img.get_pixel(*x, *y).0,
                *rgba,
                "marker at {x},{y} was not stamped back"
            );
        }
    }

    /// The three cells of a strip are the three pointer states, not one
    /// button repeated.
    #[test]
    fn a_strip_carries_distinct_interaction_states() {
        let a = cell_markup("mcp_mute_on", v::Interaction::Normal).unwrap();
        let b = cell_markup("mcp_mute_on", v::Interaction::Hover).unwrap();
        let c = cell_markup("mcp_mute_on", v::Interaction::Pressed).unwrap();
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn an_image_no_vector_control_draws_is_reported_not_guessed() {
        assert!(cell_markup("mcp_bg", v::Interaction::Normal).is_none());
        let spec = DerivedSpec {
            width: 4,
            height: 4,
            markers: vec![],
            cells: vec![(0, 4)],
        };
        assert!(render_control("mcp_bg", &spec).is_err());
    }

    /// Everything the mixer draws should be generatable, or the theme has
    /// a hole in it that only shows up as an un-retinted button.
    #[test]
    fn the_whole_mixer_strip_can_be_generated() {
        let have = generatable();
        for want in [
            "mcp_mute_on",
            "mcp_solo_on",
            "mcp_fx_norm",
            "mcp_recarm_on",
            "mcp_io_s_r",
            "mcp_monitor_on",
            "mcp_pan_knob_small",
            "mcp_volthumb",
        ] {
            assert!(have.contains(&want), "{want} has no vector control");
        }
    }
}

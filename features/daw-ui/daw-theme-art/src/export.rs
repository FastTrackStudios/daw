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

    // Cell sizes, measured from the art. The two families are *not* the
    // same drawings at two sizes: the track panel's ring has no housing
    // and is proportionally larger for it, its routing lanes sit in a row
    // rather than stacked, and its monitor icon radiates right rather
    // than down. Same components, turned and resized.
    let track = name.starts_with("track_");
    let axis = if track { v::Axis::Horizontal } else { v::Axis::Vertical };

    let rec = |state| {
        render_svg(
            v::RecordArmButton,
            v::RecordArmProps {
                state,
                cell: if track { (20.0, 20.0) } else { (36.0, 24.0) },
                housing: !track,
                width: n.0,
                height: n.1,
                at,
            },
        )
    };
    let label_cell = if track { (22.0, 24.0) } else { (21.0, 20.0) };
    // Traced: the track panel's buttons occupy rows 1..20 of a 24-row
    // cell; the mixer's fill theirs.
    let label_body = if track {
        (1.0 / 24.0, 20.0 / 24.0)
    } else {
        (0.0, 1.0)
    };
    let mute = |on| {
        render_svg(
            v::MuteButton,
            v::ToggleProps {
                on,
                cell: label_cell,
                body: label_body,
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
                cell: label_cell,
                body: label_body,
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
                cell: if track { (20.0, 22.0) } else { (28.0, 22.0) },
                body: if track { 20.0 / 22.0 } else { 18.0 / 22.0 },
                scrim: track,
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
                cell: if track { (16.0, 24.0) } else { (21.0, 20.0) },
                axis,
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
                cell: if track { (29.0, 22.0) } else { (23.0, 32.0) },
                axis,
                width: n.0,
                height: n.1,
                at,
            },
        )
    };

    // `track_fx*_h` is 50x22 in three cells, `_v` 56x22 — the track
    // panel's FX bypass toggle, which has no `mcp_` twin at all.
    let byp = |state, cell, scrim| {
        render_svg(
            v::FxBypassToggle,
            v::FxBypassProps {
                state,
                cell,
                scrim,
                width: n.0,
                height: n.1,
                at,
            },
        )
    };

    // Both families answer to the same eight controls, so match on the
    // part after the prefix rather than writing every name twice.
    let stem = name
        .strip_prefix("mcp_")
        .or_else(|| name.strip_prefix("track_"))?;

    Some(match stem {
        "fxempty_h" => byp(v::FxBypass::Empty, (17.0, 22.0), true),
        "fxon_h" => byp(v::FxBypass::On, (17.0, 22.0), true),
        "fxoff_h" => byp(v::FxBypass::Off, (17.0, 22.0), true),
        "fxempty_v" => byp(v::FxBypass::Empty, (19.0, 22.0), false),
        "fxon_v" => byp(v::FxBypass::On, (19.0, 22.0), false),
        "fxoff_v" => byp(v::FxBypass::Off, (19.0, 22.0), false),

        "recarm_off" => rec(v::RecordArm::Off),
        "recarm_on" => rec(v::RecordArm::On),
        "recarm_norec" => rec(v::RecordArm::NoRecord),
        "recarm_auto" => rec(v::RecordArm::Auto),
        "recarm_auto_on" => rec(v::RecordArm::AutoOn),
        "recarm_auto_norec" => rec(v::RecordArm::AutoNoRecord),

        "mute_off" => mute(false),
        "mute_on" => mute(true),

        "solo_off" => solo(v::Solo::Off),
        "solo_on" => solo(v::Solo::On),
        "solodefeat_on" => solo(v::Solo::Defeat),

        "fx_empty" => fx(v::FxChain::Empty),
        "fx_norm" => fx(v::FxChain::Active),
        "fx_dis" => fx(v::FxChain::Bypassed),

        "io" => io(false, false, false),
        "io_dis" => io(false, false, true),
        "io_s" => io(true, false, false),
        "io_s_dis" => io(true, false, true),
        "io_r" => io(false, true, false),
        "io_r_dis" => io(false, true, true),
        "io_s_r" => io(true, true, false),
        "io_s_r_dis" => io(true, true, true),

        "monitor_off" => mon(v::Monitoring::Off),
        "monitor_on" => mon(v::Monitoring::On),
        "monitor_auto" => mon(v::Monitoring::Auto),

        // Mixer-only: the knobs and the fader live in one panel.
        "pan_knob_small" => render_svg(
            v::PanningKnob,
            v::PanProps {
                position: 0.0,
                large: false,
                width: n.0,
                height: n.1,
            },
        ),
        "pan_knob_large" => render_svg(
            v::PanningKnob,
            v::PanProps {
                position: 0.0,
                large: true,
                width: n.0,
                height: n.1,
            },
        ),
        "volthumb" => render_svg(
            v::VolumeFaderCap,
            v::FaderCapProps {
                accent: None,
                width: n.0,
                height: n.1,
            },
        ),
        "volbg" => render_svg(
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

/// Images a vector control *should* draw but none does yet.
///
/// The `track_*` family is the track panel's own set — its own sizes, its
/// own drawings — and replacing the `mcp_*` mixer art leaves all of it
/// untouched. That is invisible in a mixer screenshot and obvious in the
/// track panel, so it is worth being able to ask.
pub fn missing_twins() -> Vec<&'static str> {
    generated::ALL
        .iter()
        .map(|a| a.name)
        .filter(|n| n.starts_with("track_") && cell_markup(n, v::Interaction::Normal).is_none())
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
    // Trust the measured split only when it agrees with what this control
    // actually draws. Detection needs a strip's cells to resemble each
    // other, which fails on the FX bypass toggle — pill, plus, plus — and
    // reported it as a single drawing, so it was stretched across its own
    // width and vanished into the strip.
    let mut spec = spec.clone();
    if spec.cells.len() != states(name) {
        let mut img = RgbaImage::new(spec.width, spec.height);
        spec.stamp(&mut img);
        spec.cells = crate::derive::even_cells(&img, states(name) as u32);
    }

    composite_cells(&spec, |i, _w| {
        cell_markup(name, interaction(i))
            .ok_or_else(|| RenderError::Svg(format!("no vector control draws {name}")))
    })
}

/// How many sprite cells a control's image holds.
///
/// Knowledge, not a measurement: every button here draws normal, hover
/// and pressed, and the knobs and the fader draw one thing.
fn states(name: &str) -> usize {
    let stem = name
        .strip_prefix("mcp_")
        .or_else(|| name.strip_prefix("track_"))
        .unwrap_or(name);
    match stem {
        "pan_knob_small" | "pan_knob_large" | "volthumb" | "volbg" => 1,
        _ => 3,
    }
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

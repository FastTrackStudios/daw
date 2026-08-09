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
    // `tcp_` is the track control panel too — REAPER names most of its
    // controls `track_*` and a handful `tcp_*`, and where both exist for
    // the same control they are the same drawing: `tcp_solodefeat_on` and
    // `track_solodefeat_on` are byte-identical.
    let track = name.starts_with("track_") || name.starts_with("tcp_");
    let axis = if track { v::Axis::Horizontal } else { v::Axis::Vertical };
    // The FX control's two halves are named for their *layout*, not their
    // panel: `track_fx*_h` is the track panel's and `track_fx*_v` the
    // mixer's, despite both carrying the `track_` prefix.
    let family = if name.ends_with("_v") || !track {
        v::FxFamily::Mixer
    } else {
        v::FxFamily::TrackPanel
    };

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
    // Measured, not assumed: the track panel's label cells are 21 wide
    // starting at x1, not 22 starting at x0. A viewBox one unit wider
    // than the cell it lands in squeezes every coordinate inward, which
    // is a fraction of a pixel at the edges and a whole one at the far
    // side — the source of the last stubborn `bottom -1`.
    let label_cell = if track { (21.0, 24.0) } else { (21.0, 20.0) };
    // Traced: the track panel's buttons occupy rows 1..20 of a 24-row
    // cell; the mixer's fill theirs.
    // The legend is not one grey. It brightens when the button lights,
    // and the track panel swings much further than the mixer:
    //
    //     mixer   mute 204/204   solo 204/217   defeat 217
    //     track   mute 183/242   solo 203/255   defeat 255
    //
    // Read as a single #f2f2f2 the track panel's unlit buttons came out
    // sixty levels hot, which is most of why they were the worst two
    // images in the set by pixel error.
    let theme = daw_theme::Theme::default();
    // Measured off the `off` cells: #464646 in the mixer, #4e4e4e in the
    // track panel, both falling about 12% over the button's height.
    let unlit = Some(theme.chrome.hardware.shade(if track { 0.078 } else { 0.036 }));
    let legend = |lit: bool, solo: bool| {
        let up = match (track, lit, solo) {
            (false, _, false) => 0.45,
            (false, false, true) => 0.45,
            (false, true, true) => 0.59,
            (true, false, false) => 0.23,
            (true, true, false) => 0.86,
            (true, false, true) => 0.44,
            (true, true, true) => 1.0,
        };
        Some(theme.chrome.hardware_mark.shade(up))
    };
    let label_body = if track {
        (1.0 / 24.0, 20.0 / 24.0)
    } else {
        (0.0, 1.0)
    };
    let mute = |on| {
        render_svg(
            v::MuteButton,
            v::ToggleProps {
                // Mute's hover is the gentlest of the three — see `ink`.
                hover: 0.25,
                // 0.11, and applied as a scale — measured 0.89 on every
                // channel from the top of the face to the bottom.
                depth: 0.11,
                on,
                cell: label_cell,
                body: label_body,
                legend: legend(on, false),
                unlit,
                // The track panel's pressed cell is identical to its
                // normal one; the mixer's is darker.
                sinks: !track,
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
                hover: 0.35,
                state,
                cell: label_cell,
                body: label_body,
                legend: legend(state != v::Solo::Off, true),
                unlit,
                // The track panel's pressed cell is identical to its
                // normal one; the mixer's is darker.
                sinks: !track,
                // Solo's red is pinned; only its green falls, by 11%.
                depth: 0.11,
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
                family,
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
                // 15, not 16: the measured cell is (origin 1, width 15),
                // and a 16-wide viewBox squeezed into it puts every
                // coordinate half a pixel right of where it was measured.
                cell: if track { (15.0, 24.0) } else { (21.0, 20.0) },
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
                cell: if track { (28.0, 22.0) } else { (23.0, 32.0) },
                axis,
                width: n.0,
                height: n.1,
                at,
            },
        )
    };

    // `track_fx*_h` is 50x22 in three cells, `_v` 56x22 — the track
    // panel's FX bypass toggle, which has no `mcp_` twin at all.
    let byp = |state| {
        render_svg(
            v::FxBypassToggle,
            v::FxBypassProps {
                state,
                family,
                width: n.0,
                height: n.1,
                at,
            },
        )
    };

    // Track-panel-only controls. The mixer strip has no room for these,
    // so unlike everything above there is no twin to keep in step.
    let env = |mode| {
        render_svg(
            v::EnvelopeButton,
            v::EnvelopeProps {
                mode,
                cell: (20.0, 20.0),
                width: n.0,
                height: n.1,
                at,
            },
        )
    };
    let phase = |inverted| {
        render_svg(
            v::PhaseButton,
            v::PhaseProps {
                inverted,
                cell: if track { (16.0, 20.0) } else { (16.0, 18.0) },
                width: n.0,
                height: n.1,
                at,
            },
        )
    };

    let recmode = |mode| {
        render_svg(
            v::RecordModeButton,
            v::RecordModeProps {
                mode,
                cell: (20.0, 20.0),
                width: n.0,
                height: n.1,
                at,
            },
        )
    };
    let fcomp = |state| {
        render_svg(
            v::FolderCompactButton,
            v::FolderCompactProps {
                state,
                cell: (17.0, 13.0),
                width: n.0,
                height: n.1,
                at,
            },
        )
    };
    let fx_in = |loaded| {
        render_svg(
            v::FxInButton,
            v::FxInProps {
                loaded,
                cell: (29.0, 20.0),
                width: n.0,
                height: n.1,
                at,
            },
        )
    };
    let folder = |state| {
        render_svg(
            v::FolderButton,
            v::FolderProps {
                state,
                cell: (54.0, 14.0),
                width: n.0,
                height: n.1,
                at,
            },
        )
    };

    // The transport bar. Its buttons are the one family here with no
    // mixer/track split — there is only one of it — so they match on the
    // whole name rather than on a stem.
    let tr = |glyph, on| {
        render_svg(
            v::TransportButton,
            v::TransportProps {
                glyph,
                on,
                cell: if glyph == v::TransportGlyph::Repeat {
                    (32.0, 24.0)
                } else {
                    (36.0, 26.0)
                },
                width: n.0,
                height: n.1,
                at,
            },
        )
    };
    {
        use v::TransportGlyph as G;
        let hit = match name {
            "transport_home" => Some(tr(G::Home, false)),
            "transport_previous" => Some(tr(G::Previous, false)),
            "transport_stop" => Some(tr(G::Stop, false)),
            "transport_play" => Some(tr(G::Play, false)),
            "transport_play_on" => Some(tr(G::Play, true)),
            "transport_pause" => Some(tr(G::Pause, false)),
            "transport_pause_on" => Some(tr(G::Pause, true)),
            "transport_next" => Some(tr(G::Next, false)),
            "transport_end" => Some(tr(G::End, false)),
            "transport_record" => Some(tr(G::Record, false)),
            "transport_record_on" => Some(tr(G::Record, true)),
            "transport_record_item" => Some(tr(G::RecordItem, false)),
            "transport_record_item_on" => Some(tr(G::RecordItem, true)),
            "transport_record_loop" => Some(tr(G::RecordLoop, false)),
            "transport_record_loop_on" => Some(tr(G::RecordLoop, true)),
            "transport_repeat_off" => Some(tr(G::Repeat, false)),
            "transport_repeat_on" => Some(tr(G::Repeat, true)),
            "transport_play_sync" => Some(tr(G::PlaySync, false)),
            "transport_play_sync_on" => Some(tr(G::PlaySync, true)),
            _ => None,
        };
        if let Some(markup) = hit {
            return Some(markup);
        }
    }
    // The FX and send lists. Three pills stacked vertically, measured
    // top to bottom; the value beside each shade is what it is today.
    {
        let g = |v: f32| theme.chrome.hardware.shade(v);
        let flat = |c: daw_theme::Color, a: f32| (c, c, a);
        let strip = |pills: Vec<v::ListPill>, cell, rows, pill, edge, inset| {
            render_svg(
                v::ListStrip,
                v::ListStripProps {
                    pills,
                    cell,
                    rows,
                    pill,
                    edge,
                    inset,
                    width: n.0,
                    height: n.1,
                    at,
                },
            )
        };
        let fx = (38.0, 53.0);
        let send = (38.0, 50.0);
        let hit = match name {
            "mcp_fxlist_norm" => Some(strip(
                // The one gradient in the set: #575757 down to #434343,
                // and each state a shade lighter than the last.
                vec![
                    (g(0.09), g(-0.10), 1.0),
                    (g(0.20), g(0.13), 1.0),
                    (g(0.55), g(0.37), 1.0),
                ],
                fx,
                (2.0, 17.0),
                15.0,
                None,
                1.0,
            )),
            // Bypassed and offline are the same drawing.
            "mcp_fxlist_byp" | "mcp_fxlist_off" => Some(strip(
                vec![
                    flat(g(-0.40), 1.0), // #262626
                    flat(g(-0.19), 1.0), // #333333
                    flat(g(-0.40), 1.0),
                ],
                fx,
                (2.0, 17.0),
                15.0,
                None,
                1.0,
            )),
            "mcp_fxlist_empty" => Some(strip(
                vec![
                    flat(g(-0.70), 0.149), // #131313
                    flat(g(1.0), 0.137),
                    flat(g(1.0), 0.204),
                ],
                fx,
                (2.0, 17.0),
                15.0,
                None,
                1.0,
            )),
            "mcp_sendlist_norm" => Some(strip(
                vec![
                    flat(g(-0.40), 1.0), // #262626
                    flat(g(-0.32), 1.0), // #2b2b2b
                    flat(g(-0.48), 1.0), // #212121
                ],
                send,
                (2.0, 16.0),
                14.0,
                None,
                0.0,
            )),
            // A muted send is not a shade of `mute` — it is the mute red
            // mixed most of the way into the plate, which desaturates it
            // as well as darkening it. Shaded, the red stayed pure and
            // came out twice as saturated as the art.
            "mcp_sendlist_mute" => Some(strip(
                vec![
                    flat(theme.signal.mute.mix(g(-0.40), 0.66), 1.0), // #54363c
                    flat(theme.signal.mute.mix(g(-0.40), 0.60), 1.0), // #5f3d44
                    flat(theme.signal.mute.mix(g(-0.40), 0.66), 1.0),
                ],
                send,
                (2.0, 16.0),
                14.0,
                None,
                0.0,
            )),
            "mcp_sendlist_empty" => Some(strip(
                vec![
                    flat(g(-1.0), 0.098),
                    flat(g(1.0), 0.137),
                    flat(g(-1.0), 0.098),
                ],
                send,
                (2.0, 16.0),
                14.0,
                None,
                0.0,
            )),
            "mcp_sendlist_midihw" => Some(strip(
                vec![
                    flat(g(-0.35), 1.0), // #292929
                    flat(g(-0.22), 1.0), // #313131
                    flat(g(-0.35), 1.0),
                ],
                send,
                (2.0, 16.0),
                14.0,
                Some(theme.chrome.accent.shade(-0.42)), // #01659f
                0.0,
            )),
            _ => None,
        };
        if let Some(markup) = hit {
            return Some(markup);
        }
    }

    // The panel backgrounds. Measured value beside each shade, because the
    // shade is what retints and the value is what it has to match today.
    {
        let g = |v: f32| theme.chrome.hardware.shade(v);
        let plate = |bands: Vec<v::Band>, cell, inset| {
            render_svg(
                v::PanelPlate,
                v::PlateProps { bands, cell, inset, width: n.0, height: n.1, at },
            )
        };
        let sunk = g(-0.19); // #333333
        let lit = g(0.09); // #515151
        let deep = g(-0.40); // #262626
        let hit = match name {
            "mcp_mainbg" => Some(plate(vec![(0.0, 6.0, sunk, 1.0)], (6.0, 6.0), 0.0)),
            "mcp_mainbgsel" => Some(plate(vec![(0.0, 6.0, lit, 1.0)], (6.0, 6.0), 0.0)),
            "mcp_bg" => Some(plate(vec![(1.0, 2.0, sunk, 1.0)], (4.0, 4.0), 1.0)),
            "mcp_bgsel" => Some(plate(vec![(1.0, 1.0, sunk, 1.0)], (4.0, 3.0), 1.0)),
            "mcp_extmixbg" => Some(plate(vec![(0.0, 3.0, sunk, 1.0)], (3.0, 3.0), 0.0)),
            "mcp_extmixbgsel" => {
                Some(plate(vec![(1.0, 1.0, sunk, 0.88)], (3.0, 3.0), 1.0))
            }
            "mcp_mainextmixbg" => {
                Some(plate(vec![(1.0, 6.0, sunk, 1.0)], (9.0, 8.0), 1.0))
            }
            "mcp_mainextmixbgsel" => Some(plate(
                vec![(1.0, 1.0, sunk, 1.0), (2.0, 5.0, g(0.005), 1.0)],
                (9.0, 8.0),
                1.0,
            )),
            "mcp_main_namebg" => Some(plate(
                vec![
                    (0.0, 1.0, sunk, 1.0),
                    (1.0, 2.0, deep, 1.0),
                    (3.0, 4.0, sunk, 1.0),
                    (7.0, 1.0, deep, 1.0),
                    (8.0, 1.0, sunk, 1.0),
                ],
                (8.0, 9.0),
                0.0,
            )),
            "mcp_main_namebg_sel" => Some(plate(
                vec![
                    (0.0, 1.0, sunk, 1.0),
                    (1.0, 2.0, g(0.93), 1.0), // #eeeeee
                    (3.0, 1.0, deep, 1.0),
                    (4.0, 3.0, lit, 1.0),
                    (7.0, 1.0, deep, 1.0),
                    (8.0, 1.0, sunk, 1.0),
                ],
                (8.0, 9.0),
                0.0,
            )),
            "mcp_iconbg" => Some(plate(
                vec![
                    (1.0, 3.0, sunk, 1.0),
                    (4.0, 4.0, g(-0.03), 1.0), // #3d3d3d
                    (8.0, 2.0, sunk, 1.0),
                ],
                (6.0, 11.0),
                1.0,
            )),
            "mcp_iconbgsel" => Some(plate(
                vec![
                    (1.0, 2.0, g(1.0), 1.0), // #ffffff
                    (3.0, 1.0, sunk, 1.0),
                    (4.0, 4.0, g(0.32), 0.416), // #797979 at 106
                    (8.0, 2.0, sunk, 1.0),
                ],
                (6.0, 11.0),
                1.0,
            )),
            "tcp_mainbg" => Some(plate(vec![(0.0, 9.0, sunk, 1.0)], (22.0, 9.0), 0.0)),
            "tcp_mainbgsel" => Some(plate(vec![(0.0, 9.0, lit, 1.0)], (22.0, 9.0), 0.0)),
            "tcp_iconbg" => Some(plate(
                vec![(1.0, 10.0, deep, 1.0), (11.0, 1.0, g(-0.52), 1.0)],
                (11.0, 13.0),
                1.0,
            )),
            "tcp_iconbgsel" => Some(plate(
                vec![(1.0, 10.0, g(-0.06), 1.0), (11.0, 1.0, deep, 1.0)],
                (11.0, 13.0),
                1.0,
            )),
            "envcp_bg" => Some(plate(
                vec![(0.0, 10.0, sunk, 1.0), (10.0, 1.0, g(-0.51), 1.0)],
                (48.0, 12.0),
                0.0,
            )),
            "envcp_bgsel" => Some(plate(
                vec![(0.0, 10.0, g(0.005), 1.0), (10.0, 1.0, g(-0.43), 1.0)],
                (48.0, 12.0),
                0.0,
            )),
            "envcp_namebg" => {
                Some(plate(vec![(1.0, 22.0, g(-0.56), 1.0)], (22.0, 24.0), 1.0))
            }
            // Drawn by REAPER, not by the theme: every pixel is
            // transparent, so an empty band list is the whole truth.
            "mcp_namebg" | "mcp_idxbg" | "mcp_idxbg_sel" | "tcp_namebg"
            | "tcp_main_namebg_sel" | "tcp_idxbg" | "tcp_idxbg_sel" => {
                Some(plate(vec![], (4.0, 4.0), 0.0))
            }
            _ => None,
        };
        if let Some(markup) = hit {
            return Some(markup);
        }
    }
    // The envelope panel's controls. Every one of these has a pressed
    // cell identical to its normal cell — only hover moves — except the
    // two unlit plates, which gain a fill when pressed that they do not
    // have at rest.
    {
        use v::EnvcpGlyph as G;
        let plate = |glyph, lit| {
            render_svg(
                v::EnvcpPlate,
                v::EnvcpPlateProps {
                    glyph,
                    lit,
                    cell: (30.0, 20.0),
                    width: n.0,
                    height: n.1,
                    at,
                },
            )
        };
        let furniture = |part, cell| {
            render_svg(
                v::EnvcpPanel,
                v::EnvcpPanelProps { part, cell, width: n.0, height: n.1, at },
            )
        };
        let hit = match name {
            "envcp_learn" => Some(plate(G::Learn, false)),
            "envcp_learn_on" => Some(plate(G::Learn, true)),
            "envcp_parammod" => Some(plate(G::ParamMod, false)),
            "envcp_parammod_on" => Some(plate(G::ParamMod, true)),
            "envcp_hide" => Some(render_svg(
                v::EnvcpOptionsButton,
                v::EnvcpOptionsProps { cell: (36.0, 20.0), width: n.0, height: n.1, at },
            )),
            "envcp_arm_off" | "envcp_arm_on" => Some(render_svg(
                v::EnvcpArmButton,
                v::EnvcpArmProps {
                    armed: name.ends_with("_on"),
                    cell: (20.0, 20.0),
                    width: n.0,
                    height: n.1,
                    at,
                },
            )),
            "envcp_faderbg" => Some(furniture(v::EnvcpPart::FaderTrack, (19.0, 24.0))),
            "custom_envcp_arm_bg" => Some(furniture(v::EnvcpPart::ArmField, (22.0, 22.0))),
            "envcp_knob_small" => Some(furniture(v::EnvcpPart::Knob, (25.0, 26.0))),
            "envcp_fader" => Some(furniture(v::EnvcpPart::FaderCap, (23.0, 29.0))),
            "envcp_bypass_off" | "envcp_bypass_on" => Some(render_svg(
                v::EnvcpBypassButton,
                v::EnvcpBypassProps {
                    bypassed: name.ends_with("_on"),
                    cell: (15.0, 20.0),
                    width: n.0,
                    height: n.1,
                    at,
                },
            )),
            _ => None,
        };
        if let Some(markup) = hit {
            return Some(markup);
        }
    }
    {
        use v::SliderPart as S;
        let slider = |part, cell| {
            render_svg(
                v::PanelSlider,
                v::SliderProps { part, cell, width: n.0, height: n.1, at },
            )
        };
        let hit = match name {
            "mcp_folder_on" => Some(slider(S::MixerFolder, (55.0, 15.0))),
            // `mcp_folder_last` and `mcp_fcomp_*` are not wired here on
            // purpose. The strip
            // detector reports three cells of 49, and cell 0 contains two
            // separate marks — so either the detection is wrong or the
            // image is not a three-state strip, and drawing it against
            // either reading before knowing which would be a guess
            // dressed as a measurement. `mcp_folder_last` reads the same
            // way: its wedge sits in one cell of three and the other two
            // are not empty, which no single drawing accounts for.
            "tcp_volbg" => Some(slider(S::VolumeTrough, (19.0, 24.0))),
            "tcp_volthumb" => Some(slider(S::VolumeThumb, (27.0, 29.0))),
            "tcp_panbg" | "tcp_widthbg" => Some(slider(S::PanTrough, (43.0, 11.0))),
            "mcp_panbg" | "mcp_widthbg" => Some(slider(S::PanTrough, (69.0, 11.0))),
            "tcp_panthumb" | "tcp_widththumb" | "mcp_panthumb" | "mcp_widththumb" => {
                Some(slider(S::PanThumb, (13.0, 23.0)))
            }
            _ => None,
        };
        if let Some(markup) = hit {
            return Some(markup);
        }
    }
    {
        use v::TransportPart as P;
        let part = |part, cell| {
            render_svg(
                v::TransportPanel,
                v::TransportPartProps { part, cell, width: n.0, height: n.1, at },
            )
        };
        let hit = match name {
            "transport_timebase_beat" => Some(part(P::TimebaseBeat, (33.0, 22.0))),
            "transport_timebase_time" => Some(part(P::TimebaseTime, (33.0, 22.0))),
            "transport_bg" => Some(part(P::Panel, (200.0, 67.0))),
            "transport_status_bg" => Some(part(P::Status, (32.0, 28.0))),
            "transport_status_bg_err" => Some(part(P::StatusError, (6.0, 10.0))),
            "transport_bpm" => Some(part(P::Bpm, (92.0, 26.0))),
            "transport_playspeedbg" => Some(part(P::SpeedTrack, (6.0, 21.0))),
            "transport_playspeedthumb" => Some(part(P::SpeedThumb, (22.0, 28.0))),
            "transport_knob_bg_large" => Some(part(P::KnobRing, (32.0, 34.0))),
            // Wholly transparent in the source, all three of them.
            "transport_bpm_bg" => Some(part(P::Empty, (6.0, 10.0))),
            "transport_edit_bg" => Some(part(P::Empty, (11.0, 11.0))),
            "transport_group_bg" => Some(part(P::Empty, (11.0, 11.0))),
            _ => None,
        };
        if let Some(markup) = hit {
            return Some(markup);
        }
    }

    // Both families answer to the same eight controls, so match on the
    // part after the prefix rather than writing every name twice.
    let stem = name
        .strip_prefix("mcp_")
        .or_else(|| name.strip_prefix("track_"))
        .or_else(|| name.strip_prefix("tcp_"))?;

    Some(match stem {
        "fxempty_h" => byp(v::FxBypass::Empty),
        "fxon_h" => byp(v::FxBypass::On),
        "fxoff_h" => byp(v::FxBypass::Off),
        "fxempty_v" => byp(v::FxBypass::Empty),
        "fxon_v" => byp(v::FxBypass::On),
        "fxoff_v" => byp(v::FxBypass::Off),

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

        // Track-panel only. The mixer's envelope button is a different
        // control entirely — no plate, and a five-pixel word under the
        // glyph — and type that small is hinted by hand, not described.
        // It stays traced for the same reason the large knob's blurred
        // edge does: a vector version would have to draw the artefact.
        "env" | "env_read" | "env_write" | "env_touch" | "env_latch"
        | "env_preview"
            if !track =>
        {
            return None;
        }

        "env" => env(v::EnvelopeMode::Off),
        "env_read" => env(v::EnvelopeMode::Read),
        "env_write" => env(v::EnvelopeMode::Write),
        "env_touch" => env(v::EnvelopeMode::Touch),
        "env_latch" => env(v::EnvelopeMode::Latch),
        "env_preview" => env(v::EnvelopeMode::Preview),

        "recmode_off" if track => recmode(v::RecordMode::Off),
        "recmode_in" if track => recmode(v::RecordMode::Input),
        "recmode_out" if track => recmode(v::RecordMode::Output),

        "fcomp_off" if track => fcomp(v::FolderCompact::Off),
        "fcomp_small" if track => fcomp(v::FolderCompact::Small),
        "fcomp_tiny" if track => fcomp(v::FolderCompact::Tiny),

        "fx_in_norm" if track => fx_in(true),
        "fx_in_empty" if track => fx_in(false),

        "folder_off" if track => folder(v::FolderState::Off),
        "folder_on" if track => folder(v::FolderState::On),
        "folder_last" if track => folder(v::FolderState::Last),

        "phase_norm" => phase(false),
        "phase_inv" => phase(true),

        "monitor_off" => mon(v::Monitoring::Off),
        "monitor_on" => mon(v::Monitoring::On),
        "monitor_auto" => mon(v::Monitoring::Auto),

        // Mixer-only: the knobs and the fader live in one panel.
        //
        // Pan and width share art — the two source PNGs are byte-identical,
        // because REAPER labels the control and the knob itself is generic.
        "pan_knob_small" | "width_knob_small" => render_svg(
            v::PanningKnob,
            v::PanProps {
                position: 0.0,
                large: false,
                width: n.0,
                height: n.1,
            },
        ),
        "pan_knob_large" | "width_knob_large" => render_svg(
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
        // `art_x` is the first *drawn* column, which is the cell origin
        // for every control whose art starts at its own left edge. The
        // mixer's FX toggle does not: it leaves one empty column as the
        // seam between the pill's halves, so the first ink is a pixel in
        // and the fallback put every cell a pixel right.
        let origin = spec.art_x.saturating_sub(leading_gap(name));
        spec.cells =
            crate::derive::even_cells(spec.width, states(name) as u32, origin);
    }

    composite_cells(&spec, |i, _w| {
        cell_markup(name, interaction(i))
            .ok_or_else(|| RenderError::Svg(format!("no vector control draws {name}")))
    })
}

/// Empty columns before a control's art, which `art_x` cannot see past.
///
/// Only the mixer's FX toggle has any — the seam its half of the pill
/// leaves for the label's half to meet. The track panel's toggle abuts
/// directly and starts at its own edge, which is why this is per name
/// rather than per family.
fn leading_gap(name: &str) -> u32 {
    match name {
        "track_fxempty_v" | "track_fxoff_v" | "track_fxon_v" => 1,
        _ => 0,
    }
}

/// How many sprite cells a control's image holds.
///
/// Knowledge, not a measurement: every button here draws normal, hover
/// and pressed, and the knobs and the fader draw one thing.
fn states(name: &str) -> usize {
    if name.starts_with("envcp_") {
        // The envelope panel's *buttons* are three-state strips like
        // everyone else's; its furniture — backgrounds, fader, knob — is
        // one drawing. Blanket-1 was right while only the backgrounds
        // were wired and renders each button into a third of its width
        // the moment one is not.
        return match name {
            "envcp_arm_off" | "envcp_arm_on" | "envcp_bypass_off" | "envcp_bypass_on"
            | "envcp_learn" | "envcp_learn_on" | "envcp_parammod" | "envcp_parammod_on"
            | "envcp_hide" => 3,
            _ => 1,
        };
    }
    let stem = name
        .strip_prefix("mcp_")
        .or_else(|| name.strip_prefix("track_"))
        .or_else(|| name.strip_prefix("tcp_"))
        .unwrap_or(name);
    match stem {
        "pan_knob_small" | "pan_knob_large" | "width_knob_small" | "width_knob_large"
        | "volthumb" | "volbg" => 1,
        // Three marks side by side in one image, not three pointer states.
        "folder_off" | "folder_on" | "folder_last" => 1,
        // Three states stacked vertically: one drawing, not three cells.
        "fxlist_norm" | "fxlist_byp" | "fxlist_off" | "fxlist_empty"
        | "sendlist_norm" | "sendlist_mute" | "sendlist_empty"
        | "sendlist_midihw" => 1,
        // Named for the panel it customises, not with the panel's prefix,
        // so the `envcp_` shortcut above never sees it.
        "custom_envcp_arm_bg" => 1,
        "mainbg" | "mainbgsel" | "bg" | "bgsel" | "extmixbg" | "extmixbgsel"
        | "mainextmixbg" | "mainextmixbgsel" | "namebg" | "main_namebg"
        | "main_namebg_sel" | "iconbg" | "iconbgsel" | "idxbg" | "idxbg_sel" => 1,
        // The transport's furniture is drawn once and stretched. Left at
        // the default of three, each panel was rendered into a third of
        // its own width and repeated across it — which reads, in the
        // audit, as the drawing being twenty columns too far left.
        "transport_bg" | "transport_bpm" | "transport_bpm_bg"
        | "transport_edit_bg" | "transport_group_bg" | "transport_status_bg"
        | "transport_status_bg_err" | "transport_playspeedbg"
        | "transport_playspeedthumb" | "transport_knob_bg_large" => 1,
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
            art_x: 0,
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
        // A name no component will ever draw, rather than one that
        // happens not to be drawn yet: this test named `mcp_bg` and broke
        // the day `mcp_bg` gained a component, which is a test failing at
        // progress rather than at a bug.
        assert!(cell_markup("mcp_not_a_real_image", v::Interaction::Normal).is_none());
        let spec = DerivedSpec {
            art_x: 0,
            width: 4,
            height: 4,
            markers: vec![],
            cells: vec![(0, 4)],
        };
        assert!(render_control("mcp_not_a_real_image", &spec).is_err());
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

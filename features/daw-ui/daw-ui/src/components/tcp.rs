//! The track control panel — REAPER's TCP, as Dioxus.
//!
//! The mixer strip's sibling, and deliberately built the same way: the
//! numbers are REAPER's, taken from `daw-theme-art`'s panel sheet, which
//! composes the same components at the same coordinates for the exporter.
//! Two renderings of one layout is the point — if they drift, the sheet is
//! the thing that is wrong.
//!
//! # It is a row, not a column
//!
//! Everything the mixer strip learned still applies, but rotated. The
//! track colour tints the *whole* row rather than a band inside it; the
//! controls sit on two rows of 17-tall fields; and mute and solo live
//! outside the tint in their own 44-wide gutter, which is why the row is
//! 340 wide and the tint only 296.
//!
//! # Layout-critical values are inline
//!
//! Same rule, same reason: every window in this tree embeds its stylesheet
//! as a static string, and a panel that mounts before the sheet arrives —
//! or a test that mounts without one — must still lay out. Tailwind is the
//! additive layer. The mixer strip spent an afternoon learning this the
//! hard way; see `components::mixer`.

use daw_proto::Track;
use daw_theme_art::dress::Panel;
use daw_theme_art::vector_controls as art;

use crate::controls::{
    EnvelopeButton, FxButton, IoButton, MuteButton, PanKnob, PhaseButton, RecordArmButton,
    SoloButton, TrackMeter, use_track_store,
};
use crate::prelude::*;

// The row's geometry — measured off REAPER itself rather than off the
// panel sheet — lives in `daw_theme_art::geometry::tcp`, one home for the
// panel's facts. The measurement notes travel with the numbers.
use daw_theme_art::geometry::tcp::{
    COLUMN_RULE_X, FIELD_H, FX_IN_X, GUTTER_BUTTON_X, GUTTER_W, LANES_FROM_FLOOR, LANES_HIDE_H,
    METER_W as TCP_METER_W, METER_X as TCP_METER_X, NAME_FIELD_H, NAME_FIELD_W, NAME_FIELD_X,
    PAN_KNOB_X, PHASE_FROM_FLOOR, PHASE_HIDE_H, ROUTING_X, ROW_H, ROW_ONE, ROW_TWO, ROW_W,
    SOLO_TOP, TINT_W, VOLUME_KNOB_X,
};

/// The panel's own grey — the same one the mixer strip's body and name
/// plate use.
const BODY_GREY: &str = "#262626";

/// One track's row in the panel.
///
/// Public so a test, a playground or a design review can photograph a row
/// without a backend behind it — the same reason `ChannelStripPreview`
/// exists on the mixer side.
#[component]
pub fn TrackRow(
    track: Track,
    #[props(default)] index: u32,
    /// The row's height. REAPER's track panel grows with the track, and
    /// two of its controls only exist above a height — phase and the
    /// fixed-lanes button — so a row that could not grow could not show
    /// them at all.
    #[props(default = ROW_H)]
    height: f32,
) -> Element {
    // The store first, the prop as the seed — a rename or a recolour in
    // REAPER reaches the row on the track stream rather than on a poll.
    // The mixer strip read its colour off the prop and sat a poll behind
    // every button beside it.
    let row_h = height;
    let store = use_track_store();
    let guid = track.guid.clone();
    let live = use_memo(use_reactive!(|guid| store.track(&guid)));
    let live = live.read();
    let track = live.as_ref().unwrap_or(&track);

    let theme = daw_theme::Theme::default();
    let tint = track
        .color
        .map(|c| daw_theme_art::dress::panel_tint(daw_theme::Color::rgb(
            (c >> 16) as u8,
            (c >> 8) as u8,
            c as u8,
        )))
        .unwrap_or(theme.chrome.surface_raised);
    // The fields are the track colour, darkened — not a flat grey. Measured
    // against the tint in REAPER's own render: the input combo is 0.75 of
    // it (#752D3F against #9D3C55) and the FX slot 0.64 (#652637). The
    // effect is of a translucent panel over the colour, which is what makes
    // a REAPER row read as one surface rather than grey boxes on a tint.
    let combo = tint.shade(-0.25).css();
    let slot = tint.shade(-0.36).css();
    // The name field is the exception: REAPER paints it the panel's own
    // #262626, flat, whatever the track's colour.
    let field = BODY_GREY;
    let gutter = theme.chrome.hardware.shade(-0.40).css();
    let rule = theme.chrome.hardware.shade(-0.72).css();
    let index_ink = theme.chrome.hardware_mark.shade(0.1).css();
    let name_ink = theme.chrome.hardware_mark.shade(0.62).css();
    let combo_ink = theme.chrome.hardware_mark.shade(0.5).css();
    let caret = theme.chrome.hardware_mark.shade(0.2).css();
    let fx_ink = theme.chrome.hardware_mark.shade(-0.1).css();

    // Where the lanes button lands: above phase in the corner when the row
    // has room for both, tucked under solo when it only has room for one.
    let lanes_top = if row_h >= PHASE_HIDE_H {
        row_h - LANES_FROM_FLOOR
    } else {
        SOLO_TOP + 20.0 + 2.0
    };

    rsx! {
        div {
            class: "relative shrink-0",
            style: "position:relative; width:{ROW_W}px; height:{row_h + 1.0}px; \
                    border-bottom:1px solid {rule};",

            // The track colour is the row: REAPER tints the whole panel,
            // not a stripe on it. It stops at the gutter.
            div {
                style: "position:absolute; left:0; top:0; \
                        width:{TINT_W}px; height:{row_h}px; background:{tint.css()};",
            }
            div {
                style: "position:absolute; left:{TINT_W}px; top:0; \
                        width:{GUTTER_W}px; height:{row_h}px; background:{gutter};",
            }

            // ── The left column ──
            //
            // Measured: a #323232 edge at 0..2, then the tint, then a
            // one-pixel rule at 20 dividing the column from the row. The
            // rule and both glyphs are the tint at 0.75 — the same
            // darkening the input combo gets — so they read as pressed
            // into the panel rather than printed on it.
            div {
                style: "position:absolute; left:0; top:0; width:3px; height:{row_h}px; \
                        background:#323232;",
            }
            div {
                style: "position:absolute; left:{COLUMN_RULE_X}px; top:0; \
                        width:1px; height:{row_h}px; background:{combo};",
            }
            div { style: "position:absolute; left:8px; top:6px;",
                art::TrackPin { colour: combo.clone() }
            }
            div { style: "position:absolute; left:7px; top:{row_h - 10.0}px;",
                art::TrackFolder { colour: combo.clone() }
            }

            // The row's right edge, which REAPER closes with a dark column
            // at 341 — the boundary between the panel and the arrange
            // view, and the last thing missing from the row's silhouette.
            div {
                style: "position:absolute; left:{ROW_W - 2.0}px; top:0; \
                        width:1px; height:{row_h}px; background:{rule};",
            }

            // The track number, between them.
            div {
                class: "absolute font-mono",
                style: "position:absolute; left:9px; top:{row_h / 2.0 - 8.0}px; \
                        font-size:11px; line-height:16px; color:{index_ink};",
                "{index + 1}"
            }

            // ── Row one ──
            //
            // No separate monitor indicator: REAPER shows monitoring *on*
            // the record arm in the track panel — `track_recarm_*` has the
            // variants — rather than as a control of its own, and drawing
            // both put a glyph in the row REAPER does not have.
            //
            // The name field is one long box and the record arm and the
            // volume knob sit *on* it, at its two ends — REAPER's field
            // runs from behind the arm at 26 to behind the knob at 230,
            // and the name is set between them. Drawn as three boxes in a
            // line instead, the arm and the knob had grey gutters either
            // side of them that REAPER does not have.
            div {
                style: "position:absolute; left:{NAME_FIELD_X}px; top:{ROW_ONE}px; \
                        width:{NAME_FIELD_W}px; height:{NAME_FIELD_H}px; \
                        background:{field}; \
                        border-radius:{NAME_FIELD_H / 2.0}px 0 0 {NAME_FIELD_H / 2.0}px;",
            }
            div { style: "position:absolute; left:{NAME_FIELD_X + 2.0}px; top:7px;",
                RecordArmButton { track: track.guid.clone(), panel: Panel::Track }
            }
            div {
                style: "position:absolute; left:58px; top:{ROW_ONE}px; \
                        width:{NAME_FIELD_X + NAME_FIELD_W - 58.0}px; \
                        height:{NAME_FIELD_H}px; \
                        line-height:{NAME_FIELD_H}px; font-size:11.5px; \
                        color:{name_ink}; white-space:nowrap; overflow:hidden; \
                        text-overflow:ellipsis; \
                        font-family:Fira Sans, DejaVu Sans, sans-serif;",
                "{track.name}"
            }
            // The volume knob, on the field's right end.
            div { style: "position:absolute; left:{VOLUME_KNOB_X}px; top:{ROW_ONE}px;",
                VolumeKnob { track: track.guid.clone() }
            }
            // Pan sits *outside* the field, not on it — at 186 it landed
            // under the volume knob, because that was the sheet's number for
            // a row whose field stopped at 180.
            div { style: "position:absolute; left:{PAN_KNOB_X}px; top:5px;",
                PanKnob { track: track.guid.clone() }
            }
            // The routing widget, and then the FX pill — both measured,
            // both drawn by the components the mixer already uses. The
            // routing button's lanes lie in a row here rather than stacked,
            // which is the only difference between REAPER's two images, and
            // the pill is the track panel's 36-wide family rather than the
            // mixer's 46.
            div { style: "position:absolute; left:{ROUTING_X}px; top:6px;",
                IoButton { track: track.guid.clone(), panel: Panel::Track }
            }
            div { style: "position:absolute; left:{FX_IN_X}px; top:6px;",
                FxButton { track: track.guid.clone(), panel: Panel::Track }
            }

            // ── Row two: envelope, the FX slot, the input combo ──
            // 29, measured: the envelope's block runs 29..48 of the row.
            div { style: "position:absolute; left:29px; top:{ROW_TWO}px;",
                EnvelopeButton { width: 22, height: 20, panel: Panel::Track }
            }
            div {
                style: "position:absolute; left:56px; top:{ROW_TWO}px; \
                        width:34px; height:{FIELD_H}px; background:{slot}; \
                        line-height:{FIELD_H}px; text-align:center; font-size:10px; \
                        color:{fx_ink}; font-family:Fira Sans, DejaVu Sans, sans-serif;",
                "FX"
            }
            div {
                // 91 and 195: REAPER's slot and combo touch — scanned
                // across the band there is no tint between them, where we
                // left a four-column gap — and the combo runs to 285.
                style: "position:absolute; left:91px; top:{ROW_TWO}px; \
                        width:195px; height:{FIELD_H}px; background:{combo};",
                div {
                    // Centred, as REAPER sets it — left-aligned it read as
                    // a text field rather than the combo it is.
                    style: "line-height:{FIELD_H}px; font-size:11px; text-align:center; \
                            color:{combo_ink}; white-space:nowrap; overflow:hidden; \
                            font-family:Fira Sans, DejaVu Sans, sans-serif;",
                    "{input_label(track)}"
                }
                // The combo's caret. A triangle rather than a glyph, so it
                // does not depend on a font having one.
                svg {
                    style: "position:absolute; left:181px; top:8px;",
                    width: "7", height: "4", view_box: "0 0 7 4",
                    xmlns: "http://www.w3.org/2000/svg",
                    path { d: "M 0 0 h 7 l -3.5 4 z", fill: "{caret}" }
                }
            }

            // ── The meter section: the meter, then mute over solo ──
            //
            // `meterRight` in the theme moves the whole section to the end
            // of the row, and within it the meter comes *first* — a
            // vertical strip against the tint, with mute and solo to its
            // right. The panel sheet draws the meter inside the tint
            // instead, which put it in the middle of the row.
            div {
                style: "position:absolute; left:{TINT_W + GUTTER_BUTTON_X}px; top:3px;",
                MuteButton { track: track.guid.clone(), panel: Panel::Track }
            }
            div {
                style: "position:absolute; left:{TINT_W + GUTTER_BUTTON_X}px; top:{SOLO_TOP}px;",
                SoloButton { track: track.guid.clone(), panel: Panel::Track }
            }
            // Phase in the corner, the lanes button above it — or, when
            // the row is too short for phase but not for lanes, the lanes
            // button tucked under solo instead.
            if row_h >= PHASE_HIDE_H {
                div {
                    style: "position:absolute; \
                            left:{TINT_W + GUTTER_BUTTON_X + 3.0}px; \
                            top:{row_h - PHASE_FROM_FLOOR}px;",
                    PhaseButton { track: track.guid.clone() }
                }
            }
            if row_h >= LANES_HIDE_H {
                div {
                    style: "position:absolute; \
                            left:{TINT_W + GUTTER_BUTTON_X + 1.0}px; top:{lanes_top}px;",
                    FixedLanes { on: false }
                }
            }

            // No scale: there is no column for numbers out here, and REAPER
            // prints them only in the mixer.
            div {
                style: "position:absolute; left:{TINT_W + TCP_METER_X}px; top:2px;",
                TrackMeter {
                    track: track.guid.clone(),
                    width: TCP_METER_W,
                    height: (row_h - 4.0) as u32,
                    scale: false,
                    // The section's own colour: REAPER's meter here shows
                    // nothing at rest, and a trough put two black bars in a
                    // strip that is meant to read as empty.
                    well: gutter.clone(),
                }
            }
        }
    }
}

/// What the input combo reads.
///
/// The row shows the track's record input, which is the same fact the
/// mixer's `RecordInputLabel` shows — spelled out here rather than
/// abbreviated, because the field is 186 wide instead of 86.
fn input_label(track: &Track) -> String {
    use daw_proto::track::RecordInput;
    match track.record_input {
        RecordInput::None => "No input".to_string(),
        RecordInput::Audio { channel } => format!("Input {}", channel + 1),
        RecordInput::Midi { device_id, channel } => match (device_id, channel) {
            (Some(d), Some(c)) => format!("MIDI {d} ch {}", c + 1),
            (Some(d), None) => format!("MIDI {d}"),
            (None, Some(c)) => format!("MIDI all ch {}", c + 1),
            (None, None) => "MIDI".to_string(),
        },
        RecordInput::Raw(v) => format!("Input #{v}"),
    }
}

/// The volume knob, on the name field's right end.
///
/// The art is `vector_controls::VolumeKnob`; this is the wrapper that binds
/// it to the track. The ring swings on the *taper*, not the gain, for the
/// same reason the fader's cap does — unity is a little under
/// three-quarters round, not hard against the stop.
#[component]
fn VolumeKnob(track: String) -> Element {
    let mut at = use_signal(art::Interaction::default);
    let store = use_track_store();
    let guid = track.clone();
    let gain = use_memo(use_reactive!(|guid| store.volume(&guid)));

    rsx! {
        div {
            style: "display:inline-block; line-height:0; cursor:ns-resize;",
            onmouseenter: move |_| at.set(art::Interaction::Hover),
            onmouseleave: move |_| at.set(art::Interaction::Normal),
            art::VolumeKnob {
                value: crate::controls::fader_position(gain()) as f32,
                width: Some(24),
                height: Some(24),
                at: at(),
            }
        }
    }
}

/// The fixed-lanes button, bound to nothing yet.
///
/// `trackfixedlanes` is not something the DAW facade reports, so this
/// draws the off state rather than inventing one — the same stance the
/// envelope button takes.
#[component]
fn FixedLanes(on: bool) -> Element {
    let mut at = use_signal(art::Interaction::default);
    rsx! {
        div {
            style: "display:inline-block; line-height:0; cursor:pointer;",
            onmouseenter: move |_| at.set(art::Interaction::Hover),
            onmouseleave: move |_| at.set(art::Interaction::Normal),
            art::FixedLanesButton { on, width: Some(20), height: Some(24), at: at() }
        }
    }
}

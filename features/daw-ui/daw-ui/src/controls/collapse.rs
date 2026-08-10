//! How the strip collapses as it gets shorter.
//!
//! A short mixer has to stay usable, not become a crushed copy of a tall
//! one, so REAPER drops parts of the strip at fixed heights. Those heights
//! are the target — the map's standing rule is that REAPER is the source of
//! truth while mapping — and they are already written down in the theme, so
//! this is data entry rather than design.
//!
//! # It is not one breakpoint ladder
//!
//! Five different kinds of thing, and only the first is a plain height
//! comparison:
//!
//! 1. **Container height.** The input-FX row, the record input, the pan
//!    labels, the pan section and the fader's dB readout each disappear at
//!    their own height.
//! 2. **A derived residual.** What is left after the fixed bands — the
//!    stretch section. The IO, envelope and phase buttons drop at their own
//!    values *of that*, and padding steps down in three stages.
//! 3. **A gap between two resolved siblings.** Not modelled here yet; see
//!    the note on [`Collapse::fader`].
//! 4. **A widget-type switch.** Below a threshold the fader stops being a
//!    fader and becomes a knob — in Dioxus a Rust conditional, not CSS.
//! 5. **A re-anchor.** Below the pan-section threshold the pan *section* is
//!    gone but the pan *control* is not: it re-parents into the input area.
//!    Genuine re-anchoring, not repositioning.
//!
//! # Why this is Rust and not a container query
//!
//! A container query can only ask about a container, so (2) and (3) are
//! inexpressible in CSS until the stretch section and the gaps are real
//! boxes. They are real boxes in the tree — that decomposition is most of
//! what this module is for — but the *decision* still has to read a derived
//! number, and a derived number is a Rust expression. Being Rust also makes
//! it testable by sweeping a height across a boundary, which is the only way
//! to prove a threshold fires where REAPER's does.

/// Every height at which the strip changes shape, in REAPER's own pixels.
///
/// One constant, read by the panel — and spliced into the theme's layout
/// file by #148, so the two sides cannot drift.
///
/// Taken from `rtconfig.txt`: the five container thresholds are the
/// `hide_*` parameters, the residual ones are `mcp_io_hide_h`,
/// `mcp_env_hide_h` and `mcp_phase_hide_h`, and the padding stages are
/// `padding_reduction_h`. The env and phase thresholds are pairs because
/// the theme carries one value per labels mode; the first is the mode this
/// strip draws.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Thresholds {
    /// Below this, the input-FX row goes.
    pub input_fx: f32,
    /// Below this, the record-input row goes.
    pub record_input: f32,
    /// Below this, the pan labels go.
    pub pan_labels: f32,
    /// Below this, the pan *section* goes — the control re-anchors.
    pub pan_section: f32,
    /// Below this, the fader's dB readout goes.
    pub volume_label: f32,
    /// Stretch-section height below which the IO button goes.
    pub io: f32,
    /// Stretch-section height below which the envelope button goes.
    pub envelope: f32,
    /// Stretch-section height below which the phase button goes.
    pub phase: f32,
    /// Stretch-section heights at which padding steps down: below the
    /// first it is 3px, below the second 2px, otherwise 4px.
    pub padding_steps: (f32, f32),
    /// Below this, the fader becomes a knob.
    pub fader_swap: f32,
}

/// REAPER's values, at scale 1.
pub const REAPER: Thresholds = Thresholds {
    input_fx: 400.0,
    record_input: 350.0,
    pan_labels: 320.0,
    pan_section: 260.0,
    volume_label: 250.0,
    io: 106.0,
    envelope: 125.0,
    phase: 144.0,
    padding_steps: (350.0, 250.0),
    fader_swap: 280.0,
};

/// The fixed bands, in REAPER's pixels, that the stretch section is what is
/// left over from. From `fx_sec`, `pan_sec`, `in_sec` and `bot_sec`.
const FX_SECTION: f32 = 33.0;
const PAN_SECTION_FULL: f32 = 50.0;
const PAN_SECTION_UNLABELLED: f32 = 33.0;
const PAN_SECTION_COLLAPSED: f32 = 6.0;
const INPUT_SECTION_FULL: f32 = 54.0;
const INPUT_SECTION_NO_FX: f32 = 42.0;
const INPUT_SECTION_MINIMAL: f32 = 22.0;
const BOTTOM_SECTION: f32 = 47.0;

/// Where the pan control lives at a given height.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanAnchor {
    /// Its own section, under the FX row.
    PanSection,
    /// Re-parented into the input area, because the pan section is gone.
    /// Record mode gives up its place to it.
    InputArea,
}

/// What the volume control is drawn as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VolumeWidget {
    /// A fader and its rail.
    Fader,
    /// A knob — below the swap threshold there is no room for travel.
    Knob,
}

/// The shape of the strip at one height.
///
/// Everything the strip renders differently is decided here, once, so the
/// markup asks a question rather than doing arithmetic in six places.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Collapse {
    pub show_input_fx: bool,
    pub show_record_input: bool,
    pub show_pan_labels: bool,
    pub show_volume_label: bool,
    pub show_io: bool,
    pub show_envelope: bool,
    pub show_phase: bool,
    pub show_record_mode: bool,
    pub pan: PanAnchor,
    pub volume: VolumeWidget,
    /// Vertical padding between the stacked controls, in px.
    pub padding: f32,
    /// The height left for the fader and meter after the fixed bands.
    pub stretch: f32,
}

impl Collapse {
    /// Resolve the strip's shape at `height`, against REAPER's thresholds.
    pub fn at(height: f32) -> Self {
        Self::with(height, REAPER)
    }

    /// The same, against an arbitrary set — for tests that sweep a boundary
    /// and for a scale other than 1.
    pub fn with(height: f32, t: Thresholds) -> Self {
        let show_pan_labels = height >= t.pan_labels;
        let show_pan_section = height >= t.pan_section;
        let show_record_input = height >= t.record_input;
        let show_input_fx = height >= t.input_fx;

        // The stretch section is a *residual*: what the height has left
        // after the bands above and below it. Which is why the collapses
        // that key off it cannot be written as a query on the strip.
        let pan_band = if !show_pan_section {
            PAN_SECTION_COLLAPSED
        } else if show_pan_labels {
            PAN_SECTION_FULL
        } else {
            PAN_SECTION_UNLABELLED
        };
        let input_band = if !show_record_input {
            INPUT_SECTION_MINIMAL
        } else if !show_input_fx {
            INPUT_SECTION_NO_FX
        } else {
            INPUT_SECTION_FULL
        };
        let stretch = (height - FX_SECTION - pan_band - input_band - BOTTOM_SECTION).max(0.0);

        Self {
            show_input_fx,
            show_record_input,
            show_pan_labels,
            show_volume_label: height >= t.volume_label,
            show_io: stretch >= t.io,
            show_envelope: stretch >= t.envelope,
            show_phase: stretch >= t.phase,
            // The other half of the re-anchor: when pan moves into the
            // input area it takes record mode's place, so record mode goes.
            show_record_mode: show_pan_section,
            pan: if show_pan_section { PanAnchor::PanSection } else { PanAnchor::InputArea },
            volume: if height >= t.fader_swap { VolumeWidget::Fader } else { VolumeWidget::Knob },
            padding: if stretch < t.padding_steps.1 {
                2.0
            } else if stretch < t.padding_steps.0 {
                3.0
            } else {
                4.0
            },
            stretch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sweeping the boundary is the only honest test of a threshold: one
    /// sample on each side, and the claim is about where it *changes*.
    fn sweep(f: impl Fn(Collapse) -> bool, at: f32) {
        assert!(!f(Collapse::at(at - 1.0)), "still on below {at}");
        assert!(f(Collapse::at(at)), "not on at {at}");
    }

    #[test]
    fn the_container_thresholds_are_reapers() {
        sweep(|c| c.show_input_fx, REAPER.input_fx);
        sweep(|c| c.show_record_input, REAPER.record_input);
        sweep(|c| c.show_pan_labels, REAPER.pan_labels);
        sweep(|c| c.show_volume_label, REAPER.volume_label);
        sweep(|c| c.pan == PanAnchor::PanSection, REAPER.pan_section);
    }

    /// The fader becomes a knob rather than becoming a very short fader.
    #[test]
    fn the_volume_widget_switches_type() {
        assert_eq!(Collapse::at(REAPER.fader_swap).volume, VolumeWidget::Fader);
        assert_eq!(Collapse::at(REAPER.fader_swap - 1.0).volume, VolumeWidget::Knob);
    }

    /// The re-anchor: the pan section goes, the pan control does not, and
    /// record mode gives up its place to it.
    #[test]
    fn pan_re_anchors_into_the_input_area() {
        let tall = Collapse::at(REAPER.pan_section);
        assert_eq!(tall.pan, PanAnchor::PanSection);
        assert!(tall.show_record_mode);

        let short = Collapse::at(REAPER.pan_section - 1.0);
        assert_eq!(short.pan, PanAnchor::InputArea, "the pan control vanished with its section");
        assert!(!short.show_record_mode, "record mode kept a place it had given away");
    }

    /// The residual-driven collapses key off the stretch section, not the
    /// strip: at the same strip height a different set of bands above it
    /// gives a different answer, which is the whole reason they are
    /// separate.
    #[test]
    fn the_residual_collapses_key_off_the_stretch_section() {
        // Tall enough that the stretch section is generous.
        let tall = Collapse::at(600.0);
        assert!(tall.show_io && tall.show_envelope && tall.show_phase);

        // Walk down until the stretch section crosses each threshold and
        // check the button goes with it rather than with the strip.
        for h in (150..=600).rev() {
            let c = Collapse::at(h as f32);
            assert_eq!(c.show_io, c.stretch >= REAPER.io, "io disagreed at h={h}");
            assert_eq!(
                c.show_envelope,
                c.stretch >= REAPER.envelope,
                "envelope disagreed at h={h}"
            );
            assert_eq!(c.show_phase, c.stretch >= REAPER.phase, "phase disagreed at h={h}");
        }
    }

    #[test]
    fn padding_steps_down_in_three_stages() {
        let stages: std::collections::BTreeSet<i32> = (120..=800)
            .map(|h| Collapse::at(h as f32).padding as i32)
            .collect();
        assert_eq!(
            stages,
            [2, 3, 4].into_iter().collect(),
            "padding does not step through its three stages"
        );
    }

    /// A strip cannot have a negative stretch section, however short it is.
    #[test]
    fn a_very_short_strip_still_resolves() {
        let c = Collapse::at(40.0);
        assert!(c.stretch >= 0.0);
        assert_eq!(c.volume, VolumeWidget::Knob);
        assert_eq!(c.pan, PanAnchor::InputArea);
    }
}

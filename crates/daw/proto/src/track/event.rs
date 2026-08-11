//! Track events for reactive subscriptions

use super::Track;
use facet::Facet;

/// Events emitted when track state changes
// Wire/domain type: `Added` carries a whole Track by design; boxing the
// event payload would ripple through every subscriber match arm.
#[allow(clippy::large_enum_variant)]
#[repr(u8)]
#[derive(Debug, Clone, Facet)]
pub enum TrackEvent {
    /// A track was added
    Added(Track),
    /// A track was removed (GUID)
    Removed(String),
    /// A track was renamed
    Renamed { guid: String, name: String },
    /// Track mute state changed
    MuteChanged { guid: String, muted: bool },
    /// Track solo state changed
    SoloChanged { guid: String, soloed: bool },
    /// Track arm state changed
    ArmChanged { guid: String, armed: bool },
    /// Track selection changed
    SelectionChanged { guid: String, selected: bool },
    /// Track volume changed
    VolumeChanged { guid: String, volume: f64 },
    /// Track pan changed
    PanChanged { guid: String, pan: f64 },
    /// Track color changed
    ColorChanged { guid: String, color: Option<u32> },
    /// Track TCP visibility changed
    TcpVisibilityChanged { guid: String, visible: bool },
    /// Track mixer visibility changed
    MixerVisibilityChanged { guid: String, visible: bool },
    /// Polarity / phase invert toggled
    PhaseInvertedChanged { guid: String, inverted: bool },
    /// Track automation mode changed
    AutomationModeChanged {
        guid: String,
        mode: crate::primitives::AutomationMode,
    },
    /// Record-input monitoring changed
    InputMonitorChanged {
        guid: String,
        monitor: super::InputMonitoringMode,
    },
    /// Track was moved (index changed)
    Moved {
        guid: String,
        old_index: u32,
        new_index: u32,
    },
    /// The track stopped or started sending to its parent / master.
    ///
    /// The IO indicator on every strip shows this, and it used to be a
    /// per-track routing call — so a mixer paid N round trips for it and
    /// then never heard about a change.
    ParentSendChanged { guid: String, enabled: bool },
    /// The track's FX chains gained or lost plugins.
    ///
    /// `Track::fx_count` and `input_fx_count` were seeded by the bulk read
    /// and then never updated by anything: a mixer's FX buttons were
    /// correct when it opened and wrong from the first plugin the user
    /// added. This is the event that was missing.
    ///
    /// Both counts ride together because a backend computing one has
    /// already computed the other, and a strip showing the input indicator
    /// separately from the chain needs them consistent — two events could
    /// leave it briefly showing a track with input FX and no chain when it
    /// has both.
    ///
    /// Deliberately on the *track* stream rather than a new `#[subscribe]`
    /// on `Effects`: a lit button is track state. What should force a
    /// chain-level stream is a surface that renders the chain.
    FxCountChanged {
        guid: String,
        fx_count: u32,
        input_fx_count: u32,
    },
}

// SelfRef compatibility: TrackEvent has no lifetime parameters, so Ref<'a> = Self.
#[allow(unsafe_code)]
unsafe impl vox_types::Reborrow for TrackEvent {
    type Ref<'a> = TrackEvent;
}

/// Streaming envelope — pairs a [`TrackEvent`] with the project it
/// applies to.
#[derive(Debug, Clone, Facet)]
pub struct TrackStreamEvent {
    pub project_guid: String,
    pub event: TrackEvent,
}

// Trivial Reborrow impls for owned types — lets `SelfRef<T>::get()`
// hand subscribers `&T` for ergonomic field access. Safe because these
// types have no borrowed lifetimes.
#[cfg(feature = "vox")]
#[allow(unsafe_code)]
mod reborrow_impls {
    use super::TrackStreamEvent;
    unsafe impl vox_types::Reborrow for TrackStreamEvent {
        type Ref<'a> = TrackStreamEvent;
    }
}

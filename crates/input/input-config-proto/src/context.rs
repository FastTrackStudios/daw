//! Portable binding-context enums, extracted from reaper-input's
//! `input::keybinds` so wasm clients can use the config types without
//! linking the REAPER runtime. reaper-input re-exports these at their
//! original paths.

use facet::Facet;

/// Keybind context — where a keyboard/wheel binding applies.
#[derive(Debug, Clone, Copy, Facet, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub enum KeybindContext {
    /// All contexts (default)
    #[default]
    Global,
    /// Arrange view (main window)
    Main,
    /// MIDI editor window
    Midi,
    /// Inline MIDI editor
    MidiInline,
    /// Media explorer
    MediaExplorer,
}

impl KeybindContext {
    /// Check if this context matches another (Global matches everything)
    pub fn matches(&self, other: &KeybindContext) -> bool {
        *self == KeybindContext::Global || *other == KeybindContext::Global || *self == *other
    }
}

/// Mouse modifier context - where mouse modifiers apply
#[derive(Debug, Clone, Copy, Facet, PartialEq, Eq, Hash, Default)]
#[repr(C)]
pub enum MouseModifierContext {
    /// Media item left edge
    #[default]
    MediaItemLeftEdge,
    /// Media item right edge
    MediaItemRightEdge,
    /// Media item bottom half
    MediaItemBottomHalf,
    /// Media item fade/autocrossfade
    MediaItemFade,
    /// Envelope point
    EnvelopePoint,
    /// Envelope segment
    EnvelopeSegment,
    /// Track control panel
    TrackControlPanel,
    /// Arrange view (empty area)
    ArrangeView,
    /// MIDI note
    MidiNote,
    /// MIDI CC lane
    MidiCCLane,
}

//! Track events for reactive subscriptions

use super::Track;
use facet::Facet;

/// Events emitted when track state changes
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
    /// Track was moved (index changed)
    Moved {
        guid: String,
        old_index: u32,
        new_index: u32,
    },
}

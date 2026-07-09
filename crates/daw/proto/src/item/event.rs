//! Item and Take events for subscriptions

use super::{Item, Take};
use facet::Facet;

/// Events related to item changes
#[repr(C)]
#[derive(Clone, Debug, Facet)]
pub enum ItemEvent {
    /// An item was created
    Created {
        project_guid: String,
        track_guid: String,
        item: Item,
    },
    /// An item was deleted
    Deleted {
        project_guid: String,
        track_guid: String,
        item_guid: String,
    },
    /// An item's position changed
    PositionChanged {
        project_guid: String,
        item_guid: String,
        old_position: f64,
        new_position: f64,
    },
    /// An item's length changed
    LengthChanged {
        project_guid: String,
        item_guid: String,
        old_length: f64,
        new_length: f64,
    },
    /// An item was moved to a different track
    MovedToTrack {
        project_guid: String,
        item_guid: String,
        old_track_guid: String,
        new_track_guid: String,
    },
    /// An item's mute state changed
    MuteChanged {
        project_guid: String,
        item_guid: String,
        muted: bool,
    },
    /// An item's selection state changed
    SelectionChanged {
        project_guid: String,
        item_guid: String,
        selected: bool,
    },
    /// An item's volume changed
    VolumeChanged {
        project_guid: String,
        item_guid: String,
        volume: f64,
    },
    /// An item's active take changed
    ActiveTakeChanged {
        project_guid: String,
        item_guid: String,
        old_take_index: u32,
        new_take_index: u32,
    },
}

/// Events related to take changes
#[repr(C)]
#[derive(Clone, Debug, Facet)]
pub enum TakeEvent {
    /// A take was created
    Created {
        project_guid: String,
        item_guid: String,
        take: Take,
    },
    /// A take was deleted
    Deleted {
        project_guid: String,
        item_guid: String,
        take_guid: String,
    },
    /// A take's name changed
    NameChanged {
        project_guid: String,
        item_guid: String,
        take_guid: String,
        name: String,
    },
    /// A take's pitch changed
    PitchChanged {
        project_guid: String,
        item_guid: String,
        take_guid: String,
        pitch: f64,
    },
    /// A take's play rate changed
    PlayRateChanged {
        project_guid: String,
        item_guid: String,
        take_guid: String,
        play_rate: f64,
    },
    /// A take's volume changed
    VolumeChanged {
        project_guid: String,
        item_guid: String,
        take_guid: String,
        volume: f64,
    },
    /// A take's source changed
    SourceChanged {
        project_guid: String,
        item_guid: String,
        take_guid: String,
        source_path: Option<String>,
    },
}

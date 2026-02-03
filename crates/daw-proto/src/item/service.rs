//! Item and Take service traits

use super::{FadeShape, Item, ItemRef, SourceType, Take, TakeRef};
use crate::primitives::{BeatAttachMode, Duration, PositionInSeconds};
use crate::{ProjectContext, TrackRef};
use roam::service;

/// Service for managing items on tracks
///
/// Items are media containers that hold one or more takes. They have position,
/// length, and various properties like volume, fades, and timing behavior.
#[service]
pub trait ItemService {
    // =========================================================================
    // Queries
    // =========================================================================

    /// Get all items on a track
    async fn get_items(&self, project: ProjectContext, track: TrackRef) -> Vec<Item>;

    /// Get a specific item
    async fn get_item(&self, project: ProjectContext, item: ItemRef) -> Option<Item>;

    /// Get all items in the project
    async fn get_all_items(&self, project: ProjectContext) -> Vec<Item>;

    /// Get all selected items in the project
    async fn get_selected_items(&self, project: ProjectContext) -> Vec<Item>;

    /// Get the number of items on a track
    async fn item_count(&self, project: ProjectContext, track: TrackRef) -> u32;

    // =========================================================================
    // CRUD Operations
    // =========================================================================

    /// Add a new item to a track
    ///
    /// Returns the GUID of the created item, or None if creation failed.
    async fn add_item(
        &self,
        project: ProjectContext,
        track: TrackRef,
        position: PositionInSeconds,
        length: Duration,
    ) -> Option<String>;

    /// Delete an item
    async fn delete_item(&self, project: ProjectContext, item: ItemRef);

    /// Duplicate an item
    ///
    /// Returns the GUID of the new item, or None if duplication failed.
    async fn duplicate_item(&self, project: ProjectContext, item: ItemRef) -> Option<String>;

    // =========================================================================
    // Position & Length
    // =========================================================================

    /// Set the position of an item
    async fn set_position(
        &self,
        project: ProjectContext,
        item: ItemRef,
        position: PositionInSeconds,
    );

    /// Set the length of an item
    async fn set_length(&self, project: ProjectContext, item: ItemRef, length: Duration);

    /// Move an item to a different track
    async fn move_to_track(&self, project: ProjectContext, item: ItemRef, track: TrackRef);

    /// Set the snap offset
    async fn set_snap_offset(&self, project: ProjectContext, item: ItemRef, offset: Duration);

    // =========================================================================
    // State
    // =========================================================================

    /// Set whether an item is muted
    async fn set_muted(&self, project: ProjectContext, item: ItemRef, muted: bool);

    /// Set whether an item is selected
    async fn set_selected(&self, project: ProjectContext, item: ItemRef, selected: bool);

    /// Set whether an item is locked
    async fn set_locked(&self, project: ProjectContext, item: ItemRef, locked: bool);

    /// Select or deselect all items in the project
    async fn select_all_items(&self, project: ProjectContext, selected: bool);

    // =========================================================================
    // Audio Properties
    // =========================================================================

    /// Set the volume of an item (1.0 = 0dB)
    async fn set_volume(&self, project: ProjectContext, item: ItemRef, volume: f64);

    /// Set the fade in properties
    async fn set_fade_in(
        &self,
        project: ProjectContext,
        item: ItemRef,
        length: Duration,
        shape: FadeShape,
    );

    /// Set the fade out properties
    async fn set_fade_out(
        &self,
        project: ProjectContext,
        item: ItemRef,
        length: Duration,
        shape: FadeShape,
    );

    // =========================================================================
    // Timing Behavior
    // =========================================================================

    /// Set whether the source should loop
    async fn set_loop_source(&self, project: ProjectContext, item: ItemRef, loop_source: bool);

    /// Set how the item attaches to the timeline
    async fn set_beat_attach_mode(
        &self,
        project: ProjectContext,
        item: ItemRef,
        mode: BeatAttachMode,
    );

    /// Set whether the item auto-stretches at tempo changes
    async fn set_auto_stretch(&self, project: ProjectContext, item: ItemRef, auto_stretch: bool);

    // =========================================================================
    // Visual Properties
    // =========================================================================

    /// Set the custom color (None to use default)
    async fn set_color(&self, project: ProjectContext, item: ItemRef, color: Option<u32>);

    /// Set the group ID (None to remove from group)
    async fn set_group_id(&self, project: ProjectContext, item: ItemRef, group_id: Option<u32>);
}

/// Service for managing takes within items
///
/// Takes are alternative recordings or sources within an item. Each item
/// can have multiple takes, but only one is active at a time.
#[service]
pub trait TakeService {
    // =========================================================================
    // Queries
    // =========================================================================

    /// Get all takes in an item
    async fn get_takes(&self, project: ProjectContext, item: ItemRef) -> Vec<Take>;

    /// Get a specific take
    async fn get_take(&self, project: ProjectContext, item: ItemRef, take: TakeRef)
    -> Option<Take>;

    /// Get the active take
    async fn get_active_take(&self, project: ProjectContext, item: ItemRef) -> Option<Take>;

    /// Get the number of takes in an item
    async fn take_count(&self, project: ProjectContext, item: ItemRef) -> u32;

    // =========================================================================
    // CRUD Operations
    // =========================================================================

    /// Add a new empty take to an item
    ///
    /// Returns the GUID of the created take, or None if creation failed.
    async fn add_take(&self, project: ProjectContext, item: ItemRef) -> Option<String>;

    /// Delete a take
    async fn delete_take(&self, project: ProjectContext, item: ItemRef, take: TakeRef);

    /// Set which take is active
    async fn set_active_take(&self, project: ProjectContext, item: ItemRef, take: TakeRef);

    // =========================================================================
    // Metadata
    // =========================================================================

    /// Set the name of a take
    async fn set_name(&self, project: ProjectContext, item: ItemRef, take: TakeRef, name: String);

    /// Set the custom color
    async fn set_color(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        color: Option<u32>,
    );

    // =========================================================================
    // Playback Properties
    // =========================================================================

    /// Set the volume (1.0 = 0dB)
    async fn set_volume(&self, project: ProjectContext, item: ItemRef, take: TakeRef, volume: f64);

    /// Set the playback rate (1.0 = normal)
    async fn set_play_rate(&self, project: ProjectContext, item: ItemRef, take: TakeRef, rate: f64);

    /// Set the pitch adjustment in semitones
    async fn set_pitch(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        semitones: f64,
    );

    /// Set whether to preserve pitch when changing playback rate
    async fn set_preserve_pitch(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        preserve: bool,
    );

    /// Set the start offset into the source
    async fn set_start_offset(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        offset: Duration,
    );

    // =========================================================================
    // Source Management
    // =========================================================================

    /// Set the source file for a take
    ///
    /// This loads an audio or MIDI file as the source for the take.
    async fn set_source_file(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        path: String,
    );

    /// Get the source type
    async fn get_source_type(
        &self,
        project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
    ) -> SourceType;
}

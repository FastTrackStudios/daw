//! Items handle and ItemHandle for individual items

use std::sync::Arc;

use crate::Result;
use crate::{DawClients, Error, MidiEditor};
use daw_proto::{
    ItemEvent, ProjectContext, TakeEvent,
    item::{FadeShape, Item, ItemRef, Take, TakeRef},
    primitives::{Duration, PositionInSeconds},
    track::TrackRef,
};
use vox::Rx;

/// Items handle for a specific track
///
/// This handle provides access to item enumeration and operations on a track.
/// Individual item operations are performed through [`ItemHandle`].
#[derive(Clone)]
pub struct Items {
    track_guid: String,
    project_id: String,
    clients: Arc<DawClients>,
}

impl Items {
    /// Create a new items handle for a track
    pub(crate) fn new(track_guid: String, project_id: String, clients: Arc<DawClients>) -> Self {
        Self {
            track_guid,
            project_id,
            clients,
        }
    }

    /// Helper to create project context
    fn context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    /// Helper to create track reference
    fn track_ref(&self) -> TrackRef {
        TrackRef::Guid(self.track_guid.clone())
    }

    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Get all items on this track
    pub async fn all(&self) -> Result<Vec<Item>> {
        let items = self
            .clients
            .item
            .get_items(self.context(), self.track_ref())
            .await?;
        Ok(items)
    }

    /// Get item by index
    pub async fn by_index(&self, index: u32) -> Result<Option<ItemHandle>> {
        let items = self
            .clients
            .item
            .get_items(self.context(), self.track_ref())
            .await?;

        let item = items.into_iter().nth(index as usize);

        Ok(item.map(|i| {
            ItemHandle::new(
                i.guid,
                self.track_guid.clone(),
                self.project_id.clone(),
                self.clients.clone(),
            )
        }))
    }

    /// Get item by GUID
    pub async fn by_guid(&self, guid: &str) -> Result<Option<ItemHandle>> {
        let item = self
            .clients
            .item
            .get_item(self.context(), ItemRef::Guid(guid.to_string()))
            .await?;

        Ok(item.map(|i| {
            ItemHandle::new(
                i.guid,
                self.track_guid.clone(),
                self.project_id.clone(),
                self.clients.clone(),
            )
        }))
    }

    /// Get item count on this track
    pub async fn count(&self) -> Result<u32> {
        let count = self
            .clients
            .item
            .item_count(self.context(), self.track_ref())
            .await?;
        Ok(count)
    }

    // =========================================================================
    // Create Items
    // =========================================================================

    /// Add a new empty item at the given position
    pub async fn add(&self, position: PositionInSeconds, length: Duration) -> Result<ItemHandle> {
        let guid = self
            .clients
            .item
            .add_item(self.context(), self.track_ref(), position, length)
            .await?
            .ok_or_else(|| Error::Other("Failed to create item".to_string()))?;

        Ok(ItemHandle::new(
            guid,
            self.track_guid.clone(),
            self.project_id.clone(),
            self.clients.clone(),
        ))
    }

    /// Create a MIDI item on this track and add notes to it.
    ///
    /// Combines `create_midi_item` + `add_notes` into a single operation.
    /// Returns the item handle if successful.
    pub async fn create_midi_item_with_notes(
        &self,
        start_seconds: f64,
        end_seconds: f64,
        notes: Vec<daw_proto::MidiNoteCreate>,
    ) -> Result<Option<ItemHandle>> {
        // Create empty MIDI item
        let location = self
            .clients
            .midi
            .create_midi_item(self.context(), self.track_ref(), start_seconds, end_seconds)
            .await?;

        let Some(location) = location else {
            return Ok(None);
        };

        // Add notes
        if !notes.is_empty() {
            self.clients.midi.add_notes(location.clone(), notes).await?;
        }

        // Return handle to the created item
        let item_guid = match &location.item {
            daw_proto::item::ItemRef::Guid(g) => g.clone(),
            _ => return Ok(None),
        };

        Ok(Some(ItemHandle::new(
            item_guid,
            self.track_guid.clone(),
            self.project_id.clone(),
            self.clients.clone(),
        )))
    }
}

impl std::fmt::Debug for Items {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Items")
            .field("track_guid", &self.track_guid)
            .field("project_id", &self.project_id)
            .finish()
    }
}

/// Project-wide items accessor
///
/// Provides access to all items in a project, selected items, etc.
#[derive(Clone)]
pub struct ProjectItems {
    project_id: String,
    clients: Arc<DawClients>,
}

impl ProjectItems {
    /// Create a new project items handle
    pub(crate) fn new(project_id: String, clients: Arc<DawClients>) -> Self {
        Self {
            project_id,
            clients,
        }
    }

    /// Helper to create project context
    fn context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    /// Get all items in the project
    pub async fn all(&self) -> Result<Vec<Item>> {
        let items = self.clients.item.get_all_items(self.context()).await?;
        Ok(items)
    }

    /// Get all selected items
    pub async fn selected(&self) -> Result<Vec<ItemHandle>> {
        let items = self.clients.item.get_selected_items(self.context()).await?;
        Ok(items
            .into_iter()
            .map(|i| {
                ItemHandle::new(
                    i.guid.clone(),
                    i.track_guid,
                    self.project_id.clone(),
                    self.clients.clone(),
                )
            })
            .collect())
    }

    /// Get item by GUID
    pub async fn by_guid(&self, guid: &str) -> Result<Option<ItemHandle>> {
        let item = self
            .clients
            .item
            .get_item(self.context(), ItemRef::Guid(guid.to_string()))
            .await?;

        Ok(item.map(|i| {
            ItemHandle::new(
                i.guid.clone(),
                i.track_guid,
                self.project_id.clone(),
                self.clients.clone(),
            )
        }))
    }

    /// Select all items
    pub async fn select_all(&self) -> Result<()> {
        self.clients
            .item
            .select_all_items(self.context(), true)
            .await?;
        Ok(())
    }

    /// Deselect all items
    pub async fn deselect_all(&self) -> Result<()> {
        self.clients
            .item
            .select_all_items(self.context(), false)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Streaming
    // =========================================================================

    /// Subscribe to item events (added, removed, moved, etc.)
    ///
    /// Returns a receiver that streams granular item events for this project.
    /// The stream continues until the returned `Rx` is dropped.
    pub async fn subscribe(&self) -> Result<Rx<ItemEvent>> {
        let (tx, rx) = vox::channel::<ItemEvent>();
        self.clients
            .item
            .subscribe_items(self.context(), tx)
            .await?;
        Ok(rx)
    }

    /// Subscribe to take events (added, removed, activated, etc.)
    ///
    /// Returns a receiver that streams granular take events for this project.
    /// The stream continues until the returned `Rx` is dropped.
    pub async fn subscribe_takes(&self) -> Result<Rx<TakeEvent>> {
        let (tx, rx) = vox::channel::<TakeEvent>();
        self.clients
            .take
            .subscribe_takes(self.context(), tx)
            .await?;
        Ok(rx)
    }
}

impl std::fmt::Debug for ProjectItems {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectItems")
            .field("project_id", &self.project_id)
            .finish()
    }
}

// =============================================================================
// ItemHandle
// =============================================================================

/// Handle to a single item
#[derive(Clone)]
pub struct ItemHandle {
    item_guid: String,
    track_guid: String,
    project_id: String,
    clients: Arc<DawClients>,
}

impl ItemHandle {
    /// Create a new item handle
    pub(crate) fn new(
        item_guid: String,
        track_guid: String,
        project_id: String,
        clients: Arc<DawClients>,
    ) -> Self {
        Self {
            item_guid,
            track_guid,
            project_id,
            clients,
        }
    }

    /// Get the item GUID
    pub fn guid(&self) -> &str {
        &self.item_guid
    }

    /// Get the track GUID
    pub fn track_guid(&self) -> &str {
        &self.track_guid
    }

    /// Helper to create project context
    fn context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    /// Helper to create item reference
    fn item_ref(&self) -> ItemRef {
        ItemRef::Guid(self.item_guid.clone())
    }

    // =========================================================================
    // Info
    // =========================================================================

    /// Get full item state
    pub async fn info(&self) -> Result<Item> {
        self.clients
            .item
            .get_item(self.context(), self.item_ref())
            .await?
            .ok_or_else(|| Error::Other(format!("Item not found: {}", self.item_guid)))
    }

    // =========================================================================
    // Position/Length
    // =========================================================================

    /// Get item position
    pub async fn position(&self) -> Result<PositionInSeconds> {
        Ok(self.info().await?.position)
    }

    /// Set item position
    pub async fn set_position(&self, position: PositionInSeconds) -> Result<()> {
        self.clients
            .item
            .set_position(self.context(), self.item_ref(), position)
            .await?;
        Ok(())
    }

    /// Get item length
    pub async fn length(&self) -> Result<Duration> {
        Ok(self.info().await?.length)
    }

    /// Set item length
    pub async fn set_length(&self, length: Duration) -> Result<()> {
        self.clients
            .item
            .set_length(self.context(), self.item_ref(), length)
            .await?;
        Ok(())
    }

    // =========================================================================
    // State
    // =========================================================================

    /// Mute the item
    pub async fn mute(&self) -> Result<()> {
        self.clients
            .item
            .set_muted(self.context(), self.item_ref(), true)
            .await?;
        Ok(())
    }

    /// Unmute the item
    pub async fn unmute(&self) -> Result<()> {
        self.clients
            .item
            .set_muted(self.context(), self.item_ref(), false)
            .await?;
        Ok(())
    }

    /// Set item color (None to use default)
    pub async fn set_color(&self, color: Option<u32>) -> Result<()> {
        self.clients
            .item
            .set_color(self.context(), self.item_ref(), color)
            .await?;
        Ok(())
    }

    /// Select the item
    pub async fn select(&self) -> Result<()> {
        self.clients
            .item
            .set_selected(self.context(), self.item_ref(), true)
            .await?;
        Ok(())
    }

    /// Deselect the item
    pub async fn deselect(&self) -> Result<()> {
        self.clients
            .item
            .set_selected(self.context(), self.item_ref(), false)
            .await?;
        Ok(())
    }

    /// Lock the item
    pub async fn lock(&self) -> Result<()> {
        self.clients
            .item
            .set_locked(self.context(), self.item_ref(), true)
            .await?;
        Ok(())
    }

    /// Unlock the item
    pub async fn unlock(&self) -> Result<()> {
        self.clients
            .item
            .set_locked(self.context(), self.item_ref(), false)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Audio
    // =========================================================================

    /// Get item volume
    pub async fn volume(&self) -> Result<f64> {
        Ok(self.info().await?.volume)
    }

    /// Set item volume
    pub async fn set_volume(&self, volume: f64) -> Result<()> {
        self.clients
            .item
            .set_volume(self.context(), self.item_ref(), volume)
            .await?;
        Ok(())
    }

    /// Set fade in
    pub async fn set_fade_in(&self, length: Duration, shape: FadeShape) -> Result<()> {
        self.clients
            .item
            .set_fade_in(self.context(), self.item_ref(), length, shape)
            .await?;
        Ok(())
    }

    /// Set fade out
    pub async fn set_fade_out(&self, length: Duration, shape: FadeShape) -> Result<()> {
        self.clients
            .item
            .set_fade_out(self.context(), self.item_ref(), length, shape)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Operations
    // =========================================================================

    /// Move this item to a different track
    pub async fn move_to_track(&self, track: TrackRef) -> Result<()> {
        self.clients
            .item
            .move_to_track(self.context(), self.item_ref(), track)
            .await?;
        Ok(())
    }

    /// Delete this item
    pub async fn delete(&self) -> Result<()> {
        self.clients
            .item
            .delete_item(self.context(), self.item_ref())
            .await?;
        Ok(())
    }

    /// Duplicate this item
    pub async fn duplicate(&self) -> Result<ItemHandle> {
        let guid = self
            .clients
            .item
            .duplicate_item(self.context(), self.item_ref())
            .await?
            .ok_or_else(|| Error::Other("Failed to duplicate item".to_string()))?;

        Ok(ItemHandle::new(
            guid,
            self.track_guid.clone(),
            self.project_id.clone(),
            self.clients.clone(),
        ))
    }

    // =========================================================================
    // Takes
    // =========================================================================

    /// Get access to this item's takes
    pub fn takes(&self) -> Takes {
        Takes::new(
            self.item_guid.clone(),
            self.project_id.clone(),
            self.clients.clone(),
        )
    }

    /// Get the active take
    pub fn active_take(&self) -> TakeHandle {
        TakeHandle::new(
            self.item_guid.clone(),
            TakeRef::Active,
            self.project_id.clone(),
            self.clients.clone(),
        )
    }
}

impl std::fmt::Debug for ItemHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ItemHandle")
            .field("item_guid", &self.item_guid)
            .field("track_guid", &self.track_guid)
            .field("project_id", &self.project_id)
            .finish()
    }
}

// =============================================================================
// Takes
// =============================================================================

/// Takes accessor for an item
#[derive(Clone)]
pub struct Takes {
    item_guid: String,
    project_id: String,
    clients: Arc<DawClients>,
}

impl Takes {
    /// Create a new takes handle
    pub(crate) fn new(item_guid: String, project_id: String, clients: Arc<DawClients>) -> Self {
        Self {
            item_guid,
            project_id,
            clients,
        }
    }

    /// Helper to create project context
    fn context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    /// Helper to create item reference
    fn item_ref(&self) -> ItemRef {
        ItemRef::Guid(self.item_guid.clone())
    }

    /// Get all takes
    pub async fn all(&self) -> Result<Vec<Take>> {
        let takes = self
            .clients
            .take
            .get_takes(self.context(), self.item_ref())
            .await?;
        Ok(takes)
    }

    /// Get take by index
    pub async fn by_index(&self, index: u32) -> Result<Option<TakeHandle>> {
        let take = self
            .clients
            .take
            .get_take(self.context(), self.item_ref(), TakeRef::Index(index))
            .await?;

        Ok(take.map(|_| {
            TakeHandle::new(
                self.item_guid.clone(),
                TakeRef::Index(index),
                self.project_id.clone(),
                self.clients.clone(),
            )
        }))
    }

    /// Get the active take
    pub async fn active(&self) -> Result<TakeHandle> {
        let _ = self
            .clients
            .take
            .get_active_take(self.context(), self.item_ref())
            .await?
            .ok_or_else(|| Error::Other("No active take".to_string()))?;

        Ok(TakeHandle::new(
            self.item_guid.clone(),
            TakeRef::Active,
            self.project_id.clone(),
            self.clients.clone(),
        ))
    }

    /// Add a new take
    pub async fn add(&self) -> Result<TakeHandle> {
        let guid = self
            .clients
            .take
            .add_take(self.context(), self.item_ref())
            .await?
            .ok_or_else(|| Error::Other("Failed to create take".to_string()))?;

        Ok(TakeHandle::new(
            self.item_guid.clone(),
            TakeRef::Guid(guid),
            self.project_id.clone(),
            self.clients.clone(),
        ))
    }
}

impl std::fmt::Debug for Takes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Takes")
            .field("item_guid", &self.item_guid)
            .field("project_id", &self.project_id)
            .finish()
    }
}

// =============================================================================
// TakeHandle
// =============================================================================

/// Handle to a single take
#[derive(Clone)]
pub struct TakeHandle {
    item_guid: String,
    take_ref: TakeRef,
    project_id: String,
    clients: Arc<DawClients>,
}

impl TakeHandle {
    /// Create a new take handle
    pub(crate) fn new(
        item_guid: String,
        take_ref: TakeRef,
        project_id: String,
        clients: Arc<DawClients>,
    ) -> Self {
        Self {
            item_guid,
            take_ref,
            project_id,
            clients,
        }
    }

    /// Helper to create project context
    fn context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    /// Helper to create item reference
    fn item_ref(&self) -> ItemRef {
        ItemRef::Guid(self.item_guid.clone())
    }

    // =========================================================================
    // Info
    // =========================================================================

    /// Get full take state
    pub async fn info(&self) -> Result<Take> {
        self.clients
            .take
            .get_take(self.context(), self.item_ref(), self.take_ref.clone())
            .await?
            .ok_or_else(|| Error::Other("Take not found".to_string()))
    }

    // =========================================================================
    // Metadata
    // =========================================================================

    /// Get take name
    pub async fn name(&self) -> Result<String> {
        Ok(self.info().await?.name)
    }

    /// Set take name
    pub async fn set_name(&self, name: &str) -> Result<()> {
        self.clients
            .take
            .set_name(
                self.context(),
                self.item_ref(),
                self.take_ref.clone(),
                name.to_string(),
            )
            .await?;
        Ok(())
    }

    // =========================================================================
    // Playback
    // =========================================================================

    /// Get take pitch adjustment (semitones)
    pub async fn pitch(&self) -> Result<f64> {
        Ok(self.info().await?.pitch)
    }

    /// Set take pitch adjustment (semitones)
    pub async fn set_pitch(&self, semitones: f64) -> Result<()> {
        self.clients
            .take
            .set_pitch(
                self.context(),
                self.item_ref(),
                self.take_ref.clone(),
                semitones,
            )
            .await?;
        Ok(())
    }

    /// Get take play rate
    pub async fn play_rate(&self) -> Result<f64> {
        Ok(self.info().await?.play_rate)
    }

    /// Set take play rate
    pub async fn set_play_rate(&self, rate: f64) -> Result<()> {
        self.clients
            .take
            .set_play_rate(self.context(), self.item_ref(), self.take_ref.clone(), rate)
            .await?;
        Ok(())
    }

    /// Get take volume
    pub async fn volume(&self) -> Result<f64> {
        Ok(self.info().await?.volume)
    }

    /// Set take volume
    pub async fn set_volume(&self, volume: f64) -> Result<()> {
        self.clients
            .take
            .set_volume(
                self.context(),
                self.item_ref(),
                self.take_ref.clone(),
                volume,
            )
            .await?;
        Ok(())
    }

    // =========================================================================
    // Operations
    // =========================================================================

    /// Make this take the active take
    pub async fn make_active(&self) -> Result<()> {
        self.clients
            .take
            .set_active_take(self.context(), self.item_ref(), self.take_ref.clone())
            .await?;
        Ok(())
    }

    /// Delete this take
    pub async fn delete(&self) -> Result<()> {
        self.clients
            .take
            .delete_take(self.context(), self.item_ref(), self.take_ref.clone())
            .await?;
        Ok(())
    }

    /// Set source file
    pub async fn set_source_file(&self, path: &str) -> Result<()> {
        self.clients
            .take
            .set_source_file(
                self.context(),
                self.item_ref(),
                self.take_ref.clone(),
                path.to_string(),
            )
            .await?;
        Ok(())
    }

    // =========================================================================
    // MIDI Editing
    // =========================================================================

    /// Get MIDI editor for this take (only for MIDI takes)
    ///
    /// Returns a handle for editing MIDI notes, CC events, and other MIDI data
    /// in this take. The take must be a MIDI take.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    ///
    /// # async fn example(handle: vox::ErasedCaller) -> daw_control::Result<()> {
    /// let daw = Daw::new(handle);
    /// let project = daw.current_project().await?;
    ///
    /// let item = project.items().selected().await?.into_iter().next().unwrap();
    /// let midi = item.active_take().midi();
    ///
    /// // Add a note (middle C, velocity 100, at beat 0, duration 1 beat)
    /// midi.add_note(60, 100, 0.0, 1.0).await?;
    ///
    /// // Quantize selected notes to 1/4 note grid
    /// midi.quantize(1.0, 1.0).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn midi(&self) -> MidiEditor {
        MidiEditor::new(
            self.item_guid.clone(),
            self.take_ref.clone(),
            self.project_id.clone(),
            self.clients.clone(),
        )
    }
}

impl std::fmt::Debug for TakeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TakeHandle")
            .field("item_guid", &self.item_guid)
            .field("take_ref", &self.take_ref)
            .field("project_id", &self.project_id)
            .finish()
    }
}

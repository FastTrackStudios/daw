//! Standalone item and take implementation

use daw_proto::{
    ProjectContext,
    item::{FadeShape, Item, ItemRef, ItemService, Take, TakeRef, TakeService},
    primitives::{BeatAttachMode, Duration, PositionInSeconds},
    track::TrackRef,
};
use roam::Context;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Internal item state
#[derive(Clone)]
pub(crate) struct ItemState {
    guid: String,
    track_guid: String,
    index: u32,
    position: PositionInSeconds,
    length: Duration,
    muted: bool,
    selected: bool,
    locked: bool,
    volume: f64,
    takes: Vec<TakeState>,
    active_take_index: u32,
}

/// Internal take state
#[derive(Clone)]
struct TakeState {
    guid: String,
    index: u32,
    name: String,
    volume: f64,
    play_rate: f64,
    pitch: f64,
    preserve_pitch: bool,
    is_midi: bool,
}

impl ItemState {
    fn new(track_guid: String, index: u32, position: PositionInSeconds, length: Duration) -> Self {
        let guid = Uuid::new_v4().to_string();
        let take = TakeState::new(0, "Take 1".to_string());
        Self {
            guid,
            track_guid,
            index,
            position,
            length,
            muted: false,
            selected: false,
            locked: false,
            volume: 1.0,
            takes: vec![take],
            active_take_index: 0,
        }
    }

    fn to_item(&self) -> Item {
        Item {
            guid: self.guid.clone(),
            track_guid: self.track_guid.clone(),
            index: self.index,
            position: self.position,
            length: self.length,
            snap_offset: Duration::ZERO,
            muted: self.muted,
            selected: self.selected,
            locked: self.locked,
            volume: self.volume,
            fade_in_length: Duration::ZERO,
            fade_out_length: Duration::ZERO,
            fade_in_shape: FadeShape::Linear,
            fade_out_shape: FadeShape::Linear,
            beat_attach_mode: BeatAttachMode::Time,
            loop_source: false,
            auto_stretch: false,
            color: None,
            group_id: None,
            take_count: self.takes.len() as u32,
            active_take_index: self.active_take_index,
        }
    }
}

impl TakeState {
    fn new(index: u32, name: String) -> Self {
        Self {
            guid: Uuid::new_v4().to_string(),
            index,
            name,
            volume: 1.0,
            play_rate: 1.0,
            pitch: 0.0,
            preserve_pitch: true,
            is_midi: false,
        }
    }

    fn to_take(&self, item_guid: &str) -> Take {
        Take {
            guid: self.guid.clone(),
            item_guid: item_guid.to_string(),
            index: self.index,
            is_active: false, // Set by caller
            name: self.name.clone(),
            color: None,
            volume: self.volume,
            play_rate: self.play_rate,
            pitch: self.pitch,
            preserve_pitch: self.preserve_pitch,
            start_offset: Duration::ZERO,
            source_type: daw_proto::item::SourceType::Empty,
            source_length: None,
            source_sample_rate: None,
            source_channels: None,
            is_midi: self.is_midi,
            midi_note_count: None,
        }
    }
}

/// Standalone item service implementation
#[derive(Clone)]
pub struct StandaloneItem {
    items: Arc<RwLock<Vec<ItemState>>>,
}

impl Default for StandaloneItem {
    fn default() -> Self {
        Self::new()
    }
}

impl StandaloneItem {
    pub fn new() -> Self {
        Self {
            items: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn find_item<'a>(items: &'a mut [ItemState], item_ref: &ItemRef) -> Option<&'a mut ItemState> {
        match item_ref {
            ItemRef::Guid(guid) => items.iter_mut().find(|i| &i.guid == guid),
            ItemRef::Index(idx) => items.iter_mut().find(|i| i.index == *idx),
            ItemRef::ProjectIndex(idx) => items.get_mut(*idx as usize),
        }
    }
}

impl ItemService for StandaloneItem {
    async fn get_items(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        track: TrackRef,
    ) -> Vec<Item> {
        let items = self.items.read().await;
        let track_guid = match track {
            TrackRef::Guid(g) => g,
            _ => return vec![],
        };
        items
            .iter()
            .filter(|i| i.track_guid == track_guid)
            .map(|i| i.to_item())
            .collect()
    }

    async fn get_item(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
    ) -> Option<Item> {
        let items = self.items.read().await;
        match &item {
            ItemRef::Guid(guid) => items.iter().find(|i| &i.guid == guid).map(|i| i.to_item()),
            ItemRef::Index(_) | ItemRef::ProjectIndex(_) => None,
        }
    }

    async fn get_all_items(&self, _cx: &Context, _project: ProjectContext) -> Vec<Item> {
        let items = self.items.read().await;
        items.iter().map(|i| i.to_item()).collect()
    }

    async fn get_selected_items(&self, _cx: &Context, _project: ProjectContext) -> Vec<Item> {
        let items = self.items.read().await;
        items
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.to_item())
            .collect()
    }

    async fn item_count(&self, _cx: &Context, _project: ProjectContext, track: TrackRef) -> u32 {
        let items = self.items.read().await;
        let track_guid = match track {
            TrackRef::Guid(g) => g,
            _ => return 0,
        };
        items.iter().filter(|i| i.track_guid == track_guid).count() as u32
    }

    async fn add_item(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        track: TrackRef,
        position: PositionInSeconds,
        length: Duration,
    ) -> Option<String> {
        let track_guid = match track {
            TrackRef::Guid(g) => g,
            _ => return None,
        };
        let mut items = self.items.write().await;
        let index = items.len() as u32;
        let item = ItemState::new(track_guid, index, position, length);
        let guid = item.guid.clone();
        items.push(item);
        Some(guid)
    }

    async fn delete_item(&self, _cx: &Context, _project: ProjectContext, item: ItemRef) {
        let mut items = self.items.write().await;
        if let ItemRef::Guid(guid) = item {
            items.retain(|i| i.guid != guid);
        }
    }

    async fn duplicate_item(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
    ) -> Option<String> {
        let mut items = self.items.write().await;
        let source = Self::find_item(&mut items, &item)?.clone();
        let mut new_item = source;
        new_item.guid = Uuid::new_v4().to_string();
        new_item.index = items.len() as u32;
        let guid = new_item.guid.clone();
        items.push(new_item);
        Some(guid)
    }

    async fn set_position(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        position: PositionInSeconds,
    ) {
        let mut items = self.items.write().await;
        if let Some(i) = Self::find_item(&mut items, &item) {
            i.position = position;
        }
    }

    async fn set_length(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        length: Duration,
    ) {
        let mut items = self.items.write().await;
        if let Some(i) = Self::find_item(&mut items, &item) {
            i.length = length;
        }
    }

    async fn move_to_track(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        track: TrackRef,
    ) {
        let track_guid = match track {
            TrackRef::Guid(g) => g,
            _ => return,
        };
        let mut items = self.items.write().await;
        if let Some(i) = Self::find_item(&mut items, &item) {
            i.track_guid = track_guid;
        }
    }

    async fn set_muted(&self, _cx: &Context, _project: ProjectContext, item: ItemRef, muted: bool) {
        let mut items = self.items.write().await;
        if let Some(i) = Self::find_item(&mut items, &item) {
            i.muted = muted;
        }
    }

    async fn set_selected(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        selected: bool,
    ) {
        let mut items = self.items.write().await;
        if let Some(i) = Self::find_item(&mut items, &item) {
            i.selected = selected;
        }
    }

    async fn set_locked(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        locked: bool,
    ) {
        let mut items = self.items.write().await;
        if let Some(i) = Self::find_item(&mut items, &item) {
            i.locked = locked;
        }
    }

    async fn select_all_items(&self, _cx: &Context, _project: ProjectContext, selected: bool) {
        let mut items = self.items.write().await;
        for i in items.iter_mut() {
            i.selected = selected;
        }
    }

    async fn set_volume(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        volume: f64,
    ) {
        let mut items = self.items.write().await;
        if let Some(i) = Self::find_item(&mut items, &item) {
            i.volume = volume;
        }
    }

    async fn set_fade_in(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _item: ItemRef,
        _length: Duration,
        _shape: FadeShape,
    ) {
        // Stub - no-op
    }

    async fn set_fade_out(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _item: ItemRef,
        _length: Duration,
        _shape: FadeShape,
    ) {
        // Stub - no-op
    }

    async fn set_loop_source(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _item: ItemRef,
        _loop_source: bool,
    ) {
        // Stub - no-op
    }

    async fn set_beat_attach_mode(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _item: ItemRef,
        _mode: BeatAttachMode,
    ) {
        // Stub - no-op
    }

    async fn set_snap_offset(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _item: ItemRef,
        _offset: Duration,
    ) {
        // Stub - no-op
    }

    async fn set_auto_stretch(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _item: ItemRef,
        _auto_stretch: bool,
    ) {
        // Stub - no-op
    }

    async fn set_color(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _item: ItemRef,
        _color: Option<u32>,
    ) {
        // Stub - no-op
    }

    async fn set_group_id(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _item: ItemRef,
        _group_id: Option<u32>,
    ) {
        // Stub - no-op
    }
}

/// Standalone take service implementation
#[derive(Clone)]
pub struct StandaloneTake {
    items: Arc<RwLock<Vec<ItemState>>>,
}

impl StandaloneTake {
    #[allow(dead_code)]
    pub(crate) fn new(items: Arc<RwLock<Vec<ItemState>>>) -> Self {
        Self { items }
    }

    /// Create with shared state from StandaloneItem
    pub fn from_item_service(item_service: &StandaloneItem) -> Self {
        Self {
            items: item_service.items.clone(),
        }
    }
}

impl TakeService for StandaloneTake {
    async fn get_takes(&self, _cx: &Context, _project: ProjectContext, item: ItemRef) -> Vec<Take> {
        let items = self.items.read().await;
        let item_state = match &item {
            ItemRef::Guid(guid) => items.iter().find(|i| &i.guid == guid),
            _ => None,
        };
        item_state
            .map(|i| {
                i.takes
                    .iter()
                    .enumerate()
                    .map(|(idx, t)| {
                        let mut take = t.to_take(&i.guid);
                        take.is_active = idx as u32 == i.active_take_index;
                        take
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn get_take(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
    ) -> Option<Take> {
        let items = self.items.read().await;
        let item_state = match &item {
            ItemRef::Guid(guid) => items.iter().find(|i| &i.guid == guid),
            _ => None,
        }?;

        let take_state = match &take {
            TakeRef::Guid(guid) => item_state.takes.iter().find(|t| &t.guid == guid),
            TakeRef::Index(idx) => item_state.takes.get(*idx as usize),
            TakeRef::Active => item_state.takes.get(item_state.active_take_index as usize),
        }?;

        let mut result = take_state.to_take(&item_state.guid);
        result.is_active = take_state.index == item_state.active_take_index;
        Some(result)
    }

    async fn get_active_take(
        &self,
        cx: &Context,
        project: ProjectContext,
        item: ItemRef,
    ) -> Option<Take> {
        self.get_take(cx, project, item, TakeRef::Active).await
    }

    async fn add_take(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
    ) -> Option<String> {
        let mut items = self.items.write().await;
        let item_state = match &item {
            ItemRef::Guid(guid) => items.iter_mut().find(|i| &i.guid == guid),
            _ => None,
        }?;

        let index = item_state.takes.len() as u32;
        let take = TakeState::new(index, format!("Take {}", index + 1));
        let guid = take.guid.clone();
        item_state.takes.push(take);
        Some(guid)
    }

    async fn delete_take(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
    ) {
        let mut items = self.items.write().await;
        let item_state = match &item {
            ItemRef::Guid(guid) => items.iter_mut().find(|i| &i.guid == guid),
            _ => None,
        };
        if let Some(item_state) = item_state
            && let TakeRef::Guid(guid) = take
        {
            item_state.takes.retain(|t| t.guid != guid);
        }
    }

    async fn set_active_take(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
    ) {
        let mut items = self.items.write().await;
        let item_state = match &item {
            ItemRef::Guid(guid) => items.iter_mut().find(|i| &i.guid == guid),
            _ => None,
        };
        if let Some(item_state) = item_state {
            let idx = match &take {
                TakeRef::Index(i) => Some(*i),
                TakeRef::Guid(guid) => item_state
                    .takes
                    .iter()
                    .position(|t| &t.guid == guid)
                    .map(|i| i as u32),
                TakeRef::Active => Some(item_state.active_take_index),
            };
            if let Some(idx) = idx {
                item_state.active_take_index = idx;
            }
        }
    }

    async fn set_name(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        name: String,
    ) {
        let mut items = self.items.write().await;
        let item_state = match &item {
            ItemRef::Guid(guid) => items.iter_mut().find(|i| &i.guid == guid),
            _ => None,
        };
        if let Some(item_state) = item_state {
            let take_state = match &take {
                TakeRef::Guid(guid) => item_state.takes.iter_mut().find(|t| &t.guid == guid),
                TakeRef::Index(idx) => item_state.takes.get_mut(*idx as usize),
                TakeRef::Active => item_state
                    .takes
                    .get_mut(item_state.active_take_index as usize),
            };
            if let Some(take_state) = take_state {
                take_state.name = name;
            }
        }
    }

    async fn set_color(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _item: ItemRef,
        _take: TakeRef,
        _color: Option<u32>,
    ) {
        // Stub - no-op
    }

    async fn set_volume(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        volume: f64,
    ) {
        let mut items = self.items.write().await;
        let item_state = match &item {
            ItemRef::Guid(guid) => items.iter_mut().find(|i| &i.guid == guid),
            _ => None,
        };
        if let Some(item_state) = item_state {
            let take_state = match &take {
                TakeRef::Guid(guid) => item_state.takes.iter_mut().find(|t| &t.guid == guid),
                TakeRef::Index(idx) => item_state.takes.get_mut(*idx as usize),
                TakeRef::Active => item_state
                    .takes
                    .get_mut(item_state.active_take_index as usize),
            };
            if let Some(take_state) = take_state {
                take_state.volume = volume;
            }
        }
    }

    async fn set_play_rate(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        rate: f64,
    ) {
        let mut items = self.items.write().await;
        let item_state = match &item {
            ItemRef::Guid(guid) => items.iter_mut().find(|i| &i.guid == guid),
            _ => None,
        };
        if let Some(item_state) = item_state {
            let take_state = match &take {
                TakeRef::Guid(guid) => item_state.takes.iter_mut().find(|t| &t.guid == guid),
                TakeRef::Index(idx) => item_state.takes.get_mut(*idx as usize),
                TakeRef::Active => item_state
                    .takes
                    .get_mut(item_state.active_take_index as usize),
            };
            if let Some(take_state) = take_state {
                take_state.play_rate = rate;
            }
        }
    }

    async fn set_pitch(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        semitones: f64,
    ) {
        let mut items = self.items.write().await;
        let item_state = match &item {
            ItemRef::Guid(guid) => items.iter_mut().find(|i| &i.guid == guid),
            _ => None,
        };
        if let Some(item_state) = item_state {
            let take_state = match &take {
                TakeRef::Guid(guid) => item_state.takes.iter_mut().find(|t| &t.guid == guid),
                TakeRef::Index(idx) => item_state.takes.get_mut(*idx as usize),
                TakeRef::Active => item_state
                    .takes
                    .get_mut(item_state.active_take_index as usize),
            };
            if let Some(take_state) = take_state {
                take_state.pitch = semitones;
            }
        }
    }

    async fn set_preserve_pitch(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
        preserve: bool,
    ) {
        let mut items = self.items.write().await;
        let item_state = match &item {
            ItemRef::Guid(guid) => items.iter_mut().find(|i| &i.guid == guid),
            _ => None,
        };
        if let Some(item_state) = item_state {
            let take_state = match &take {
                TakeRef::Guid(guid) => item_state.takes.iter_mut().find(|t| &t.guid == guid),
                TakeRef::Index(idx) => item_state.takes.get_mut(*idx as usize),
                TakeRef::Active => item_state
                    .takes
                    .get_mut(item_state.active_take_index as usize),
            };
            if let Some(take_state) = take_state {
                take_state.preserve_pitch = preserve;
            }
        }
    }

    async fn set_start_offset(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _item: ItemRef,
        _take: TakeRef,
        _offset: Duration,
    ) {
        // Stub - no-op
    }

    async fn set_source_file(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _item: ItemRef,
        _take: TakeRef,
        _path: String,
    ) {
        // Stub - no-op
    }

    async fn take_count(&self, _cx: &Context, _project: ProjectContext, item: ItemRef) -> u32 {
        let items = self.items.read().await;
        match &item {
            ItemRef::Guid(guid) => items
                .iter()
                .find(|i| &i.guid == guid)
                .map(|i| i.takes.len() as u32)
                .unwrap_or(0),
            _ => 0,
        }
    }

    async fn get_source_type(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        item: ItemRef,
        take: TakeRef,
    ) -> daw_proto::item::SourceType {
        let items = self.items.read().await;
        let item_state = match &item {
            ItemRef::Guid(guid) => items.iter().find(|i| &i.guid == guid),
            _ => None,
        };
        if let Some(item_state) = item_state {
            let take_state = match &take {
                TakeRef::Guid(guid) => item_state.takes.iter().find(|t| &t.guid == guid),
                TakeRef::Index(idx) => item_state.takes.get(*idx as usize),
                TakeRef::Active => item_state.takes.get(item_state.active_take_index as usize),
            };
            if let Some(take_state) = take_state {
                if take_state.is_midi {
                    return daw_proto::item::SourceType::Midi;
                }
                return daw_proto::item::SourceType::Audio;
            }
        }
        daw_proto::item::SourceType::Empty
    }
}

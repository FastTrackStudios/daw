//! Safe wrappers for REAPER item/take APIs.
//!
//! Covers both low-level (`medium.low()`) and unsafe medium-level calls
//! so that service code in `item.rs` can be 100% safe Rust.

use std::ffi::CStr;

use super::ReaperLow;
use reaper_medium::{
    DurationInSeconds, ItemAttributeKey, MediaItem, MediaItemTake, MediaTrack,
    ProjectContext as ReaperProjectContext, Semitones, TakeAttributeKey, UiRefreshBehavior,
};

// =============================================================================
// Low-level wrappers (via medium.low())
// =============================================================================

/// Get item state chunk (serialized RPP text).
pub fn get_item_state_chunk(
    low: &ReaperLow,
    item: MediaItem,
    buffer_size: usize,
) -> Option<String> {
    super::buffer::with_string_buffer(buffer_size, |buf, len| unsafe {
        low.GetSetItemState(item.as_ptr(), buf, len)
    })
}

/// Get a track's GUID string.
pub fn get_track_guid(low: &ReaperLow, track: MediaTrack) -> String {
    let mut buf = vec![0u8; 256];
    unsafe {
        low.GetSetMediaTrackInfo_String(
            track.as_ptr(),
            c"GUID".as_ptr(),
            buf.as_mut_ptr() as *mut i8,
            false,
        );
    }
    super::buffer::string_from_buffer(&buf)
}

/// Check if an item is selected.
pub fn is_item_selected(low: &ReaperLow, item: MediaItem) -> bool {
    unsafe { low.IsMediaItemSelected(item.as_ptr()) }
}

/// Count takes in an item.
pub fn count_takes(low: &ReaperLow, item: MediaItem) -> i32 {
    unsafe { low.CountTakes(item.as_ptr()) }
}

/// Get a take by index within an item.
pub fn get_take(low: &ReaperLow, item: MediaItem, index: i32) -> Option<MediaItemTake> {
    let ptr = unsafe { low.GetTake(item.as_ptr(), index) };
    MediaItemTake::new(ptr)
}

/// Get a take by index, returning a medium-level newtype.
#[deprecated(note = "use get_take instead, which now returns Option<MediaItemTake>")]
pub fn get_take_medium(low: &ReaperLow, item: MediaItem, index: i32) -> Option<MediaItemTake> {
    get_take(low, item, index)
}

/// Set the active take for an item.
pub fn set_active_take(low: &ReaperLow, take: MediaItemTake) {
    unsafe {
        low.SetActiveTake(take.as_ptr());
    }
}

/// Set a take's name.
pub fn set_take_name(low: &ReaperLow, take: MediaItemTake, name: &std::ffi::CString) {
    unsafe {
        low.GetSetMediaItemTakeInfo_String(
            take.as_ptr(),
            c"P_NAME".as_ptr(),
            name.as_ptr() as *mut i8,
            true,
        );
    }
}

/// Read a take's name into a buffer.
pub fn get_take_name(low: &ReaperLow, take: MediaItemTake) -> String {
    let mut buf = vec![0u8; 256];
    unsafe {
        low.GetSetMediaItemTakeInfo_String(
            take.as_ptr(),
            c"P_NAME".as_ptr(),
            buf.as_mut_ptr() as *mut i8,
            false,
        );
    }
    super::buffer::string_from_buffer(&buf)
}

/// Move a media item to a different track.
pub fn move_item_to_track(low: &ReaperLow, item: MediaItem, track: MediaTrack) {
    unsafe {
        low.MoveMediaItemToTrack(item.as_ptr(), track.as_ptr());
    }
}

/// Get the media item that owns a take.
pub fn get_take_item(low: &ReaperLow, take: MediaItemTake) -> Option<MediaItem> {
    let ptr = unsafe { low.GetMediaItemTake_Item(take.as_ptr()) };
    MediaItem::new(ptr)
}

/// Get a float info value from a media item.
pub fn get_media_item_info_value(low: &ReaperLow, item: MediaItem, attr: &CStr) -> f64 {
    unsafe { low.GetMediaItemInfo_Value(item.as_ptr(), attr.as_ptr()) }
}

// =============================================================================
// Medium-level wrappers (unsafe reaper_medium calls)
// =============================================================================

/// Get a media item from a track by index.
pub fn get_track_media_item(
    medium: &reaper_medium::Reaper,
    track: MediaTrack,
    index: u32,
) -> Option<MediaItem> {
    unsafe { medium.get_track_media_item(track, index) }
}

/// Get the track that owns a media item.
pub fn get_media_item_track(medium: &reaper_medium::Reaper, item: MediaItem) -> Option<MediaTrack> {
    unsafe { medium.get_media_item_track(item) }
}

/// Count media items on a track.
pub fn count_track_media_items(medium: &reaper_medium::Reaper, track: MediaTrack) -> u32 {
    unsafe { medium.count_track_media_items(track) }
}

/// Get an item attribute value (medium-level, typed key).
pub fn get_item_info_value(
    medium: &reaper_medium::Reaper,
    item: MediaItem,
    key: ItemAttributeKey,
) -> f64 {
    unsafe { medium.get_media_item_info_value(item, key) }
}

/// Set an item attribute value (medium-level, typed key).
pub fn set_item_info_value(
    medium: &reaper_medium::Reaper,
    item: MediaItem,
    key: ItemAttributeKey,
    value: f64,
) {
    unsafe {
        let _ = medium.set_media_item_info_value(item, key, value);
    }
}

/// Set media item position.
pub fn set_media_item_position(
    medium: &reaper_medium::Reaper,
    item: MediaItem,
    position: reaper_medium::PositionInSeconds,
    refresh: UiRefreshBehavior,
) {
    unsafe {
        let _ = medium.set_media_item_position(item, position, refresh);
    }
}

/// Set media item length.
pub fn set_media_item_length(
    medium: &reaper_medium::Reaper,
    item: MediaItem,
    length: DurationInSeconds,
    refresh: UiRefreshBehavior,
) {
    unsafe {
        let _ = medium.set_media_item_length(item, length, refresh);
    }
}

/// Set media item selected state.
pub fn set_media_item_selected(medium: &reaper_medium::Reaper, item: MediaItem, selected: bool) {
    unsafe {
        medium.set_media_item_selected(item, selected);
    }
}

/// Add a new media item to a track.
pub fn add_media_item_to_track(
    medium: &reaper_medium::Reaper,
    track: MediaTrack,
) -> Option<MediaItem> {
    unsafe { medium.add_media_item_to_track(track).ok() }
}

/// Delete a media item from a track.
pub fn delete_track_media_item(medium: &reaper_medium::Reaper, track: MediaTrack, item: MediaItem) {
    unsafe {
        let _ = medium.delete_track_media_item(track, item);
    }
}

/// Execute a REAPER action by command ID.
pub fn main_on_command_ex(
    medium: &reaper_medium::Reaper,
    command_id: reaper_medium::CommandId,
    flag: i32,
    project: ReaperProjectContext,
) {
    medium.main_on_command_ex(command_id, flag, project);
}

/// Get the active take of a media item.
pub fn get_active_take(medium: &reaper_medium::Reaper, item: MediaItem) -> Option<MediaItemTake> {
    unsafe { medium.get_active_take(item) }
}

/// Add a new take to a media item.
pub fn add_take_to_media_item(
    medium: &reaper_medium::Reaper,
    item: MediaItem,
) -> Option<MediaItemTake> {
    unsafe { medium.add_take_to_media_item(item).ok() }
}

/// Get a take attribute value.
pub fn get_take_info_value(
    medium: &reaper_medium::Reaper,
    take: MediaItemTake,
    key: TakeAttributeKey,
) -> f64 {
    unsafe { medium.get_media_item_take_info_value(take, key) }
}

/// Set a take attribute value.
pub fn set_take_info_value(
    medium: &reaper_medium::Reaper,
    take: MediaItemTake,
    key: TakeAttributeKey,
    value: f64,
) {
    unsafe {
        let _ = medium.set_media_item_take_info_value(take, key, value);
    }
}

/// Get the pitch of a take in semitones.
pub fn get_take_pitch(medium: &reaper_medium::Reaper, take: MediaItemTake) -> f64 {
    unsafe { medium.get_set_media_item_take_info_get_pitch(take).get() }
}

/// Set the pitch of a take in semitones.
pub fn set_take_pitch(medium: &reaper_medium::Reaper, take: MediaItemTake, semitones: Semitones) {
    unsafe {
        medium.get_set_media_item_take_info_set_pitch(take, semitones);
    }
}

/// Get the PCM source of a take.
pub fn get_take_source(
    medium: &reaper_medium::Reaper,
    take: MediaItemTake,
) -> Option<reaper_medium::PcmSource> {
    unsafe { medium.get_media_item_take_source(take) }
}

/// Get the source file path for a take (returns None for MIDI/empty takes).
pub fn get_take_source_file_path(
    medium: &reaper_medium::Reaper,
    take: MediaItemTake,
) -> Option<String> {
    let source = unsafe { medium.get_media_item_take_source(take)? };
    let low = medium.low();
    let mut buf = vec![0u8; 4096];
    unsafe {
        low.GetMediaSourceFileName(
            source.as_ptr(),
            buf.as_mut_ptr() as *mut i8,
            buf.len() as i32,
        );
    }
    let path = super::buffer::string_from_buffer(&buf);
    if path.is_empty() { None } else { Some(path) }
}

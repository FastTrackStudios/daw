//! Safe wrappers for REAPER MIDI APIs.

use super::ReaperLow;
use reaper_medium::{MediaItem, MediaItemTake, MediaTrack};

/// Counts of MIDI events in a take.
pub struct MidiEventCounts {
    pub notes: i32,
    pub ccs: i32,
    pub text_sysex: i32,
}

/// Count MIDI events (notes, CCs, text/sysex) in a take.
pub fn count_events(low: &ReaperLow, take: MediaItemTake) -> MidiEventCounts {
    let mut notes: i32 = 0;
    let mut ccs: i32 = 0;
    let mut text_sysex: i32 = 0;
    unsafe {
        low.MIDI_CountEvts(take.as_ptr(), &mut notes, &mut ccs, &mut text_sysex);
    }
    MidiEventCounts {
        notes,
        ccs,
        text_sysex,
    }
}

/// Raw MIDI note data.
pub struct MidiNoteRaw {
    pub selected: bool,
    pub muted: bool,
    pub start_ppq: f64,
    pub end_ppq: f64,
    pub channel: i32,
    pub pitch: i32,
    pub velocity: i32,
}

/// Get a MIDI note by index.
pub fn get_note(low: &ReaperLow, take: MediaItemTake, index: i32) -> Option<MidiNoteRaw> {
    let mut n = MidiNoteRaw {
        selected: false,
        muted: false,
        start_ppq: 0.0,
        end_ppq: 0.0,
        channel: 0,
        pitch: 0,
        velocity: 0,
    };
    let ok = unsafe {
        low.MIDI_GetNote(
            take.as_ptr(),
            index,
            &mut n.selected,
            &mut n.muted,
            &mut n.start_ppq,
            &mut n.end_ppq,
            &mut n.channel,
            &mut n.pitch,
            &mut n.velocity,
        )
    };
    ok.then_some(n)
}

/// Insert a MIDI note.
#[allow(clippy::too_many_arguments)]
pub fn insert_note(
    low: &ReaperLow,
    take: MediaItemTake,
    selected: bool,
    muted: bool,
    start_ppq: f64,
    end_ppq: f64,
    channel: i32,
    pitch: i32,
    velocity: i32,
) {
    unsafe {
        low.MIDI_InsertNote(
            take.as_ptr(),
            selected,
            muted,
            start_ppq,
            end_ppq,
            channel,
            pitch,
            velocity,
            std::ptr::null_mut(),
        );
    }
}

/// Sort MIDI events in a take.
pub fn sort(low: &ReaperLow, take: MediaItemTake) {
    unsafe { low.MIDI_Sort(take.as_ptr()) };
}

/// Convert a project quarter-note position to PPQ for a take.
pub fn get_ppq_pos_from_proj_qn(low: &ReaperLow, take: MediaItemTake, qn: f64) -> f64 {
    unsafe { low.MIDI_GetPPQPosFromProjQN(take.as_ptr(), qn) }
}

/// Check if a take contains MIDI data.
pub fn take_is_midi(low: &ReaperLow, take: MediaItemTake) -> bool {
    unsafe { low.TakeIsMIDI(take.as_ptr()) }
}

/// Create a new MIDI item in a project.
pub fn create_new_midi_item(
    low: &ReaperLow,
    track: MediaTrack,
    start: f64,
    end: f64,
) -> Option<MediaItem> {
    let ptr =
        unsafe { low.CreateNewMIDIItemInProj(track.as_ptr(), start, end, std::ptr::null_mut()) };
    MediaItem::new(ptr)
}

/// Get the active take of a media item (low-level).
pub fn get_active_take(low: &ReaperLow, item: MediaItem) -> Option<MediaItemTake> {
    let ptr = unsafe { low.GetActiveTake(item.as_ptr()) };
    MediaItemTake::new(ptr)
}

/// Delete a MIDI note by index.
///
/// Indices shift down as notes are removed, so a caller deleting
/// several must work from the highest index down.
pub fn delete_note(low: &ReaperLow, take: MediaItemTake, index: i32) -> bool {
    unsafe { low.MIDI_DeleteNote(take.as_ptr(), index) }
}

/// Overwrite a note's fields. `None` leaves a field as it is.
#[allow(clippy::too_many_arguments)]
pub fn set_note(
    low: &ReaperLow,
    take: MediaItemTake,
    index: i32,
    selected: Option<bool>,
    muted: Option<bool>,
    start_ppq: Option<f64>,
    end_ppq: Option<f64>,
    channel: Option<i32>,
    pitch: Option<i32>,
    velocity: Option<i32>,
) -> bool {
    // REAPER reads through the pointers it is given and ignores null
    // ones, which is how a partial update is expressed.
    let mut sel = selected.unwrap_or(false);
    let mut mute = muted.unwrap_or(false);
    let mut start = start_ppq.unwrap_or(0.0);
    let mut end = end_ppq.unwrap_or(0.0);
    let mut chan = channel.unwrap_or(0);
    let mut p = pitch.unwrap_or(0);
    let mut vel = velocity.unwrap_or(0);
    unsafe {
        low.MIDI_SetNote(
            take.as_ptr(),
            index,
            if selected.is_some() { &mut sel } else { std::ptr::null_mut() },
            if muted.is_some() { &mut mute } else { std::ptr::null_mut() },
            if start_ppq.is_some() { &mut start } else { std::ptr::null_mut() },
            if end_ppq.is_some() { &mut end } else { std::ptr::null_mut() },
            if channel.is_some() { &mut chan } else { std::ptr::null_mut() },
            if pitch.is_some() { &mut p } else { std::ptr::null_mut() },
            if velocity.is_some() { &mut vel } else { std::ptr::null_mut() },
            std::ptr::null_mut(),
        )
    }
}

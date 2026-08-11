//! Flams: the same drum, both hands, a hair apart.
//!
//! A flam is one piece struck by both hands with a small offset — the
//! grace note lands just before or just after the main hit. It is the
//! reason a drum row is *two-handed* at all: for most parts nobody cares
//! which stick hit the snare, so the roll shows one row, and it splits
//! only when something needs the distinction. Notated sticking is the
//! other reason.
//!
//! ## The offset
//!
//! 30 ms by default, and that number is measured rather than chosen.
//! #176's corpus work swept real flams and found they resolve into two
//! hits from about 40 ms apart, with 75% sitting at **25–35 ms** and
//! nothing below 15 ms. A "flam" written at 5 ms is a doubled hit; one
//! at 100 ms is two notes.
//!
//! Expressed in milliseconds and converted through the document's own
//! time base, because a flam is a physical gesture — two sticks and one
//! wrist — and does not scale with tempo the way a subdivision does.

use crate::doc::{ExpressionDoc, Note, NoteId};
use crate::edit::Edit;
use crate::rows::DrumMap;

/// Which side of the main hit the grace note falls on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FlamSide {
    /// Grace note before the beat — the ordinary flam, and the default.
    #[default]
    Before,
    /// Grace note after: a drag, or a deliberate reverse flam.
    After,
}

impl FlamSide {
    /// What pressing the key again does.
    pub fn toggled(self) -> Self {
        match self {
            FlamSide::Before => FlamSide::After,
            FlamSide::After => FlamSide::Before,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FlamSide::Before => "Flam before",
            FlamSide::After => "Flam after",
        }
    }
}

/// Measured from real playing: see the module docs.
pub const DEFAULT_FLAM_MS: f64 = 30.0;

/// How loud the grace note is relative to the main hit.
///
/// A flam's grace note is quieter — that is what makes it read as one
/// gesture rather than two hits. Not silent, or the flam disappears.
pub const GRACE_VELOCITY: f64 = 0.55;

/// Why a flam could not be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlamError {
    /// Nothing selected, or the note is gone.
    NoNote,
    /// The row is a single-handed piece — a hi-hat has no other hand to
    /// flam with.
    NotTwoHanded,
    /// The other hand already has a hit there, so this would stack two
    /// notes on one row.
    AlreadyFlammed,
}

/// Build the edit that turns a hit into a flam.
///
/// The grace note goes on the **other hand's row**, which is the whole
/// point: a flam played with one hand twice is a drag, and the roll
/// should show the difference. Its length matches the main hit, and its
/// velocity is lower so it reads as a grace note.
pub fn flam(
    doc: &ExpressionDoc,
    map: &DrumMap,
    id: NoteId,
    side: FlamSide,
    offset_ms: f64,
    bpm: f64,
) -> Result<Edit, FlamError> {
    let note = doc.note(id).ok_or(FlamError::NoNote)?;
    let row = note.row.max(0) as usize;
    let other = map.other_hand_row(row).ok_or(FlamError::NotTwoHanded)?;

    let units_per_second = doc.time_base.units_per_second(bpm).max(1e-9);
    let offset = (offset_ms.abs() / 1000.0) * units_per_second;
    let start = match side {
        FlamSide::Before => note.start - offset,
        FlamSide::After => note.start + offset,
    };

    // Refuse rather than stack: two notes on one row at one tick is not
    // a flam, it is a bug you find later by ear.
    let occupied = doc
        .notes
        .iter()
        .any(|n| n.row == other as i32 && (n.start - start).abs() < offset * 0.5);
    if occupied {
        return Err(FlamError::AlreadyFlammed);
    }

    let mut grace = Note::new(
        NoteId(next_id(doc)),
        start,
        start + (note.end - note.start),
        other as i32,
    );
    grace.velocity = (note.velocity * GRACE_VELOCITY).clamp(0.0, 1.0);
    grace.channel = note.channel;

    Ok(Edit::AddNote(Box::new(grace)))
}

/// The next free note id in a document.
fn next_id(doc: &ExpressionDoc) -> u64 {
    doc.notes.iter().map(|n| n.id.0).max().unwrap_or(0) + 1
}

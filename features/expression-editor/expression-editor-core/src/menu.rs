//! The note context menu.
//!
//! Riffer §6.2.2 is the source for the core set — cut, copy, paste,
//! delete, select all, clear, copy measure, note properties — and the
//! per-mode additions come from what each product's note actually
//! carries: a lyric in Vocals, a fret and an articulation in Guitar, a
//! channel in MPE.
//!
//! The menu is **built in core and rendered by the UI**, so what it
//! offers is testable without a browser and cannot drift between the
//! standalone, plugin and REAPER-embedded surfaces.

use crate::doc::NoteId;
use crate::mode::Mode;
use crate::Editor;

/// What a menu entry does when chosen.
///
/// Deliberately separate from [`crate::mouse::Action`]: an action is
/// bound to a gesture and often opens a drag, where a command is a
/// discrete thing that happens once. Overlapping them would force every
/// mouse binding to be menu-able and vice versa.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
    /// Select every note in the bar under the pointer.
    SelectMeasure,
    /// Copy the bar under the pointer.
    CopyMeasure,
    /// Clear expression from the selection, keeping the notes.
    ClearExpression,
    ToggleMute,
    /// Open the inspector on the selection.
    Properties,

    // ── per-mode ─────────────────────────────────────────────────────
    /// Vocals: type a syllable onto the note.
    EditLyric(NoteId),
    /// Guitar/bass: set the playing technique.
    SetArticulation(NoteId),
    /// Guitar/bass: re-finger at the same sounding pitch.
    CycleString(NoteId),
    /// Guitar/bass: mark the note joined to the next on its string.
    ToggleLegato(NoteId),
    /// MPE: reassign member channels across the selection.
    AssignChannels,
    /// Audio: split the detected note at the pointer.
    SplitNote(NoteId, f64),
    /// Audio: merge with the following note.
    MergeNotes(NoteId),
}

/// One row of the menu.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    pub label: String,
    pub command: Command,
    /// Greyed rather than hidden. A menu whose shape changes with the
    /// selection is a menu you cannot learn — the items stay put and
    /// stop responding.
    pub enabled: bool,
    /// Draw a rule above this item.
    pub group_break: bool,
    /// Shown right-aligned. Not bound here; the UI owns the keymap.
    pub shortcut: Option<&'static str>,
}

impl MenuItem {
    fn new(label: impl Into<String>, command: Command, enabled: bool) -> Self {
        Self {
            label: label.into(),
            command,
            enabled,
            group_break: false,
            shortcut: None,
        }
    }

    fn key(mut self, k: &'static str) -> Self {
        self.shortcut = Some(k);
        self
    }

    fn group(mut self) -> Self {
        self.group_break = true;
        self
    }
}

/// Build the context menu for a right-click at `t`, over `under`.
///
/// A right-click on an unselected note acts on *that* note — the
/// alternative, silently operating on a selection elsewhere on screen,
/// is how menus delete the wrong thing.
pub fn note_menu(ed: &Editor, under: Option<NoteId>, t: f64) -> Vec<MenuItem> {
    let has_sel = under.is_some() || !ed.selection.is_empty();
    let has_notes = !ed.doc.notes.is_empty();
    let can_paste = !ed.clipboard.is_empty();

    let mut items = vec![
        MenuItem::new("Cut", Command::Cut, has_sel).key("Ctrl+X"),
        MenuItem::new("Copy", Command::Copy, has_sel).key("Ctrl+C"),
        MenuItem::new("Paste", Command::Paste, can_paste).key("Ctrl+V"),
        MenuItem::new("Delete", Command::Delete, has_sel).key("Del"),
        MenuItem::new("Select All", Command::SelectAll, has_notes)
            .key("Ctrl+A")
            .group(),
        MenuItem::new("Select Measure", Command::SelectMeasure, has_notes),
        MenuItem::new("Copy Measure", Command::CopyMeasure, has_notes),
        MenuItem::new("Clear Expression", Command::ClearExpression, has_sel).group(),
        MenuItem::new("Mute", Command::ToggleMute, has_sel).key("M"),
    ];

    // Mode-specific entries, keyed on what the note in that product
    // actually carries. Each needs a concrete note, so they only appear
    // when the click landed on one.
    if let Some(id) = under {
        let mut extra = match ed.mode {
            Mode::Vocals => vec![MenuItem::new("Edit Lyric…", Command::EditLyric(id), true)],
            Mode::Guitar => vec![
                MenuItem::new("Articulation…", Command::SetArticulation(id), true),
                MenuItem::new("Next String", Command::CycleString(id), true),
                MenuItem::new("Legato", Command::ToggleLegato(id), true),
            ],
            Mode::Mpe => vec![MenuItem::new(
                "Reassign Channels",
                Command::AssignChannels,
                true,
            )],
            Mode::Audio => vec![
                MenuItem::new("Split Here", Command::SplitNote(id, t), true),
                MenuItem::new("Merge With Next", Command::MergeNotes(id), true),
            ],
            Mode::Midi | Mode::Drums => Vec::new(),
        };
        if let Some(first) = extra.first_mut() {
            first.group_break = true;
        }
        items.append(&mut extra);
    }

    items.push(MenuItem::new("Properties", Command::Properties, has_sel).group());
    items
}

//! Mouse modifiers: context × modifiers → action.
//!
//! Modelled directly on REAPER's MIDI-editor mouse-modifier system,
//! whose contexts and behaviour names are already decoded in-tree at
//! `features/reaper/reaper-input/src/input/mouse_modifiers/behaviors/
//! midi/`. Same shape, so a REAPER mouse map can be loaded into this
//! table without translation.
//!
//! The point of making this a *table* rather than a `match` in the
//! event handler is that the five products this editor serves want
//! different defaults — a drum editor wants paint-on-drag where a
//! Melodyne editor wants marquee — and users want to rebind. The
//! interaction layer resolves an [`Action`] and then executes it; it
//! never decides policy.

use crate::tools::Mods;

/// Where a press landed. REAPER calls these mouse-modifier contexts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Context {
    /// Empty canvas.
    PianoRoll,
    /// A note body.
    Note,
    /// Within the resize handle at either end of a note.
    NoteEdge,
    /// A Q-zone split handle.
    ZoneSplit,
    /// The velocity/CC lane strip below the roll.
    CcLane,
    /// An event already in a CC dimension.
    CcEvent,
    /// The piano-key gutter.
    Keys,
    /// The timeline ruler.
    Ruler,
    /// Inside an existing razor area.
    RazorArea,
    /// The left/right edge of a razor area.
    RazorEdge,
}

/// How the press arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Gesture {
    Click,
    DoubleClick,
    Drag,
    RightClick,
    MiddleClick,
}

/// What a resolved binding does.
///
/// The names track REAPER's decoded behaviour names so the two can be
/// mapped 1:1, with a few additions REAPER lacks (`EditLyric`,
/// `CycleArticulation`) that the guitar and vocal editors need.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    None,

    // ── selection ────────────────────────────────────────────────────
    SelectNote,
    ToggleNoteSelection,
    AddNoteToSelection,
    DeselectAll,
    SelectAllInMeasure,
    /// Select this note and every later one — the "fix everything from
    /// here" gesture.
    SelectNoteAndLater,
    /// Same, restricted to the note's own row.
    SelectNoteAndLaterSameRow,
    MarqueeSelect,
    MarqueeAdd,
    MarqueeToggle,
    /// Select whatever the pointer sweeps over, without a rectangle.
    SelectTouched,
    ToggleSelectTouched,

    // ── note creation ────────────────────────────────────────────────
    /// Insert and let the drag set length, snapped.
    InsertNoteDragToExtend,
    InsertNoteDragToExtendNoSnap,
    /// Insert and let the drag set pitch/position.
    InsertNoteDragToMove,
    /// Insert one note at the grid division and stop.
    InsertNote,
    InsertNoteNoSnap,
    /// Insert and let vertical drag set velocity.
    InsertNoteDragToEditVelocity,
    /// Fill the swept path with notes on the grid.
    PaintNotes,
    PaintNotesNoSnap,
    /// Repeat one row's note across the sweep.
    PaintRowOfNotes,

    // ── note editing ─────────────────────────────────────────────────
    MoveNote,
    MoveNoteNoSnap,
    /// Locks to whichever axis the drag committed to first.
    MoveNoteOneAxis,
    MoveNoteHorizontally,
    MoveNoteVertically,
    /// Ignore the rest of the selection and move only what was grabbed.
    MoveNoteIgnoringSelection,
    CopyNote,
    CopyNoteNoSnap,
    MoveNoteEdge,
    MoveNoteEdgeNoSnap,
    /// Scale every selected note's length proportionally.
    StretchNotes,
    /// Scale selected note *positions* — arpeggiate.
    StretchNotePositions,
    EditNoteVelocity,
    EditNoteVelocityFine,
    EraseNote,
    EraseNotes,
    ToggleNoteMute,
    SetNoteChannelHigher,
    SetNoteChannelLower,
    DoubleNoteLength,
    HalveNoteLength,

    // ── expression ───────────────────────────────────────────────────
    /// Whichever expression tool the toolbar has active.
    ActiveTool,
    /// Temporary freehand pen, from any tool.
    PenOverride,
    ScaleExpression,
    TransposeSnapped,
    /// Freehand into the active controller lane.
    EditCcEvents,
    /// A straight ramp between the drag's two ends, restyled by the
    /// toolbar shape. Stays live after release so the shape buttons can
    /// change their mind, exactly like the note-dimension `Curve` gesture.
    DrawCcLine,
    /// Splice the dimension's default back in across the swept range.
    EraseCcEvents,
    /// Ride the fader: scale the swept range about a pivot, keeping its
    /// shape and changing only its depth.
    ScaleCcEvents,

    // ── domain-specific ──────────────────────────────────────────────
    /// Vocal editor: type a syllable onto the note.
    EditLyric,
    /// Guitar/bass: step the articulation badge.
    CycleArticulation,
    /// Guitar/bass: same pitch, next playable string.
    CycleString,

    // ── razor edits ──────────────────────────────────────────────────
    /// Drag out a new rectangular area.
    RazorCreate,
    RazorAddArea,
    /// Move the area and everything in it.
    RazorMoveContents,
    RazorMoveContentsNoSnap,
    RazorCopyContents,
    /// Move the rectangle, leaving the notes where they are.
    RazorMoveAreaOnly,
    RazorMoveVertically,
    RazorMoveHorizontally,
    /// Drag an edge; contents stretch with it.
    RazorStretchContents,
    /// Drag an edge without touching the contents.
    RazorResizeArea,
    RazorDeleteContents,
    RazorRemoveArea,
    RazorClearAll,

    // ── navigation ───────────────────────────────────────────────────
    Pan,
    ZoomAnchored,
    MovePlayhead,
    MovePlayheadNoSnap,
    SelectRow,
    ContextMenu,
    /// Sound the note under the pointer without editing it.
    Audition,
}

impl Action {
    /// Actions that must snapshot history before the drag starts, so
    /// the whole gesture collapses into one undo step.
    pub fn is_edit(&self) -> bool {
        !matches!(
            self,
            Action::None
                | Action::SelectNote
                | Action::ToggleNoteSelection
                | Action::AddNoteToSelection
                | Action::DeselectAll
                | Action::SelectAllInMeasure
                | Action::SelectNoteAndLater
                | Action::SelectNoteAndLaterSameRow
                | Action::MarqueeSelect
                | Action::MarqueeAdd
                | Action::MarqueeToggle
                | Action::SelectTouched
                | Action::ToggleSelectTouched
                | Action::RazorCreate
                | Action::RazorAddArea
                | Action::RazorRemoveArea
                | Action::RazorClearAll
                | Action::Pan
                | Action::ZoomAnchored
                | Action::MovePlayhead
                | Action::MovePlayheadNoSnap
                | Action::SelectRow
                | Action::ContextMenu
                | Action::Audition
        )
    }

    /// Whether the action ignores the grid.
    pub fn ignores_snap(&self) -> bool {
        matches!(
            self,
            Action::InsertNoteDragToExtendNoSnap
                | Action::InsertNoteNoSnap
                | Action::MoveNoteNoSnap
                | Action::CopyNoteNoSnap
                | Action::MoveNoteEdgeNoSnap
                | Action::PaintNotesNoSnap
                | Action::MovePlayheadNoSnap
                | Action::RazorMoveContentsNoSnap
        )
    }
}

/// A modifier combination. Cmd is normalized to Ctrl upstream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModKey(u8);

impl ModKey {
    pub const NONE: ModKey = ModKey(0);
    pub const SHIFT: ModKey = ModKey(1);
    pub const CTRL: ModKey = ModKey(2);
    pub const ALT: ModKey = ModKey(4);

    pub fn of(m: Mods) -> Self {
        let mut bits = 0;
        if m.shift {
            bits |= 1;
        }
        if m.ctrl {
            bits |= 2;
        }
        if m.alt {
            bits |= 4;
        }
        ModKey(bits)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    /// From REAPER's mouse-modifier index bits (bit0=Shift, bit1=Ctrl,
    /// bit2=Alt) — the same layout this type uses, kept as an explicit
    /// constructor so a host loading `reaper-mouse.ini` states the
    /// correspondence instead of relying on it silently.
    pub const fn from_bits(bits: u8) -> Self {
        ModKey(bits & 0b111)
    }

    /// REAPER's `<S-C-A->` notation.
    pub fn notation(self) -> String {
        if self.0 == 0 {
            return String::new();
        }
        let mut s = String::from("<");
        if self.0 & 1 != 0 {
            s.push_str("S-");
        }
        if self.0 & 2 != 0 {
            s.push_str("C-");
        }
        if self.0 & 4 != 0 {
            s.push_str("A-");
        }
        s.push('>');
        s
    }
}

/// One binding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Binding {
    pub context: Context,
    pub gesture: Gesture,
    pub mods: ModKey,
    pub action: Action,
}

/// The full table.
#[derive(Clone, Debug, PartialEq)]
pub struct MouseMap {
    pub name: &'static str,
    bindings: Vec<Binding>,
}

impl Default for MouseMap {
    fn default() -> Self {
        host_overlay(Self::reaper_like())
    }
}

/// Host-supplied refinement of every default map.
///
/// The REAPER host registers a loader that overlays the user's own
/// mouse-modifier map (`reaper-mouse.ini`, the same table REAPER's
/// preferences edit) onto whatever preset a mode hands out — so a
/// binding the user set for REAPER's MIDI editor means the same thing
/// on this roll. Routed through both [`MouseMap::default`] and
/// [`crate::Mode::default_mouse`], so it survives a mode switch, which
/// rebuilds the map from the mode preset. Standalone hosts leave it
/// unset and get the presets untouched.
static HOST_OVERLAY: std::sync::OnceLock<fn(MouseMap) -> MouseMap> = std::sync::OnceLock::new();

/// Register the host overlay. First caller wins; later calls are
/// ignored, so a test harness cannot be trampled by a late host.
pub fn set_host_overlay(f: fn(MouseMap) -> MouseMap) {
    let _ = HOST_OVERLAY.set(f);
}

/// Run a preset through the host overlay, if one is registered.
pub fn host_overlay(map: MouseMap) -> MouseMap {
    match HOST_OVERLAY.get() {
        Some(f) => f(map),
        None => map,
    }
}

impl MouseMap {
    /// Resolve a press. Falls back to the unmodified binding for the
    /// same context and gesture, then to `None`.
    ///
    /// Falling back matters: without it, every unbound modifier
    /// combination becomes dead, and a user who happens to be holding
    /// Shift gets nothing instead of the obvious thing.
    pub fn resolve(&self, context: Context, gesture: Gesture, mods: Mods) -> Action {
        let key = ModKey::of(mods);
        self.find(context, gesture, key)
            .or_else(|| self.find(context, gesture, ModKey::NONE))
            .unwrap_or(Action::None)
    }

    fn find(&self, context: Context, gesture: Gesture, mods: ModKey) -> Option<Action> {
        self.bindings
            .iter()
            .find(|b| b.context == context && b.gesture == gesture && b.mods == mods)
            .map(|b| b.action)
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// Add or replace one binding.
    pub fn set(&mut self, context: Context, gesture: Gesture, mods: ModKey, action: Action) {
        match self
            .bindings
            .iter_mut()
            .find(|b| b.context == context && b.gesture == gesture && b.mods == mods)
        {
            Some(b) => b.action = action,
            None => self.bindings.push(Binding {
                context,
                gesture,
                mods,
                action,
            }),
        }
    }

    /// Every binding for one context, for a cheat-sheet UI.
    pub fn for_context(&self, context: Context) -> Vec<&Binding> {
        self.bindings
            .iter()
            .filter(|b| b.context == context)
            .collect()
    }

    /// Defaults that follow REAPER's MIDI editor.
    pub fn reaper_like() -> Self {
        use Action as A;
        use Context as C;
        use Gesture as G;
        const N: ModKey = ModKey::NONE;
        const S: ModKey = ModKey::SHIFT;
        const CT: ModKey = ModKey::CTRL;
        const AL: ModKey = ModKey::ALT;
        const SC: ModKey = ModKey(3);
        const CA: ModKey = ModKey(6);
        const SA: ModKey = ModKey(5);
        const ALL3: ModKey = ModKey(7);

        let b = |context, gesture, mods, action| Binding {
            context,
            gesture,
            mods,
            action,
        };

        Self {
            name: "REAPER-like",
            bindings: vec![
                // ── empty canvas ─────────────────────────────────────
                b(C::PianoRoll, G::Drag, N, A::MarqueeSelect),
                b(C::PianoRoll, G::Drag, S, A::MarqueeAdd),
                b(C::PianoRoll, G::Drag, CT, A::PenOverride),
                b(C::PianoRoll, G::Drag, AL, A::PaintNotes),
                // Shift+Alt belongs to razor creation; the no-snap
                // paint variant takes the remaining combination.
                b(C::PianoRoll, G::Drag, ALL3, A::PaintNotesNoSnap),
                b(C::PianoRoll, G::Drag, SC, A::Pan),
                b(C::PianoRoll, G::Drag, CA, A::ZoomAnchored),
                b(C::PianoRoll, G::Click, N, A::DeselectAll),
                b(C::PianoRoll, G::Click, AL, A::MovePlayhead),
                b(C::PianoRoll, G::DoubleClick, N, A::InsertNote),
                b(C::PianoRoll, G::DoubleClick, AL, A::InsertNoteNoSnap),
                b(C::PianoRoll, G::RightClick, N, A::ContextMenu),
                // Hand scroll, REAPER's own MIDI-editor middle-drag.
                b(C::PianoRoll, G::MiddleClick, N, A::Pan),
                b(C::PianoRoll, G::MiddleClick, S, A::MovePlayhead),
                // ── notes ────────────────────────────────────────────
                b(C::Note, G::Click, N, A::SelectNote),
                b(C::Note, G::Click, S, A::AddNoteToSelection),
                b(C::Note, G::Click, CT, A::ToggleNoteSelection),
                b(C::Note, G::Click, AL, A::ToggleNoteMute),
                b(C::Note, G::Click, SC, A::SelectNoteAndLater),
                b(C::Note, G::Click, CA, A::SelectNoteAndLaterSameRow),
                b(C::Note, G::DoubleClick, N, A::EraseNote),
                b(C::Note, G::Drag, N, A::MoveNote),
                b(C::Note, G::Drag, S, A::MoveNoteOneAxis),
                b(C::Note, G::Drag, CT, A::EditNoteVelocity),
                b(C::Note, G::Drag, AL, A::CopyNote),
                b(C::Note, G::Drag, SC, A::EditNoteVelocityFine),
                b(C::Note, G::Drag, CA, A::MoveNoteNoSnap),
                b(C::Note, G::RightClick, N, A::ContextMenu),
                // Hand scroll works from a note too — grabbing the roll
                // to pan must not depend on hitting empty canvas.
                b(C::Note, G::MiddleClick, N, A::Pan),
                b(C::Note, G::MiddleClick, S, A::Audition),
                // ── note edges ───────────────────────────────────────
                b(C::NoteEdge, G::Drag, N, A::MoveNoteEdge),
                b(C::NoteEdge, G::Drag, CT, A::MoveNoteEdgeNoSnap),
                b(C::NoteEdge, G::Drag, S, A::StretchNotes),
                b(C::NoteEdge, G::Drag, AL, A::StretchNotePositions),
                b(C::NoteEdge, G::Click, N, A::SelectNote),
                b(C::NoteEdge, G::DoubleClick, N, A::DoubleNoteLength),
                b(C::NoteEdge, G::DoubleClick, S, A::HalveNoteLength),
                // ── zones ────────────────────────────────────────────
                b(C::ZoneSplit, G::Drag, N, A::ActiveTool),
                b(C::ZoneSplit, G::Click, AL, A::None),
                b(C::ZoneSplit, G::RightClick, N, A::ContextMenu),
                // ── velocity / CC dimension ───────────────────────────────
                // Shift is deliberately unbound here: it is the
                // snap-reverse modifier for every CC gesture, the way it
                // is everywhere else in the surface. Binding it to a
                // tool would cost the only consistent "other behaviour"
                // key.
                b(C::CcLane, G::Drag, N, A::EditCcEvents),
                b(C::CcLane, G::Drag, AL, A::DrawCcLine),
                b(C::CcLane, G::Drag, CT, A::ScaleCcEvents),
                b(C::CcLane, G::Click, N, A::EditCcEvents),
                b(C::CcLane, G::RightClick, N, A::EraseCcEvents),
                b(C::CcEvent, G::Drag, N, A::EditCcEvents),
                b(C::CcEvent, G::Drag, AL, A::DrawCcLine),
                b(C::CcEvent, G::Drag, CT, A::ScaleCcEvents),
                b(C::CcEvent, G::Click, N, A::SelectNote),
                b(C::CcEvent, G::RightClick, N, A::EraseCcEvents),
                // ── chrome ───────────────────────────────────────────
                b(C::Keys, G::Click, N, A::SelectRow),
                b(C::Keys, G::Click, AL, A::Audition),
                b(C::Ruler, G::Click, N, A::MovePlayhead),
                b(C::Ruler, G::Click, AL, A::MovePlayheadNoSnap),
                b(C::Ruler, G::Drag, N, A::MovePlayhead),
                // ── razor ────────────────────────────────────────────
                // Creating a razor is a deliberate modifier gesture, so
                // it never steals a plain drag from marquee select.
                b(C::PianoRoll, G::Drag, SA, A::RazorCreate),
                b(C::RazorArea, G::Drag, N, A::RazorMoveContents),
                b(C::RazorArea, G::Drag, CT, A::RazorMoveContentsNoSnap),
                b(C::RazorArea, G::Drag, AL, A::RazorCopyContents),
                b(C::RazorArea, G::Drag, S, A::RazorMoveAreaOnly),
                b(C::RazorArea, G::Click, N, A::RazorRemoveArea),
                b(C::RazorArea, G::Click, S, A::RazorAddArea),
                b(C::RazorArea, G::DoubleClick, N, A::RazorDeleteContents),
                b(C::RazorArea, G::RightClick, N, A::ContextMenu),
                b(C::RazorEdge, G::Drag, N, A::RazorResizeArea),
                b(C::RazorEdge, G::Drag, S, A::RazorStretchContents),
            ],
        }
    }

    /// Drum-editor defaults.
    ///
    /// Painting is the primary gesture — a drum part is built by
    /// sweeping hits onto the grid, not by drawing one note at a time —
    /// and length editing is removed, since a diamond has none.
    pub fn drums() -> Self {
        use Action as A;
        use Context as C;
        use Gesture as G;
        let mut m = Self::reaper_like();
        m.name = "Drums";
        m.set(C::PianoRoll, G::Drag, ModKey::NONE, A::PaintNotes);
        m.set(C::PianoRoll, G::Click, ModKey::NONE, A::InsertNote);
        m.set(C::PianoRoll, G::Drag, ModKey::SHIFT, A::MarqueeSelect);
        m.set(C::Note, G::Drag, ModKey::NONE, A::EditNoteVelocity);
        m.set(C::Note, G::Drag, ModKey::SHIFT, A::MoveNote);
        m.set(C::Note, G::Click, ModKey::NONE, A::SelectNote);
        m.set(C::NoteEdge, G::Drag, ModKey::NONE, A::EditNoteVelocity);
        m
    }

    /// Guitar/bass defaults, following Ample's Riffer key commands.
    ///
    /// Riffer's set is deliberately terse and differs from REAPER's:
    /// plain click inserts, double-click deletes, Ctrl+vertical-drag is
    /// velocity, and Shift+drag moves. Matching it means someone coming
    /// from Riffer is not retrained.
    pub fn riffer() -> Self {
        use Action as A;
        use Context as C;
        use Gesture as G;
        let mut m = Self::reaper_like();
        m.name = "Riffer (Ample)";
        m.set(C::PianoRoll, G::Click, ModKey::NONE, A::InsertNote);
        m.set(
            C::PianoRoll,
            G::Drag,
            ModKey::NONE,
            A::InsertNoteDragToExtend,
        );
        m.set(C::Note, G::Click, ModKey::NONE, A::SelectNote);
        m.set(C::Note, G::DoubleClick, ModKey::NONE, A::EraseNote);
        m.set(C::Note, G::Drag, ModKey::NONE, A::MoveNoteVertically);
        m.set(C::Note, G::Drag, ModKey::CTRL, A::EditNoteVelocity);
        m.set(C::Note, G::Drag, ModKey::SHIFT, A::MoveNote);
        m.set(C::Note, G::Click, ModKey::ALT, A::ContextMenu);
        m.set(C::NoteEdge, G::Drag, ModKey::NONE, A::MoveNoteEdge);
        m.set(C::NoteEdge, G::Drag, ModKey::CTRL, A::MoveNoteEdgeNoSnap);
        m.set(C::Keys, G::Click, ModKey::NONE, A::SelectRow);
        m
    }

    /// Vocal-lyric defaults: double-clicking a note types on it.
    pub fn lyrics() -> Self {
        use Action as A;
        use Context as C;
        use Gesture as G;
        let mut m = Self::reaper_like();
        m.name = "Lyrics";
        m.set(C::Note, G::DoubleClick, ModKey::NONE, A::EditLyric);
        m
    }

    pub const PRESETS: [&'static str; 4] = ["REAPER-like", "Drums", "Riffer (Ample)", "Lyrics"];

    pub fn preset(name: &str) -> Self {
        match name {
            "Drums" => Self::drums(),
            "Riffer (Ample)" => Self::riffer(),
            "Lyrics" => Self::lyrics(),
            _ => Self::reaper_like(),
        }
    }
}

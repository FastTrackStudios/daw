//! Editor state that travels with the project.
//!
//! The rule that decides what belongs here is one question: *would a
//! collaborator opening this project want this, exactly as I left it?*
//! Yes, it lives here. No, but you want it in every project, it is a
//! preference (`utils::prefs`). No to both, it is ephemeral and is never
//! written at all.
//!
//! Scoped to mode corrections for now; the rest of the per-take bucket
//! is #192 and the lane layout is #201, both of which extend this type
//! rather than adding a second one.
//!
//! ## Only corrections are written
//!
//! Inference is never persisted. That keeps the stored table small,
//! makes every row a decision a human made — so it stays readable inside
//! a `.daw`, which #155 requires — and means improving the inference
//! heuristic later automatically benefits every take except the ones
//! somebody explicitly overrode.
//!
//! One deliberate exception: a correction that happens to match what
//! inference would have picked is still written. "The user affirmed
//! this" is worth pinning against a future heuristic change.
//!
//! ## Versioning
//!
//! A `version` at the root, tolerant parsing underneath, and **no
//! migration chain**. An unrecognised version discards the blob and uses
//! defaults — never a refusal to open, never a half-read. Editor state
//! is derivable and cheap to lose: a dropped lane layout costs a
//! re-layout, a dropped mode override costs one click. That asymmetry is
//! what makes defaults the correct failure rather than a lazy one, and
//! it is why a migration framework would be dead weight across every
//! shape change still ahead of this map.
//!
//! The number exists for the one change tolerant parsing cannot survive:
//! a field that keeps its name and changes its meaning.

use daw::service::ExtState;
use daw::service::ProjectContext;
use daw::service::ext_state::side_store;
use expression_editor_core::Mode;
use facet::Facet;

/// The namespace the expression editor stores under.
pub const NAMESPACE: &str = "expression-editor";

/// Bumped only when a field keeps its name and changes its meaning.
pub const CURRENT_VERSION: u32 = 1;

/// A mode somebody corrected, against the guid it was corrected on.
#[derive(Debug, Clone, PartialEq, Facet)]
pub struct ModeOverride {
    /// A track guid or a take guid. Which one it is follows from the
    /// list it appears in.
    pub guid: String,
    pub mode: Mode,
}

/// Everything about a project that the editor persists.
#[derive(Debug, Clone, PartialEq, Facet)]
pub struct EditorState {
    pub version: u32,
    /// Track-level corrections: the default for takes on that track,
    /// including takes recorded later.
    pub track_modes: Vec<ModeOverride>,
    /// Take-level corrections, which beat the track's.
    pub take_modes: Vec<ModeOverride>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            track_modes: Vec::new(),
            take_modes: Vec::new(),
        }
    }
}

impl EditorState {
    /// Resolve a mode: **take → track → `None`**.
    ///
    /// `None` means nobody has corrected this, so the caller infers. The
    /// order is what makes a track-level correction apply to takes
    /// recorded later while still letting one odd comp disagree.
    pub fn mode_for(&self, track_guid: &str, take_guid: &str) -> Option<Mode> {
        self.take_modes
            .iter()
            .find(|o| o.guid == take_guid)
            .or_else(|| self.track_modes.iter().find(|o| o.guid == track_guid))
            .map(|o| o.mode)
    }

    /// Record a correction on a take.
    pub fn correct_take(&mut self, take_guid: impl Into<String>, mode: Mode) {
        upsert(&mut self.take_modes, take_guid.into(), mode);
    }

    /// Record a correction on a track, which becomes the default for its
    /// takes.
    pub fn correct_track(&mut self, track_guid: impl Into<String>, mode: Mode) {
        upsert(&mut self.track_modes, track_guid.into(), mode);
    }

    /// Forget a take-level correction, falling back to the track's.
    pub fn clear_take(&mut self, take_guid: &str) {
        self.take_modes.retain(|o| o.guid != take_guid);
    }

    /// Whether anything here is worth writing.
    pub fn is_empty(&self) -> bool {
        self.track_modes.is_empty() && self.take_modes.is_empty()
    }
}

fn upsert(list: &mut Vec<ModeOverride>, guid: String, mode: Mode) {
    match list.iter_mut().find(|o| o.guid == guid) {
        Some(existing) => existing.mode = mode,
        None => list.push(ModeOverride { guid, mode }),
    }
}

/// Read the editor's state for a project.
///
/// Never fails in a way the caller has to handle: unset, unparseable, or
/// written by a version this build does not understand all yield
/// defaults. Losing editor state costs a re-layout and a click; refusing
/// to open a project costs the session.
pub fn load<D: ExtState + ?Sized>(daw: &D, project: ProjectContext) -> EditorState {
    let Some(raw) = side_store::load(daw, project, NAMESPACE) else {
        return EditorState::default();
    };
    match facet_styx::from_str::<EditorState>(&raw) {
        Ok(state) if state.version <= CURRENT_VERSION => state,
        // A newer version is discarded whole rather than half-read: a
        // field whose meaning changed would otherwise be misread as if
        // it had not.
        _ => EditorState::default(),
    }
}

/// Write the editor's state for a project.
///
/// Marks the project dirty, which is the point: a mode correction that
/// is a project's only change must still prompt to save, or it is
/// silently lost on close — the complaint #157 opened with.
pub fn save<D: ExtState + ?Sized>(
    daw: &D,
    project: ProjectContext,
    state: &EditorState,
) -> daw::service::DawResult<()> {
    let text = facet_styx::to_string(state).unwrap_or_default();
    side_store::store(daw, project, NAMESPACE, &text)
}

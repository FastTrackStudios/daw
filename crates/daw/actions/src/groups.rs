//! Track-group manager actions.
//!
//! Registered under the `fts.session.*` namespace via the `session_actions`
//! `define_actions!` block in `crate::lib`, dispatched from
//! `daw_module`'s `action_for_id` chain. All work runs on REAPER's main
//! thread (the action-callback context), delegating to [`crate::group_manager`].

use crate::group_manager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupAction {
    /// Write the instrument-category partition into the 128 group names.
    ApplyNaming,
    /// Assign the selected tracks to the next free slot in a category band.
    AssignSelected(&'static str),
}

pub fn dispatch(action: GroupAction) {
    match action {
        GroupAction::ApplyNaming => {
            group_manager::apply_group_naming();
        }
        GroupAction::AssignSelected(category) => {
            group_manager::assign_selected_to_category(category);
        }
    }
}

// ── architect::actions implementation ───────────────────────────────────
//
// The action enum + `dispatch` below stay: they are the shared body every
// action method (and, where one exists, the RPC service impl) calls into.
// What's gone is the string-keyed `action_for_id` lookup and the
// `session_actions` `define_actions!` entries that declared the same
// `FTS_SESSION_*` command ids a second time.

/// Bridges the nine track-group-manager actions onto
/// `#[architect::actions]`. Every method forwards to the existing
/// synchronous `dispatch` — no behavior change, just a declarative front
/// door with real metadata.
pub struct GroupActionsImpl;

#[architect::actions(namespace = "FTS_SESSION")]
pub trait GroupActions {
    #[action(
        description = "Name the project's 128 track groups by the FTS instrument partition (Drums 1-10, Bass 11-20, Electric Gtr 21-40, Acoustic Gtr 41-60, Keys 61-70, Synths 71-80, Lead Vocal 81-100, Background Vox 101-120, Spare 121-128).",
        category = "Tracks",
        group = "Track Groups"
    )]
    fn group_apply_naming(&self);
    #[action(
        description = "Add the selected tracks to the next free Drums group slot as a mutual group (all flag families).",
        category = "Tracks",
        group = "Track Groups"
    )]
    fn group_assign_drums(&self);
    #[action(
        description = "Add the selected tracks to the next free Bass group slot as a mutual group.",
        category = "Tracks",
        group = "Track Groups"
    )]
    fn group_assign_bass(&self);
    #[action(
        description = "Add the selected tracks to the next free Electric Gtr group slot as a mutual group.",
        category = "Tracks",
        group = "Track Groups"
    )]
    fn group_assign_electric_gtr(&self);
    #[action(
        description = "Add the selected tracks to the next free Acoustic Gtr group slot as a mutual group.",
        category = "Tracks",
        group = "Track Groups"
    )]
    fn group_assign_acoustic_gtr(&self);
    #[action(
        description = "Add the selected tracks to the next free Keys group slot as a mutual group.",
        category = "Tracks",
        group = "Track Groups"
    )]
    fn group_assign_keys(&self);
    #[action(
        description = "Add the selected tracks to the next free Synths group slot as a mutual group.",
        category = "Tracks",
        group = "Track Groups"
    )]
    fn group_assign_synths(&self);
    #[action(
        description = "Add the selected tracks to the next free Lead Vocal group slot as a mutual group.",
        category = "Tracks",
        group = "Track Groups"
    )]
    fn group_assign_lead_vocal(&self);
    #[action(
        description = "Add the selected tracks to the next free Background Vox group slot as a mutual group.",
        category = "Tracks",
        group = "Track Groups"
    )]
    fn group_assign_background_vox(&self);
}

impl GroupActions for GroupActionsImpl {
    fn group_apply_naming(&self) {
        dispatch(GroupAction::ApplyNaming);
    }
    fn group_assign_drums(&self) {
        dispatch(GroupAction::AssignSelected("Drums"));
    }
    fn group_assign_bass(&self) {
        dispatch(GroupAction::AssignSelected("Bass"));
    }
    fn group_assign_electric_gtr(&self) {
        dispatch(GroupAction::AssignSelected("Electric Gtr"));
    }
    fn group_assign_acoustic_gtr(&self) {
        dispatch(GroupAction::AssignSelected("Acoustic Gtr"));
    }
    fn group_assign_keys(&self) {
        dispatch(GroupAction::AssignSelected("Keys"));
    }
    fn group_assign_synths(&self) {
        dispatch(GroupAction::AssignSelected("Synths"));
    }
    fn group_assign_lead_vocal(&self) {
        dispatch(GroupAction::AssignSelected("Lead Vocal"));
    }
    fn group_assign_background_vox(&self) {
        dispatch(GroupAction::AssignSelected("Background Vox"));
    }
}

/// Registers all nine track-group-manager actions with `backend`.
pub fn register_actions<B>(backend: &B)
where
    B: ::architect::action::ActionBackend + ?Sized,
{
    register_group_actions(backend, std::sync::Arc::new(GroupActionsImpl));
}

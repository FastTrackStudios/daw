//! Typed action ID constants and the [`StaticActionId`] wrapper.
//!
//! Each domain crate declares its own action IDs via [`define_actions!`](crate::define_actions).
//! This module provides `StaticActionId` (the const-constructible wrapper) and
//! constants for actions owned by `actions-proto` itself (e.g., standalone app actions).
//!
//! ```ignore
//! // Domain crate declares its own action IDs:
//! // session::session_actions::TOGGLE_PLAYBACK
//!
//! // Standalone actions are defined here:
//! use actions_proto::ids::standalone;
//! standalone::TOGGLE_DARK_MODE.to_id()  // ActionId
//! ```

use crate::ActionId;

/// A compile-time action identifier backed by a `&'static str`.
///
/// Use `.to_id()` to convert to an owned `ActionId` for dispatch,
/// or `.as_str()` to get the raw string for matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StaticActionId(pub &'static str);

impl StaticActionId {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(&self) -> &'static str {
        self.0
    }

    pub fn to_id(&self) -> ActionId {
        ActionId::new(self.0)
    }

    /// Convert to REAPER command ID format (e.g., "FTS_SESSION_TOGGLE_PLAYBACK").
    pub fn to_command_id(&self) -> String {
        self.to_id().to_command_id()
    }
}

impl From<StaticActionId> for ActionId {
    fn from(s: StaticActionId) -> Self {
        ActionId::new(s.0)
    }
}

impl PartialEq<ActionId> for StaticActionId {
    fn eq(&self, other: &ActionId) -> bool {
        self.0 == other.as_str()
    }
}

impl PartialEq<StaticActionId> for ActionId {
    fn eq(&self, other: &StaticActionId) -> bool {
        self.as_str() == other.0
    }
}

impl std::fmt::Display for StaticActionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Standalone app actions (settings, dark mode, command palette).
pub mod standalone {
    use super::StaticActionId;

    pub const OPEN_SETTINGS: StaticActionId = StaticActionId::new("fts.standalone.open_settings");
    pub const TOGGLE_DARK_MODE: StaticActionId =
        StaticActionId::new("fts.standalone.toggle_dark_mode");
    pub const COMMAND_PALETTE: StaticActionId =
        StaticActionId::new("fts.standalone.command_palette");
    pub const SHOW_ABOUT: StaticActionId = StaticActionId::new("fts.standalone.show_about");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ActionId;

    #[test]
    fn static_action_id_new() {
        let id = StaticActionId::new("fts.session.toggle_playback");
        assert_eq!(id.0, "fts.session.toggle_playback");
    }

    #[test]
    fn static_action_id_as_str() {
        let id = StaticActionId::new("fts.session.toggle_playback");
        assert_eq!(id.as_str(), "fts.session.toggle_playback");
    }

    #[test]
    fn static_action_id_to_id() {
        let static_id = StaticActionId::new("fts.session.toggle_playback");
        let action_id = static_id.to_id();
        assert_eq!(action_id.as_str(), "fts.session.toggle_playback");
    }

    #[test]
    fn static_action_id_to_command_id() {
        let id = StaticActionId::new("fts.session.toggle_playback");
        assert_eq!(id.to_command_id(), "FTS_SESSION_TOGGLE_PLAYBACK");
    }

    #[test]
    fn static_action_id_display() {
        let id = StaticActionId::new("fts.standalone.open_settings");
        assert_eq!(format!("{}", id), "fts.standalone.open_settings");
        assert_eq!(id.to_string(), "fts.standalone.open_settings");
    }

    #[test]
    fn static_action_id_eq_action_id() {
        let static_id = StaticActionId::new("fts.session.play");
        let action_id = ActionId::new("fts.session.play");
        assert_eq!(static_id, action_id);
    }

    #[test]
    fn action_id_eq_static_action_id() {
        let static_id = StaticActionId::new("fts.session.play");
        let action_id = ActionId::new("fts.session.play");
        assert_eq!(action_id, static_id);
    }

    #[test]
    fn partial_eq_mismatch() {
        let static_id = StaticActionId::new("fts.session.play");
        let action_id = ActionId::new("fts.session.stop");
        assert_ne!(static_id, action_id);
        assert_ne!(action_id, static_id);
    }

    #[test]
    fn from_static_to_action_id() {
        let static_id = StaticActionId::new("fts.transport.stop");
        let action_id: ActionId = static_id.into();
        assert_eq!(action_id.as_str(), "fts.transport.stop");
    }

    #[test]
    fn standalone_constants_exist() {
        assert_eq!(
            standalone::OPEN_SETTINGS.as_str(),
            "fts.standalone.open_settings"
        );
        assert_eq!(
            standalone::TOGGLE_DARK_MODE.as_str(),
            "fts.standalone.toggle_dark_mode"
        );
        assert_eq!(
            standalone::COMMAND_PALETTE.as_str(),
            "fts.standalone.command_palette"
        );
        assert_eq!(standalone::SHOW_ABOUT.as_str(), "fts.standalone.show_about");
    }
}

//! Take ranking — set a take's rank marker (`:)`, `:))`, `:)))`, `:(`)
//! to a specific level in one shot, at a chosen scope.
//!
//! Contract only. The implementation drives a live backend and lives in
//! `daw-actions` (`daw_actions::take_ranking`), which is also where the
//! internal `Scope` / `RankAction` enums live — the types here are the
//! wire-side mirror.

use facet::Facet;

/// Scope for [`TakeRankingService::apply_rank`]. Wire-side mirror of
/// the `Scope` enum in `daw_actions::take_ranking`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
pub enum TakeRankScope {
    /// Active take of each selected item, marker at `(play_pos - 2s)`
    /// when playing or at edit cursor when stopped.
    PlayPosMinus2s,
    /// Active take of each selected item, marker at item start.
    ItemWide,
    /// Take under the mouse cursor, marker at mouse project-time.
    MouseCursor,
}

/// Rank level: 1..=3 stars (up-rank) or `Down`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
pub enum TakeRankLevel {
    One,
    Two,
    Three,
    Down,
}

pub mod take_ranking_service {
    use super::{TakeRankLevel, TakeRankScope};
    use crate::DawError;

    #[architect::rpc]
    pub trait TakeRankingService {
        /// Apply `level` at `scope`. Behavior matches the
        /// `daw_actions::take_ranking::apply` semantics: replace-if-near
        /// for position-based scopes, single marker per take for
        /// item-wide.
        async fn apply_rank(
            &self,
            scope: TakeRankScope,
            level: TakeRankLevel,
        ) -> Result<(), DawError>;
    }
}

#[cfg(feature = "vox")]
pub use take_ranking_service::TakeRankingServiceClient;
pub use take_ranking_service::{
    Service as TakeRankingServiceLayer, TakeRankingService, TakeRankingServiceDispatcher,
    layer as take_ranking_service_layer, serve as serve_take_ranking_service,
    take_ranking_service_rpc_service_descriptor, take_ranking_service_service_descriptor,
};

//! A track's automate menu, as the engine reads it. Its envelopes and
//! their actions are the engine's (`session_engine::automation_actions`).

use facet::Facet;

/// A track's automate menu, as the engine reads it.
#[derive(Facet, Clone, Debug, Default)]
pub struct MenuFacts {
    pub shown: Vec<daw_proto::automation::EnvelopeRef>,
    pub takes: bool,
    /// `Some(shown)` when the track has the leveler's lanes.
    pub leveler: Option<bool>,
    pub mode: Option<daw_proto::primitives::AutomationMode>,
    /// Each plugin: its guid and name, in chain order.
    pub fxs: Vec<(String, String)>,
    /// The parameters of the plugin asked for: index and name.
    pub params: Vec<(u32, String)>,
}

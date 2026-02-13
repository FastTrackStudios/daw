//! DAW action grouping roots.

actions_proto::define_actions! {
    /// Group root for DAW-scoped action sets.
    pub daw_action_groups {
        prefix: "fts.daw",
        title: "DAW",
    }
}

pub const GROUP_TRANSPORT: &str = "Transport";

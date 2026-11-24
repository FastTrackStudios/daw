//! Where opening a set has got to — the engine's word for it, which every
//! loading screen (`crate::loading`) shows.

/// Where opening a set has got to: what its loading screen says.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, facet::Facet)]
pub enum Progress {
    /// Reaching Task and joining the set — `retry` says why the last try
    /// failed, while it keeps trying.
    Joining { retry: Option<String> },
    /// Bringing the first song's files in.
    Fetching {
        title: String,
        retry: Option<String>,
    },
    /// Opening it into the engine.
    Opening { title: String },
}

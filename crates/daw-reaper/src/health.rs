//! REAPER Health Service Implementation
//!
//! Trivial implementation — always returns `true`. The value of the ping is
//! in the RPC round-trip succeeding, not in the response payload.

use daw_proto::HealthService;

/// REAPER health-check implementation.
#[derive(Clone)]
pub struct ReaperHealth;

impl ReaperHealth {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperHealth {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthService for ReaperHealth {
    async fn ping(&self) -> bool {
        true
    }
}

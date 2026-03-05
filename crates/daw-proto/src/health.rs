//! Health Service
//!
//! Minimal service for connection liveness checks. Returns `true` immediately
//! with no side effects — the cheapest possible RPC round-trip.

use roam::service;

/// Lightweight health-check service for connection liveness probing.
#[service]
pub trait HealthService {
    /// Returns `true` if the DAW is reachable. Used by fts-control's
    /// health-check loop to detect disconnects faster than process polling.
    async fn ping(&self) -> bool;
}

//! mDNS service discovery for vox RPC services.
//!
//! Provides zero-config peer discovery on local networks using DNS-SD (mDNS).
//! Any vox service can advertise itself and discover peers without manual
//! configuration — just agree on a service type string.
//!
//! Uses the system's native mDNS implementation (avahi on Linux, Bonjour on macOS)
//! via the `zeroconf` crate.
//!
//! # Advertising
//!
//! ```rust,no_run
//! use vox_discover::{advertise, ServiceInfo};
//!
//! # async fn example() -> vox_discover::Result<()> {
//! let _guard = advertise(ServiceInfo {
//!     service_type: "fts-daw",
//!     instance_name: "Studio A - Tracks".into(),
//!     port: 9451,
//!     metadata: vec![
//!         ("role".into(), "tracks".into()),
//!         ("session".into(), "my-setlist".into()),
//!     ],
//! })?;
//! // Service is advertised until _guard is dropped
//! # Ok(())
//! # }
//! ```
//!
//! # Discovery
//!
//! ```rust,no_run
//! use vox_discover::discover;
//!
//! # async fn example() {
//! let mut rx = discover("fts-daw").await;
//! while let Some(event) = rx.recv().await {
//!     match event {
//!         vox_discover::PeerEvent::Found(peer) => {
//!             println!("Found: {} at {}:{}", peer.instance_name, peer.host, peer.port);
//!             // Connect: vox::connect_tcp((peer.host, peer.port))
//!         }
//!         vox_discover::PeerEvent::Lost(name) => {
//!             println!("Lost: {}", name);
//!         }
//!     }
//! }
//! # }
//! ```
//!
//! # DNS-SD Convention
//!
//! Service types are mapped to DNS-SD format:
//! - `"fts-daw"` → `_fts-daw._tcp.local.`
//! - `"fts-control"` → `_fts-control._tcp.local.`
//!
//! Metadata is stored as TXT records (key=value pairs, per RFC 6763).

mod advertise;
mod discover;
mod types;

pub use advertise::{advertise, AdvertiseGuard};
pub use discover::discover;
pub use types::*;

/// Result type for vox-discover operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error type for vox-discover operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("zeroconf error: {0}")]
    Zeroconf(#[from] zeroconf::error::Error),
    #[error("invalid service type: {0}")]
    InvalidServiceType(String),
}

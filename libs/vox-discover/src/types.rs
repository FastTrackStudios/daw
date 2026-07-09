//! Shared types for service advertisement and discovery.

use std::collections::HashMap;
use std::net::IpAddr;

/// Information needed to advertise a vox service on the network.
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    /// Short service type name (e.g., "fts-daw", "fts-control").
    /// Mapped to DNS-SD as `_<service_type>._tcp.local.`
    pub service_type: &'static str,

    /// Human-readable instance name (e.g., "Studio A - Tracks").
    /// Must be unique on the local network.
    pub instance_name: String,

    /// TCP port the vox service is listening on.
    pub port: u16,

    /// Key-value metadata stored as DNS-SD TXT records.
    /// Common keys: "role", "session", "version", "pid"
    pub metadata: Vec<(String, String)>,
}

/// A discovered peer on the network.
#[derive(Debug, Clone)]
pub struct Peer {
    /// The instance name from the mDNS advertisement.
    pub instance_name: String,

    /// The full service type (e.g., "_fts-daw._tcp.local.")
    pub service_type: String,

    /// Hostname or IP address.
    pub host: String,

    /// IP addresses resolved for this peer.
    pub addresses: Vec<IpAddr>,

    /// TCP port.
    pub port: u16,

    /// Metadata from TXT records.
    pub metadata: HashMap<String, String>,
}

impl Peer {
    /// Get a metadata value by key.
    pub fn get_meta(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(|s| s.as_str())
    }

    /// Get the first usable socket address (prefers IPv4).
    pub fn addr(&self) -> Option<std::net::SocketAddr> {
        // Prefer IPv4
        let ip = self
            .addresses
            .iter()
            .find(|a| a.is_ipv4())
            .or_else(|| self.addresses.first())?;
        Some(std::net::SocketAddr::new(*ip, self.port))
    }
}

/// Events emitted by the discovery stream.
#[derive(Debug, Clone)]
pub enum PeerEvent {
    /// A new peer was found on the network.
    Found(Peer),
    /// A previously found peer disappeared.
    Lost(String),
}

/// Convert a short service type to the full DNS-SD service type string.
///
/// `"fts-daw"` → `"_fts-daw._tcp.local."`
pub fn to_dns_sd_type(service_type: &str) -> String {
    format!("_{service_type}._tcp.local.")
}

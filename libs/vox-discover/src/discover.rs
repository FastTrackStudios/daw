//! Peer discovery via mDNS browsing using the system's native implementation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use zeroconf::prelude::*;
use zeroconf::ServiceType;

use crate::types::{Peer, PeerEvent};

/// Start discovering vox services of the given type on the local network.
///
/// Returns a receiver that yields [`PeerEvent`]s as peers appear and disappear.
/// Discovery runs in a background thread and forwards events through a tokio channel.
///
/// Note: The `zeroconf` crate only provides discovery callbacks (not removal).
/// Peer loss detection must be handled at a higher layer (e.g., TCP connection drop).
///
/// The discovery stops when the returned receiver is dropped.
pub async fn discover(service_type: &str) -> mpsc::Receiver<PeerEvent> {
    let (tx, rx) = mpsc::channel(64);
    let service_type_owned = service_type.to_string();

    let zc_service_type = match ServiceType::new(service_type, "tcp") {
        Ok(st) => st,
        Err(e) => {
            warn!("Failed to create service type for discovery: {e}");
            return rx;
        }
    };

    let dns_sd = format!("_{service_type}._tcp.local.");

    // MdnsBrowser must stay alive on the thread that polls its event loop —
    // dropping it destroys the avahi client connection.
    std::thread::Builder::new()
        .name(format!("mdns-discover-{service_type}"))
        .spawn(move || {
            let mut browser = zeroconf::MdnsBrowser::new(zc_service_type);

            let tx = Arc::new(tx);
            let tx_cb = tx.clone();

            browser.set_service_discovered_callback(Box::new(move |result, _ctx| {
                match result {
                    Ok(discovery) => {
                        let mut metadata = HashMap::new();
                        if let Some(txt) = discovery.txt() {
                            for (key, value) in txt.iter() {
                                metadata.insert(key, value);
                            }
                        }

                        let addresses = discovery.address()
                            .parse()
                            .ok()
                            .into_iter()
                            .collect();

                        let peer = Peer {
                            instance_name: discovery.name().to_string(),
                            service_type: format!(
                                "_{}._{}.local.",
                                discovery.service_type().name(),
                                discovery.service_type().protocol()
                            ),
                            host: discovery.host_name().to_string(),
                            addresses,
                            port: *discovery.port(),
                            metadata,
                        };

                        debug!(
                            instance = %peer.instance_name,
                            host = %peer.host,
                            port = peer.port,
                            addrs = ?peer.addresses,
                            "Peer found"
                        );

                        if tx_cb.blocking_send(PeerEvent::Found(peer)).is_err() {
                            debug!("Discovery receiver dropped");
                        }
                    }
                    Err(e) => {
                        warn!("mDNS discovery callback error: {e}");
                    }
                }
            }));

            info!(
                service_type = %service_type_owned,
                dns_sd = %dns_sd,
                "mDNS discovery started"
            );

            let event_loop = match browser.browse_services() {
                Ok(el) => el,
                Err(e) => {
                    warn!("Failed to start mDNS browse: {e}");
                    return;
                }
            };

            loop {
                if tx.is_closed() {
                    debug!("Discovery receiver dropped, stopping browse");
                    break;
                }

                if let Err(e) = event_loop.poll(Duration::from_millis(100)) {
                    debug!("mDNS browse poll error: {e}");
                    break;
                }
            }

            debug!("mDNS discovery loop exiting");
        })
        .expect("failed to spawn mDNS discovery thread");

    rx
}

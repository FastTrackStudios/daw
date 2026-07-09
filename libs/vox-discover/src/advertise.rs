//! Service advertisement via mDNS using the system's native implementation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, info, warn};
use zeroconf::prelude::*;
use zeroconf::{MdnsService, ServiceType, TxtRecord};

use crate::types::ServiceInfo;
use crate::Result;

/// Advertise a vox service on the local network via mDNS.
///
/// Returns an [`AdvertiseGuard`] that keeps the advertisement alive.
/// When dropped, the service is unregistered from the network.
pub fn advertise(info: ServiceInfo) -> Result<AdvertiseGuard> {
    let service_type = ServiceType::new(info.service_type, "tcp")
        .map_err(crate::Error::Zeroconf)?;

    let port = info.port;
    let svc_type_str = info.service_type;
    let instance_name = info.instance_name.clone();
    let metadata = info.metadata.clone();

    // Run the event loop on a background thread — the MdnsService must stay
    // alive for the duration, so we move it into the thread.
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    std::thread::Builder::new()
        .name(format!("mdns-advertise-{svc_type_str}"))
        .spawn(move || {
            let mut mdns_service = MdnsService::new(service_type, port);
            mdns_service.set_name(&instance_name);

            if !metadata.is_empty() {
                let mut txt = TxtRecord::new();
                for (key, value) in &metadata {
                    if let Err(e) = txt.insert(key, value) {
                        warn!("Failed to set TXT record {key}: {e}");
                    }
                }
                mdns_service.set_txt_record(txt);
            }

            let inst = instance_name.clone();
            mdns_service.set_registered_callback(Box::new(move |result, _ctx| {
                match result {
                    Ok(registration) => {
                        info!(
                            service_type = svc_type_str,
                            instance = %inst,
                            port = port,
                            name = %registration.name(),
                            "mDNS service registered via system daemon"
                        );
                    }
                    Err(e) => {
                        warn!("mDNS registration callback error: {e}");
                    }
                }
            }));

            let event_loop = match mdns_service.register() {
                Ok(el) => el,
                Err(e) => {
                    warn!("mDNS register failed: {e}");
                    return;
                }
            };

            info!(
                service_type = svc_type_str,
                instance = %instance_name,
                port = port,
                "mDNS service advertised"
            );

            while running_clone.load(Ordering::Relaxed) {
                if let Err(e) = event_loop.poll(Duration::from_millis(100)) {
                    debug!("mDNS advertise poll error: {e}");
                    break;
                }
            }
            debug!("mDNS advertise thread exiting");
        })
        .expect("failed to spawn mDNS advertise thread");

    Ok(AdvertiseGuard {
        running,
        service_type: info.service_type,
        instance_name: info.instance_name,
    })
}

/// RAII guard that keeps a service advertised on the network.
///
/// Stops the advertisement when dropped.
pub struct AdvertiseGuard {
    running: Arc<AtomicBool>,
    service_type: &'static str,
    instance_name: String,
}

impl Drop for AdvertiseGuard {
    fn drop(&mut self) {
        debug!(
            service_type = self.service_type,
            instance = %self.instance_name,
            "Stopping mDNS advertisement"
        );
        self.running.store(false, Ordering::Relaxed);
    }
}

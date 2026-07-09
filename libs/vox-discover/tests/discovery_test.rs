//! Integration test: verify two services can discover each other via mDNS.
//!
//! Run with: cargo test -p vox-discover --test discovery_test -- --nocapture

use vox_discover::{advertise, discover, PeerEvent, ServiceInfo};
use std::time::Duration;

#[tokio::test]
async fn two_services_discover_each_other() {
    // Advertise two services with different instance names
    let _guard_a = advertise(ServiceInfo {
        service_type: "fts-test",
        instance_name: "test-instance-a".into(),
        port: 19001,
        metadata: vec![("peer_id".into(), "peer-a".into())],
    })
    .expect("advertise A failed");

    let _guard_b = advertise(ServiceInfo {
        service_type: "fts-test",
        instance_name: "test-instance-b".into(),
        port: 19002,
        metadata: vec![("peer_id".into(), "peer-b".into())],
    })
    .expect("advertise B failed");

    // Give avahi a moment to register
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Start discovery
    let mut rx = discover("fts-test").await;

    let mut found = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Some(PeerEvent::Found(peer)) => {
                        eprintln!("  Found peer: {} at {}:{}", peer.instance_name, peer.host, peer.port);
                        if let Some(pid) = peer.get_meta("peer_id") {
                            eprintln!("    peer_id = {pid}");
                        }
                        found.push(peer);
                        if found.len() >= 2 {
                            break;
                        }
                    }
                    Some(PeerEvent::Lost(name)) => {
                        eprintln!("  Lost peer: {name}");
                    }
                    None => {
                        panic!("Discovery channel closed unexpectedly");
                    }
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                panic!(
                    "Timed out waiting for discovery — found {} of 2 peers.\n\
                     This likely means avahi is not configured for publishing.\n\
                     Check: services.avahi.publish.{{enable,userServices}} = true",
                    found.len()
                );
            }
        }
    }

    // Verify we found both
    let ports: Vec<u16> = found.iter().map(|p| p.port).collect();
    assert!(ports.contains(&19001), "Should have found service A (port 19001)");
    assert!(ports.contains(&19002), "Should have found service B (port 19002)");

    eprintln!("  SUCCESS: Both services discovered each other via mDNS!");
}

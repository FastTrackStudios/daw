//! Example DAW guest process — connects to REAPER via SHM bootstrap.
//!
//! Connects to daw-bridge, registers a test action, subscribes to action
//! events, and stays alive waiting for triggers. This is the minimal
//! lifecycle a real guest extension should follow.
//!
//! You can rebuild and restart this binary without restarting REAPER.
//!
//! # Usage
//!
//! ```sh
//! # Start REAPER with the daw-bridge loaded, then:
//! cargo run -p daw-guest-example
//!
//! # Or specify a custom bootstrap socket:
//! FTS_SHM_BOOTSTRAP_SOCK=/tmp/fts-daw-12345.bootstrap.sock cargo run -p daw-guest-example
//! ```

use daw_extension_runtime::{ActionDef, GuestOptions};
use eyre::Result;
use tracing::info;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(run())
}

async fn run() -> Result<()> {
    let pid = std::process::id();
    info!("[guest:{pid}] DAW guest example starting");

    // Connect to daw-bridge via SHM — one call does the full handshake
    let daw = daw_extension_runtime::connect(GuestOptions {
        role: "guest-example",
        ..Default::default()
    })
    .await?;

    info!("[guest:{pid}] Connected to daw-bridge via SHM");

    // Register actions and subscribe to events
    let reg = daw_extension_runtime::register_actions(&daw, &[
        ActionDef {
            command_name: "FTS_GUEST_HELLO",
            description: "FTS: Guest Hello World",
            toggleable: false,
        },
    ])
    .await?;

    let mut rx = reg.rx;

    info!("[guest:{pid}] Ready — waiting for action triggers");

    // ── Event loop ─────────────────────────────────────────────────────
    loop {
        match rx.recv().await {
            Ok(Some(item)) => {
                let daw::service::ActionEvent::Triggered { ref command_name } = *item;
                println!(">>> HOT RELOAD WORKS! Action: {command_name} <<<");
                info!("[guest:{pid}] Action triggered: {command_name}");
            }
            Ok(None) => {
                info!("[guest:{pid}] Action event stream ended");
                break;
            }
            Err(e) => {
                info!("[guest:{pid}] Action event stream error: {e:?}");
                break;
            }
        }
    }

    Ok(())
}

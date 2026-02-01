//! DAW Reaper Implementation
//!
//! This crate provides a REAPER-specific implementation of the DAW Protocol.

#![deny(unsafe_code)]

use daw_reaper::ReaperTransport;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    info!("DAW Reaper Cell starting...");

    let _transport = ReaperTransport::new();
    
    info!("Reaper transport initialized");
    info!("Waiting for connections...");
    
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");
    
    Ok(())
}
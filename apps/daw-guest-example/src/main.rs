//! Example DAW guest process — connects to REAPER via SHM bootstrap.
//!
//! This demonstrates the hot-reloadable guest pattern using `daw-extension-runtime`:
//! 1. One-call connect to daw-bridge via SHM
//! 2. Call DAW services (transport, project, tracks, FX, actions) via zero-copy SHM RPC
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

use daw_extension_runtime::GuestOptions;
use eyre::{Result, eyre};
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

    // ── Transport ────────────────────────────────────────────────────────
    let project = daw.current_project().await.map_err(|e| eyre!("{e}"))?;
    let transport = project.transport();

    info!("[guest:{pid}] Querying transport state...");
    let state = transport.get_state().await.map_err(|e| eyre!("{e}"))?;
    info!(
        "[guest:{pid}] Transport: tempo={:.1} BPM, playing={}, looping={}",
        state.tempo,
        state.play_state == daw::service::PlayState::Playing,
        state.looping,
    );

    // ── Project ──────────────────────────────────────────────────────────
    info!("[guest:{pid}] Creating new project tab...");
    let test_project = daw.create_project().await.map_err(|e| eyre!("{e}"))?;
    info!(
        "[guest:{pid}] Created project: guid={}",
        test_project.guid()
    );

    // ── Tracks ───────────────────────────────────────────────────────────
    info!("[guest:{pid}] Adding tracks...");
    let tracks = test_project.tracks();
    let track_a = tracks
        .add("SHM-Track-A", None)
        .await
        .map_err(|e| eyre!("{e}"))?;
    let track_b = tracks
        .add("SHM-Track-B", None)
        .await
        .map_err(|e| eyre!("{e}"))?;
    let track_a_info = track_a.info().await.map_err(|e| eyre!("{e}"))?;
    let track_b_info = track_b.info().await.map_err(|e| eyre!("{e}"))?;
    info!(
        "[guest:{pid}] Added tracks: A={}, B={}",
        track_a_info.name, track_b_info.name
    );

    let all_tracks = tracks.all().await.map_err(|e| eyre!("{e}"))?;
    info!("[guest:{pid}] Project has {} tracks", all_tracks.len());

    // ── FX ───────────────────────────────────────────────────────────────
    info!("[guest:{pid}] Adding ReaEQ to Track A...");
    let fx = track_a
        .fx_chain()
        .add("ReaEQ")
        .await
        .map_err(|e| eyre!("{e}"))?;
    let fx_info = fx.info().await.map_err(|e| eyre!("{e}"))?;
    info!(
        "[guest:{pid}] Added FX: {} (guid={})",
        fx_info.name,
        fx.guid()
    );

    let params = fx.parameters().await.map_err(|e| eyre!("{e}"))?;
    info!("[guest:{pid}] FX has {} parameters", params.len());
    if let Some(first) = params.first() {
        info!(
            "[guest:{pid}] First param: '{}' = {:.4}",
            first.name, first.value
        );
    }

    // ── Action Registry ────────────────────────────────────────────────────
    info!("[guest:{pid}] Testing action registry...");
    let actions = daw.action_registry();

    // Register a custom action
    let cmd_id = actions
        .register("fts.guest.hello", "FTS: Guest Hello World")
        .await
        .map_err(|e| eyre!("{e}"))?;
    info!("[guest:{pid}] Registered action 'fts.guest.hello' → cmd_id={cmd_id}");

    // Check it's registered
    let exists = actions
        .is_registered("fts.guest.hello")
        .await
        .map_err(|e| eyre!("{e}"))?;
    info!("[guest:{pid}] is_registered('fts.guest.hello') = {exists}");

    // Look up command ID
    let looked_up = actions
        .lookup_command_id("fts.guest.hello")
        .await
        .map_err(|e| eyre!("{e}"))?;
    info!("[guest:{pid}] lookup_command_id('fts.guest.hello') = {looked_up:?}");

    // Try looking up a non-existent action
    let missing = actions
        .is_registered("fts.nonexistent.action")
        .await
        .map_err(|e| eyre!("{e}"))?;
    info!("[guest:{pid}] is_registered('fts.nonexistent.action') = {missing}");

    // ── Cleanup ──────────────────────────────────────────────────────────
    info!("[guest:{pid}] Cleaning up: closing test project tab...");
    let _ = tracks.remove_all().await;
    let _ = daw.close_project(test_project.guid().to_string()).await;

    info!(
        "[guest:{pid}] Done. All DAW services available over SHM virtual connection. \
         Guest can be rebuilt and restarted without touching REAPER."
    );
    Ok(())
}

//! REAPER Transport Implementation
//!
//! Implements TransportService by queuing commands for main thread execution.
//! REAPER APIs can only be called from the main thread, so we use a callback
//! mechanism to dispatch commands.

use daw_proto::{PlayState, TransportService};
use roam::Context;
use std::sync::OnceLock;
use tracing::info;

/// Callback type for dispatching transport commands to the main thread
pub type TransportCallback = Box<dyn Fn(TransportCommand) + Send + Sync>;

/// Transport commands that need to run on the main thread
#[derive(Debug, Clone, Copy)]
pub enum TransportCommand {
    Play,
    Stop,
}

/// Global callback for dispatching transport commands
static TRANSPORT_CALLBACK: OnceLock<TransportCallback> = OnceLock::new();

/// Set the callback for dispatching transport commands to the main thread.
/// This should be called once during extension initialization.
pub fn set_transport_callback<F>(callback: F)
where
    F: Fn(TransportCommand) + Send + Sync + 'static,
{
    if TRANSPORT_CALLBACK.set(Box::new(callback)).is_err() {
        tracing::warn!("Transport callback already set");
    }
}

/// REAPER transport implementation that queues commands for main thread execution.
#[derive(Clone)]
pub struct ReaperTransport;

impl ReaperTransport {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportService for ReaperTransport {
    async fn play(&self, _cx: &Context, _project_id: Option<String>) {
        info!("REAPER Transport: Queuing play command");

        if let Some(callback) = TRANSPORT_CALLBACK.get() {
            callback(TransportCommand::Play);
        } else {
            tracing::error!("Transport callback not set - cannot execute play");
        }
    }

    async fn stop(&self, _cx: &Context, _project_id: Option<String>) {
        info!("REAPER Transport: Queuing stop command");

        if let Some(callback) = TRANSPORT_CALLBACK.get() {
            callback(TransportCommand::Stop);
        } else {
            tracing::error!("Transport callback not set - cannot execute stop");
        }
    }
}

/// Get the current transport play state from REAPER.
/// NOTE: This must only be called from the main thread!
pub fn get_play_state() -> PlayState {
    use reaper_high::Reaper;
    use reaper_medium::ProjectContext;

    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();

    let state = medium.get_play_state_ex(ProjectContext::CurrentProject);

    // Priority: Recording > Playing > Paused > Stopped
    if state.is_recording {
        PlayState::Recording
    } else if state.is_playing {
        PlayState::Playing
    } else if state.is_paused {
        PlayState::Paused
    } else {
        PlayState::Stopped
    }
}

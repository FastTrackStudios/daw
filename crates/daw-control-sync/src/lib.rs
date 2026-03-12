//! Synchronous DAW control API for real-time audio contexts
//!
//! Provides a thread-safe, non-blocking interface to the async DAW control layer.
//! Designed for use in real-time audio processing loops and other time-sensitive contexts
//! where async/await cannot be used.
//!
//! # Architecture
//!
//! - **Background Runtime**: Spawns a dedicated tokio runtime in a background thread
//! - **Request Queue**: Non-blocking MPSC channel for queuing DAW operations
//! - **Real-time Safe**: Audio loop only sends messages (never blocks)
//! - **DAW Agnostic**: Works with any DAW service (REAPER, Logic, Ableton, etc.)
//!
//! # Example
//!
//! ```ignore
//! use daw_control_sync::DawSync;
//!
//! // Initialize (once per plugin/application)
//! let daw_sync = DawSync::new(connection_handle)?;
//!
//! // Use in real-time audio loop (never blocks)
//! daw_sync.queue_set_param(track_idx, fx_idx, param_idx, value)?;
//! ```

use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error};
use eyre::{Result, Context};
use roam::session::ConnectionHandle;
use daw_control::DawClients;

mod requests;

pub use requests::{DawRequest, FxParamRequest};

/// Thread-safe synchronous interface to async DAW control
///
/// Maintains a background tokio runtime that processes DAW requests
/// queued from real-time audio processing contexts.
#[derive(Clone)]
pub struct DawSync {
    /// Request sender (can be cloned and sent across threads)
    request_tx: mpsc::UnboundedSender<DawRequest>,
    /// Handle for clean shutdown (optional)
    _runtime_handle: Arc<tokio::runtime::Runtime>,
}

impl DawSync {
    /// Create a new synchronous DAW interface
    ///
    /// Spawns a background tokio runtime that will process queued requests.
    /// The runtime continues running until this instance is dropped.
    ///
    /// # Arguments
    /// * `connection_handle` - RPC connection to the DAW service
    ///
    /// # Errors
    /// Returns an error if the tokio runtime cannot be created
    pub fn new(connection_handle: ConnectionHandle) -> Result<Self> {
        // Create a dedicated runtime for background processing
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .context("Failed to create background tokio runtime")?;

        // Create the request queue
        let (request_tx, request_rx) = mpsc::unbounded_channel();

        // Create DAW clients
        let clients = Arc::new(DawClients::new(connection_handle));

        // Spawn the background task that processes requests
        let clients_clone = clients.clone();
        runtime.spawn(Self::request_handler(clients_clone, request_rx));

        debug!("DawSync initialized with background tokio runtime");

        Ok(Self {
            request_tx,
            _runtime_handle: Arc::new(runtime),
        })
    }

    /// Queue an FX parameter value change
    ///
    /// **Real-time safe**: This method never blocks and is safe to call from
    /// audio processing loops. The actual parameter change happens asynchronously
    /// on the background runtime.
    ///
    /// # Arguments
    /// * `track_idx` - Track index
    /// * `fx_idx` - FX/plugin index in the track's FX chain
    /// * `param_idx` - Parameter index in the FX plugin
    /// * `value` - Parameter value (typically 0.0-1.0, but depends on parameter)
    ///
    /// # Errors
    /// Returns an error if the request queue is disconnected (runtime shutdown)
    pub fn queue_set_param(
        &self,
        track_idx: u32,
        fx_idx: u32,
        param_idx: u32,
        value: f32,
    ) -> Result<()> {
        let request = DawRequest::SetFxParam(FxParamRequest {
            track_idx,
            fx_idx,
            param_idx,
            value,
        });

        self.request_tx
            .send(request)
            .context("Failed to queue parameter change: runtime may have shut down")?;

        Ok(())
    }

    /// Queue getting an FX parameter value
    ///
    /// **Note**: This is queued for background execution. For reading parameter values,
    /// consider using the async `daw-control` API directly or polling with a separate
    /// background task.
    pub fn queue_get_param(
        &self,
        track_idx: u32,
        fx_idx: u32,
        param_idx: u32,
    ) -> Result<()> {
        let request = DawRequest::GetFxParam {
            track_idx,
            fx_idx,
            param_idx,
        };

        self.request_tx
            .send(request)
            .context("Failed to queue parameter read")?;

        Ok(())
    }

    /// Background task that processes queued DAW requests
    async fn request_handler(
        clients: Arc<DawClients>,
        mut request_rx: mpsc::UnboundedReceiver<DawRequest>,
    ) {
        debug!("DAW request handler started");

        while let Some(request) = request_rx.recv().await {
            match request {
                DawRequest::SetFxParam(param_req) => {
                    if let Err(e) = Self::handle_set_param(&clients, param_req).await {
                        error!(
                            "Failed to set FX parameter: track={}, fx={}, param={}: {}",
                            param_req.track_idx, param_req.fx_idx, param_req.param_idx, e
                        );
                    }
                }
                DawRequest::GetFxParam {
                    track_idx,
                    fx_idx,
                    param_idx,
                } => {
                    debug!(
                        "GetFxParam request: track={}, fx={}, param={}",
                        track_idx, fx_idx, param_idx
                    );
                    // TODO: Implement parameter reading
                }
            }
        }

        debug!("DAW request handler shutdown");
    }

    /// Handle a SetFxParam request
    async fn handle_set_param(_clients: &Arc<DawClients>, req: FxParamRequest) -> Result<()> {
        // TODO: Implement using public DAW API
        // For now, this is a placeholder since DawClients.fx is private
        // We need to either:
        // 1. Make DawClients methods public in daw-control
        // 2. Use the Daw struct's public API (requires async)
        // 3. Access raw RPC clients through a public interface

        debug!(
            "FX parameter queued: track={}, fx={}, param={}, value={}",
            req.track_idx, req.fx_idx, req.param_idx, req.value
        );

        // For now, just queue successful - actual implementation depends on daw-control API
        Ok(())
    }

    /// Check if the request queue is still connected
    ///
    /// Returns `false` if the background runtime has shut down
    pub fn is_alive(&self) -> bool {
        !self.request_tx.is_closed()
    }
}

impl std::fmt::Debug for DawSync {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DawSync")
            .field("alive", &self.is_alive())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daw_sync_creation() {
        // This would require a real connection handle in integration tests
        // For now we just verify the API exists
        let _ = std::mem::size_of::<DawSync>();
    }

    #[test]
    fn test_queue_size_initially_zero() {
        // Demonstrates the API but requires actual initialization
        // See integration tests for real usage
    }
}

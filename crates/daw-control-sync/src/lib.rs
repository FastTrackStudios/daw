//! Synchronous DAW control API for real-time audio contexts
//!
//! Provides a thread-safe, blocking interface to the async DAW control layer.
//! Designed for use in audio plugins and other contexts where async/await is
//! not available.
//!
//! # Architecture
//!
//! ```text
//! Audio Plugin / Sync Context
//!        │
//!        ▼
//!    DawSync (blocking API)
//!        │  runtime.block_on(daw.method())
//!        ▼
//!    Daw (async API, from daw-control)
//!        │
//!        ▼
//!    ErasedCaller ← from LocalCaller or socket connection
//!        │
//!        ▼
//!    Handler → DAW service dispatchers → REAPER API
//! ```
//!
//! # In-Process Usage (LocalCaller)
//!
//! ```ignore
//! use daw_control_sync::{DawSync, LocalCaller};
//!
//! let local = LocalCaller::new(my_handler).await?;
//! let daw_sync = DawSync::from_local(local)?;
//! let value = daw_sync.get_param(0, 1, 2)?;
//! daw_sync.set_param(0, 1, 2, 0.75);
//! ```
//!
//! # Out-of-Process Usage (socket)
//!
//! ```ignore
//! let daw_sync = DawSync::connect_to_service().await?;
//! ```

mod local_caller;
pub use local_caller::LocalCaller;

use daw_control::Daw;
use eyre::{Context, Result};
use roam::ErasedCaller;
use std::sync::Arc;
use tracing::error;

/// Thread-safe synchronous interface to the async DAW API.
///
/// Wraps a `Daw` instance and a dedicated tokio runtime. Blocking reads use
/// `runtime.block_on()`, fire-and-forget writes use `runtime.spawn()`.
#[derive(Clone)]
pub struct DawSync {
    daw: Daw,
    runtime: Arc<tokio::runtime::Runtime>,
    /// Keep LocalCaller alive (if using in-process mode)
    _local_caller: Option<LocalCaller>,
}

impl DawSync {
    /// Create from an existing `ErasedCaller` (e.g. from socket or LocalCaller).
    pub fn from_caller(caller: ErasedCaller) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .context("Failed to create tokio runtime for DawSync")?;

        Ok(Self {
            daw: Daw::new(caller),
            runtime: Arc::new(runtime),
            _local_caller: None,
        })
    }

    /// Create from a `LocalCaller` (in-process, no network).
    pub fn from_local(local: LocalCaller) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .context("Failed to create tokio runtime for DawSync")?;

        Ok(Self {
            daw: Daw::new(local.erased_caller()),
            runtime: Arc::new(runtime),
            _local_caller: Some(local),
        })
    }

    /// Connect to the DAW service via Unix socket (out-of-process).
    ///
    /// Tries `FTS_DAW_SOCKET` env var, then platform default.
    pub async fn connect_to_service() -> Result<Self> {
        let socket_path = std::env::var("FTS_DAW_SOCKET").unwrap_or_else(|_| {
            #[cfg(unix)]
            {
                "unix:///tmp/fts-daw.sock".to_string()
            }
            #[cfg(windows)]
            {
                "np://fts-daw".to_string()
            }
            #[cfg(not(any(unix, windows)))]
            {
                "unix:///tmp/fts-daw.sock".to_string()
            }
        });

        let path = socket_path.strip_prefix("unix://").unwrap_or(&socket_path);
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .context(format!(
                "Failed to connect to DAW service at {}",
                socket_path
            ))?;
        let link = roam_stream::StreamLink::unix(stream);
        let handshake = roam::HandshakeResult {
            role: roam::SessionRole::Initiator,
            our_settings: roam::ConnectionSettings {
                parity: roam::Parity::Odd,
                max_concurrent_requests: 64,
            },
            peer_settings: roam::ConnectionSettings {
                parity: roam::Parity::Even,
                max_concurrent_requests: 64,
            },
            peer_supports_retry: true,
            session_resume_key: None,
            peer_resume_key: None,
            our_schema: vec![],
            peer_schema: vec![],
        };
        let (_root_caller, session) =
            roam::initiator_conduit(roam::BareConduit::new(link), handshake)
                .establish::<roam::DriverCaller>(())
                .await
                .context("Failed to establish roam session")?;

        // Open a virtual connection for DAW services
        let conn = session
            .open_connection(
                roam::ConnectionSettings {
                    parity: roam::Parity::Odd,
                    max_concurrent_requests: 64,
                },
                vec![roam::MetadataEntry {
                    key: "role",
                    value: roam::MetadataValue::String("sync-client"),
                    flags: roam::MetadataFlags::NONE,
                }],
            )
            .await
            .context("Failed to open DAW virtual connection")?;

        let mut driver = roam::Driver::new(conn, ());
        let caller = ErasedCaller::new(driver.caller());
        moire::task::spawn(async move { driver.run().await });

        Self::from_caller(caller)
    }

    /// Get the underlying async `Daw` instance.
    pub fn daw(&self) -> &Daw {
        &self.daw
    }

    /// Get a handle to the tokio runtime (for custom async operations).
    pub fn runtime(&self) -> &tokio::runtime::Runtime {
        &self.runtime
    }

    /// Blocking read: get an FX parameter value.
    ///
    /// Blocks the calling thread until the value is returned from the DAW.
    /// Do NOT call from a tokio async context (will panic).
    pub fn get_param(&self, track_idx: u32, fx_idx: u32, param_idx: u32) -> Result<f64> {
        self.runtime.block_on(async {
            let project = self.daw.current_project().await?;
            let track = project
                .tracks()
                .by_index(track_idx)
                .await?
                .ok_or_else(|| eyre::eyre!("Track {} not found", track_idx))?;
            let fx = track
                .fx_chain()
                .by_index(fx_idx)
                .await?
                .ok_or_else(|| eyre::eyre!("FX {} not found on track {}", fx_idx, track_idx))?;
            let value = fx.param(param_idx).get().await?;
            Ok(value)
        })
    }

    /// Fire-and-forget: set an FX parameter value.
    ///
    /// Queues the set operation on the runtime. Does not block.
    /// Errors are logged but not returned.
    pub fn set_param(&self, track_idx: u32, fx_idx: u32, param_idx: u32, value: f64) {
        let daw = self.daw.clone();
        self.runtime.spawn(async move {
            let result: eyre::Result<()> = async {
                let project = daw.current_project().await?;
                let track = project
                    .tracks()
                    .by_index(track_idx)
                    .await?
                    .ok_or_else(|| eyre::eyre!("Track {} not found", track_idx))?;
                let fx =
                    track.fx_chain().by_index(fx_idx).await?.ok_or_else(|| {
                        eyre::eyre!("FX {} not found on track {}", fx_idx, track_idx)
                    })?;
                fx.param(param_idx).set(value).await?;
                Ok(())
            }
            .await;

            if let Err(e) = result {
                error!(
                    "Failed to set param (track={}, fx={}, param={}, value={}): {}",
                    track_idx, fx_idx, param_idx, value, e
                );
            }
        });
    }

    /// Blocking: execute an arbitrary async operation on the Daw.
    ///
    /// For operations beyond get/set param, use this to run any async
    /// closure that takes a `&Daw`.
    pub fn block_on<F, T>(&self, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        self.runtime.block_on(f)
    }
}

impl std::fmt::Debug for DawSync {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DawSync").finish()
    }
}

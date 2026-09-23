//! The standalone engine as a [`daw_transport_sync`] backend: several
//! machines' transports kept sample-aligned.
//!
//! Each project's transport publishes, at the start of every audio
//! buffer, where its playhead is *and when that was* (an
//! [`AudioSnapshot`], stamped in [`now_micros`]); runs at any rate with
//! true varispeed (the audio is resampled, so a drift correction of
//! 1.00008 is a pitch change nobody hears, never a skip); and lands a
//! scheduled locate on the exact frame of the buffer its moment falls
//! in. [`SyncBackend`] is that, for one project, behind
//! [`TransportBackend`] — get one with [`Standalone::sync_backend`] and
//! hand it to a [`daw_transport_sync::Follower`].
//!
//! Every transport driver does the per-buffer work: the cpal output
//! callback and the duplex callback (stamps filtered by a
//! [`daw_transport_sync::BufferClock`] over callback entry times), and
//! the soft clock when no device runs (stamped with its own tick times).

use std::sync::Arc;

use daw_transport_sync::{AudioSnapshot, ProjectId, TransportBackend};

use crate::sync::Standalone;
use crate::transport_engine::{ScheduledLocate, TransportShared};

/// The sync clock: this process's monotonic clock, microseconds — the
/// time domain of every snapshot's `host_micros` and of every
/// `at_micros` handed to [`SyncBackend::locate_at`].
///
/// Native: exactly [`daw_transport_sync::clock::now_micros_f64`] (so
/// stamps compare with anything else in the process that uses it). Web:
/// the browser's `performance.now()` clock via `web_time` (std's
/// `Instant` does not exist there).
#[must_use]
pub fn now_micros() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        daw_transport_sync::clock::now_micros_f64()
    }
    #[cfg(target_arch = "wasm32")]
    {
        static EPOCH: std::sync::OnceLock<web_time::Instant> = std::sync::OnceLock::new();
        EPOCH
            .get_or_init(web_time::Instant::now)
            .elapsed()
            .as_secs_f64()
            * 1e6
    }
}

/// A project's id for sync, from its GUID: the UUID's bytes when the
/// GUID is one (`{…}` or bare), otherwise a stable 128-bit hash of it.
#[must_use]
pub fn project_id_of(guid: &str) -> ProjectId {
    let bare = guid.trim_start_matches('{').trim_end_matches('}');
    if let Ok(u) = uuid::Uuid::parse_str(bare) {
        return *u.as_bytes();
    }
    // FNV-1a, twice with different offsets — stable across runs and
    // machines, which is all an id needs.
    let fnv = |seed: u64| {
        guid.bytes().fold(seed, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    };
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&fnv(0xcbf2_9ce4_8422_2325).to_le_bytes());
    out[8..].copy_from_slice(&fnv(0x6c62_272e_07bb_0142).to_le_bytes());
    out
}

/// One project's transport, as a [`TransportBackend`].
///
/// Cheap to clone (an `Arc`), `Send + Sync`: drive it from a tokio task.
/// All calls are lock-free atomics; the audio thread applies them.
#[derive(Clone, Debug)]
pub struct SyncBackend {
    shared: Arc<TransportShared>,
    project_id: ProjectId,
}

impl SyncBackend {
    /// A backend over a transport's shared state (what
    /// [`Standalone::sync_backend`] builds; public for engines wired by
    /// hand, and tests).
    #[must_use]
    pub const fn new(shared: Arc<TransportShared>, project_id: ProjectId) -> Self {
        Self { shared, project_id }
    }

    /// The transport this drives.
    #[must_use]
    pub const fn shared(&self) -> &Arc<TransportShared> {
        &self.shared
    }

    #[must_use]
    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }
}

impl TransportBackend for SyncBackend {
    fn snapshot(&self) -> Option<AudioSnapshot> {
        self.shared.sync_snapshot().map(|mut s| {
            s.project_id = self.project_id;
            s
        })
    }

    fn set_rate(&self, rate: f64) {
        if rate.is_finite() && rate > 0.0 {
            self.shared.set_playrate(rate);
        }
    }

    fn locate_at(&self, at_micros: f64, position: f64, playing: bool, rate: f64) {
        let rate = if rate.is_finite() && rate > 0.0 {
            rate
        } else {
            self.shared.playrate()
        };
        self.shared.schedule_locate(ScheduledLocate {
            at_micros,
            position_seconds: position,
            playing,
            rate,
        });
    }

    fn stop(&self, position: f64) {
        // Landed by the next buffer, at its first frame — so a stop never
        // races a block that is advancing the playhead.
        self.shared.schedule_locate(ScheduledLocate {
            at_micros: f64::NEG_INFINITY,
            position_seconds: position,
            playing: false,
            rate: self.shared.playrate(),
        });
    }
}

impl Standalone {
    /// Project `project_guid`'s transport as a sync backend; `None` when
    /// no such project is open. Creates the project's transport engine
    /// if it has none yet (which spawns its soft clock — call from
    /// within the async runtime).
    #[must_use]
    pub fn sync_backend(&self, project_guid: &str) -> Option<SyncBackend> {
        self.read_project(project_guid, |_| ())?;
        let bundle = self.transport_engine_for(project_guid);
        Some(SyncBackend::new(
            bundle.shared.clone(),
            project_id_of(project_guid),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_braced_uuid_guid_is_its_uuid() {
        let id = project_id_of("{67E55044-10B1-426F-9247-BB680E5FE0C8}");
        assert_eq!(
            id,
            *uuid::Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8")
                .unwrap()
                .as_bytes()
        );
    }

    #[test]
    fn any_other_guid_hashes_stably() {
        assert_eq!(project_id_of("p"), project_id_of("p"));
        assert_ne!(project_id_of("p"), project_id_of("q"));
    }

    #[test]
    fn the_backend_is_send_sync_clone() {
        fn check<T: Send + Sync + Clone + 'static>() {}
        check::<SyncBackend>();
    }
}

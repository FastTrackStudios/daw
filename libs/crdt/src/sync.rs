//! Real-time, offline-first doc sync over vox — the transport layer that
//! turns a [`CrdtDoc`] into a **collaborative replica**.
//!
//! Loro gives us the hard part for free: every replica holds the full
//! document, local writes apply instantly (offline-by-construction), and
//! `import` merges any update from any peer, in any order, idempotently.
//! What's left is moving update bytes around, and that is one vox call:
//!
//! ```text
//!  client A                      server                       client B
//!  ┌────────────┐   sync(vv,up,down)  ┌──────────────┐  sync   ┌────────────┐
//!  │ CrdtDoc    │ ──────────────────▶ │ canonical    │ ◀────── │ CrdtDoc    │
//!  │ (replica)  │  ◀── backlog ────── │ CrdtDoc      │ ──────▶ │ (replica)  │
//!  │ local ops ─┼──── up channel ───▶ │ + Persistence│ ─down─▶ │            │
//!  │            │ ◀─── down channel ──│ + PubSub     │         │            │
//!  └────────────┘                     └──────────────┘         └────────────┘
//! ```
//!
//! One [`DocSync::sync`] call per client: it carries the client's version
//! vector (so the server replies exactly the missing history — delta
//! catch-up after any offline period), an **up** channel of the client's
//! future local updates, and a **down** sink. The server merges every
//! incoming update into the canonical doc (persisting it), and fans it
//! out to every other replica through an [`architect::PubSub`] — the same
//! snapshot-then-changes attach discipline as entity events, so nothing
//! is missed between catch-up and live traffic.
//!
//! There is **no rollback** in this model — concurrent edits merge. The
//! optimistic store's reconcile/rollback machinery is for server-owned
//! state; CRDT-backed features replace it with a local replica that is
//! always written synchronously and always converges.
//!
//! The pieces:
//! - [`DocSync`] — the `#[vox::service]` wire trait (client + dispatcher
//!   emitted as usual).
//! - [`DocSyncHost`] — the server: canonical doc + fan-out. Native only
//!   (it spawns a pump per subscriber).
//! - [`SyncedDoc`] — the client driver: wires a local [`CrdtDoc`]'s
//!   update stream to the sync call and applies what comes back. Returns
//!   its pump as a future so any spawner works (`tokio::spawn`,
//!   `dioxus::spawn`, `wasm_bindgen_futures`) — wasm-clean.

use std::sync::Arc;

use uuid::Uuid;

use crate::{CrdtDoc, PersistError};

/// Why a sync attach failed.
#[derive(Debug, Clone, PartialEq, Eq, facet::Facet, thiserror::Error)]
#[repr(u8)]
pub enum SyncError {
    #[error("unknown doc")]
    UnknownDoc,
    #[error("bad version vector: {0}")]
    BadVersion(String),
    #[error("internal error: {0}")]
    Internal(String),
}

/// The sync wire trait. `from` is the client's encoded Loro version
/// vector (empty for a fresh replica); the server sends the missing
/// history through `down` first, then every subsequent update from any
/// peer. Updates the client makes flow through `up`.
#[vox::service]
pub trait DocSync {
    async fn sync(
        &self,
        doc_id: Uuid,
        from: Vec<u8>,
        up: vox::Rx<Vec<u8>>,
        down: vox::Tx<Vec<u8>>,
    ) -> Result<(), SyncError>;
}

// ── Server ──────────────────────────────────────────────────────────────

/// The server side of one collaborative doc: the canonical [`CrdtDoc`]
/// (with its persistence) plus the update fan-out.
///
/// Every update — whether it arrived from a replica's up-channel or was
/// made locally on the server (another transport mutating the same doc
/// through its `LoroRepo`s) — is published to every attached replica.
/// Loro imports are idempotent, so echoing a sender its own update is
/// harmless; replicas simply converge.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct DocSyncHost {
    doc_id: Uuid,
    doc: CrdtDoc,
    hub: architect::PubSub<Vec<u8>>,
    // Keeps the server-local-update → hub bridge alive.
    _local_sub: Arc<loro::Subscription>,
}

#[cfg(not(target_arch = "wasm32"))]
impl DocSyncHost {
    /// Wrap a canonical doc for serving. Server-side writes to the same
    /// doc (in-process repos, other transports) broadcast automatically.
    pub fn new(doc_id: Uuid, doc: CrdtDoc) -> Self {
        // Unbounded: update bytes must never be dropped (unlike state-
        // shaped entity events, a missed CRDT update is a lost edit until
        // the next full catch-up).
        let hub = architect::PubSub::unbounded();
        let bridge = hub.clone();
        let sub = doc.loro().subscribe_local_update(Box::new(move |bytes| {
            bridge.publish(bytes.to_vec());
            true
        }));
        Self {
            doc_id,
            doc,
            hub,
            _local_sub: Arc::new(sub),
        }
    }

    /// The canonical doc (for mounting entity repos on the server).
    pub fn doc(&self) -> &CrdtDoc {
        &self.doc
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl DocSync for DocSyncHost {
    async fn sync(
        &self,
        doc_id: Uuid,
        from: Vec<u8>,
        mut up: vox::Rx<Vec<u8>>,
        down: vox::Tx<Vec<u8>>,
    ) -> Result<(), SyncError> {
        if doc_id != self.doc_id {
            return Err(SyncError::UnknownDoc);
        }
        let from = if from.is_empty() {
            loro::VersionVector::new()
        } else {
            loro::VersionVector::decode(&from).map_err(|e| SyncError::BadVersion(e.to_string()))?
        };

        // Snapshot-then-changes: park the subscriber, export exactly the
        // history the replica is missing, deliver it ahead of anything
        // published since the park. (Overlap is fine — imports merge.)
        let pending = self.hub.begin_attach(down);
        let backlog = self.doc.loro().export(loro::ExportMode::Updates {
            from: std::borrow::Cow::Owned(from),
        });
        let backlog = match backlog {
            Ok(b) => b,
            Err(e) => {
                self.hub.abort_attach(pending);
                return Err(SyncError::Internal(format!("export updates: {e}")));
            }
        };
        self.hub.complete_attach(pending, Some(backlog));

        // Pump this replica's local updates into the canonical doc (which
        // persists them) and out to everyone else. The task ends when the
        // replica's connection closes.
        let doc = self.doc.clone();
        let hub = self.hub.clone();
        tokio::spawn(async move {
            while let Ok(Some(update)) = up.recv().await {
                let mut owned: Option<Vec<u8>> = None;
                let _ = update.map(|u| owned = Some(u.clone()));
                let Some(bytes) = owned else { continue };
                if let Err(e) = doc.apply_remote(&bytes) {
                    tracing::warn!("doc-sync: dropping bad update: {e}");
                    continue;
                }
                hub.publish(bytes);
            }
        });
        Ok(())
    }
}

// ── Client ──────────────────────────────────────────────────────────────

/// The client driver for one replica: connect a local [`CrdtDoc`] to a
/// [`DocSyncClient`] and the doc becomes collaborative — local writes
/// stream up, remote writes merge in, and a dropped connection just means
/// the next [`SyncedDoc::run`] catches up by version vector.
pub struct SyncedDoc {
    doc: CrdtDoc,
    doc_id: Uuid,
    outbox: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    _local_sub: loro::Subscription,
}

impl SyncedDoc {
    /// Wire a doc for syncing. Do this **once** per replica, before
    /// making local edits, so every local update lands in the outbox.
    pub fn new(doc_id: Uuid, doc: CrdtDoc) -> Self {
        let (tx, outbox) = tokio::sync::mpsc::unbounded_channel();
        let sub = doc.loro().subscribe_local_update(Box::new(move |bytes| {
            // Buffered while offline; drained by `run` when connected.
            let _ = tx.send(bytes.to_vec());
            true
        }));
        Self {
            doc,
            doc_id,
            outbox,
            _local_sub: sub,
        }
    }

    /// The local replica — hand out `doc().repo::<E>()` views to the UI.
    pub fn doc(&self) -> &CrdtDoc {
        &self.doc
    }

    /// Run one sync session to completion: catch up (by version vector),
    /// then exchange updates until the connection drops or the future is
    /// cancelled. Spawn it with whatever executor the platform has; on
    /// disconnect, call again with a fresh client — the version vector
    /// makes re-sync a delta, not a re-download.
    pub async fn run(&mut self, client: &DocSyncClient) -> Result<(), PersistError> {
        let (up_tx, up_rx) = vox::channel::<Vec<u8>>();
        let (down_tx, mut down_rx) = vox::channel::<Vec<u8>>();
        let vv = self.doc.loro().oplog_vv().encode();
        client
            .sync(self.doc_id, vv, up_rx, down_tx)
            .await
            .map_err(|e| PersistError::Backend(format!("doc sync attach: {e}")))?;

        loop {
            tokio::select! {
                // Local edits (live, or buffered while offline) go up.
                Some(update) = self.outbox.recv() => {
                    if up_tx.send(update).await.is_err() {
                        return Ok(()); // connection gone; caller re-runs
                    }
                }
                // Remote history + live peer updates merge in.
                incoming = down_rx.recv() => {
                    match incoming {
                        Ok(Some(update)) => {
                            let mut owned: Option<Vec<u8>> = None;
                            let _ = update.map(|u| owned = Some(u.clone()));
                            if let Some(bytes) = owned {
                                self.doc.apply_remote(&bytes)?;
                            }
                        }
                        _ => return Ok(()), // stream ended
                    }
                }
            }
        }
    }
}

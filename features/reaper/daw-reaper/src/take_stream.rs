//! Take state polling + broadcasting.
//!
//! Mirrors `item.rs`'s own-broadcaster pattern. Called from the
//! daw-bridge timer on the main thread.
//!
//! Diff strategy:
//! - For each open project, enumerate items via `Items::get_all_items`.
//! - For each item, snapshot takes via `Takes::get_takes`.
//! - Cache keyed by `(project_guid, item_guid)`.
//! - Emit `Created`, `Deleted`, `NameChanged`, `PitchChanged`,
//!   `PlayRateChanged`, `VolumeChanged`.
//!
//! `TakeEvent::SourceChanged` is intentionally not emitted from the
//! poll path — source changes are rare and detecting them reliably
//! needs source-pointer comparison rather than the lightweight diff
//! used here. Apply side is wired regardless.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use daw_proto::{ItemRef, Items, ProjectContext, Take, TakeEvent, Takes};
use reaper_high::{Project, Reaper as ReaperHigh};
use reaper_medium::ProjectRef;
use tokio::sync::broadcast;

use crate::project_context::{MAX_PROJECT_TABS, project_guid};

#[derive(Clone, Debug)]
struct CachedTake {
    guid: String,
    name: String,
    pitch: f64,
    play_rate: f64,
    volume: f64,
}

impl From<&Take> for CachedTake {
    fn from(t: &Take) -> Self {
        Self {
            guid: t.guid.clone(),
            name: t.name.clone(),
            pitch: t.pitch,
            play_rate: t.play_rate,
            volume: t.volume,
        }
    }
}

/// Cache key: (project_guid, item_guid).
type ItemKey = (String, String);

static TAKE_BROADCASTER: OnceLock<broadcast::Sender<TakeEvent>> = OnceLock::new();
static TAKE_CACHE: OnceLock<Mutex<HashMap<ItemKey, Vec<CachedTake>>>> = OnceLock::new();

const VOLUME_THRESHOLD: f64 = 0.0001;
const PITCH_THRESHOLD: f64 = 0.0001;
const PLAY_RATE_THRESHOLD: f64 = 0.0001;

/// Initialize the take event broadcaster. Idempotent.
pub fn init_take_broadcaster() {
    let (tx, _rx) = broadcast::channel::<TakeEvent>(1024);
    let _ = TAKE_BROADCASTER.set(tx);
    let _ = TAKE_CACHE.set(Mutex::new(HashMap::new()));
}

/// Subscribe to take events. Returns `None` until [`init_take_broadcaster`] runs.
pub fn subscribe_takes() -> Option<broadcast::Receiver<TakeEvent>> {
    TAKE_BROADCASTER.get().map(|tx| tx.subscribe())
}

/// Poll REAPER take state for every open project. **Main thread only.**
pub fn poll_and_broadcast_takes() {
    let Some(tx) = TAKE_BROADCASTER.get() else {
        return;
    };
    if tx.receiver_count() == 0 {
        return;
    }
    let Some(cache_cell) = TAKE_CACHE.get() else {
        return;
    };
    let mut cache = cache_cell.lock().expect("take cache mutex poisoned");

    let medium = ReaperHigh::get().medium_reaper();

    let mut seen_keys: Vec<ItemKey> = Vec::new();

    for tab_index in 0..MAX_PROJECT_TABS {
        let Some(result) = medium.enum_projects(ProjectRef::Tab(tab_index), 0) else {
            break;
        };
        let project = Project::new(result.project);
        let project_guid_str = project_guid(&project);
        let project_ctx = ProjectContext::Project(project_guid_str.clone());

        let items = Items::get_all_items(&crate::Reaper, project_ctx.clone());

        for item in &items {
            if item.guid.is_empty() {
                continue;
            }
            let fresh: Vec<Take> = Takes::get_takes(
                &crate::Reaper,
                project_ctx.clone(),
                ItemRef::Guid(item.guid.clone()),
            );
            let key = (project_guid_str.clone(), item.guid.clone());
            seen_keys.push(key.clone());

            let fresh_cached: Vec<CachedTake> = fresh.iter().map(CachedTake::from).collect();

            match cache.get(&key) {
                None => {
                    // First poll for this item — seed only.
                }
                Some(prev) => {
                    diff_and_emit(
                        tx,
                        &project_guid_str,
                        &item.guid,
                        prev,
                        &fresh_cached,
                        &fresh,
                    );
                }
            }
            cache.insert(key, fresh_cached);
        }
    }

    cache.retain(|k, _| seen_keys.contains(k));
}

fn diff_and_emit(
    tx: &broadcast::Sender<TakeEvent>,
    project_guid: &str,
    item_guid: &str,
    prev: &[CachedTake],
    curr: &[CachedTake],
    curr_full: &[Take],
) {
    use std::collections::HashMap;

    let prev_by_guid: HashMap<&str, &CachedTake> =
        prev.iter().map(|t| (t.guid.as_str(), t)).collect();
    let curr_by_guid: HashMap<&str, &CachedTake> =
        curr.iter().map(|t| (t.guid.as_str(), t)).collect();

    // Deleted.
    for p in prev {
        if !p.guid.is_empty() && !curr_by_guid.contains_key(p.guid.as_str()) {
            let _ = tx.send(TakeEvent::Deleted {
                project_guid: project_guid.to_string(),
                item_guid: item_guid.to_string(),
                take_guid: p.guid.clone(),
            });
        }
    }

    // Created / field-changed.
    for (i, c) in curr.iter().enumerate() {
        if c.guid.is_empty() {
            continue;
        }
        match prev_by_guid.get(c.guid.as_str()) {
            None => {
                if let Some(take) = curr_full.get(i) {
                    let _ = tx.send(TakeEvent::Created {
                        project_guid: project_guid.to_string(),
                        item_guid: item_guid.to_string(),
                        take: take.clone(),
                    });
                }
            }
            Some(prev) => {
                if prev.name != c.name {
                    let _ = tx.send(TakeEvent::NameChanged {
                        project_guid: project_guid.to_string(),
                        item_guid: item_guid.to_string(),
                        take_guid: c.guid.clone(),
                        name: c.name.clone(),
                    });
                }
                if (prev.pitch - c.pitch).abs() > PITCH_THRESHOLD {
                    let _ = tx.send(TakeEvent::PitchChanged {
                        project_guid: project_guid.to_string(),
                        item_guid: item_guid.to_string(),
                        take_guid: c.guid.clone(),
                        pitch: c.pitch,
                    });
                }
                if (prev.play_rate - c.play_rate).abs() > PLAY_RATE_THRESHOLD {
                    let _ = tx.send(TakeEvent::PlayRateChanged {
                        project_guid: project_guid.to_string(),
                        item_guid: item_guid.to_string(),
                        take_guid: c.guid.clone(),
                        play_rate: c.play_rate,
                    });
                }
                if (prev.volume - c.volume).abs() > VOLUME_THRESHOLD {
                    let _ = tx.send(TakeEvent::VolumeChanged {
                        project_guid: project_guid.to_string(),
                        item_guid: item_guid.to_string(),
                        take_guid: c.guid.clone(),
                        volume: c.volume,
                    });
                }
            }
        }
    }
}

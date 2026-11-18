//! One loaded source per file.
//!
//! A song is a few dozen recordings cut into thousands of items, and every
//! take of every item plays from one of them. `ProjectState.audio_sources`
//! is keyed by take, so loading media take by take opened the same file
//! once per take: 390 resident decodes of one 51 MB drum mic on
//! Unbreakable (#311). Every loader asks this cache instead: a file
//! already open hands back the `Arc<AudioSource>` its other takes play,
//! and a file is loaded at most once at a time (takes of one file loading
//! side by side wait for the first rather than each decoding it).
//!
//! The cache keeps nothing alive: it holds `Weak` refs, so a file no take
//! (and no undo step) plays any more unloads when its last `Arc` drops.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

use super::source::AudioSource;

/// What makes two takes' media the same media.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MediaKey {
    /// A file on disk, by its real path (links resolved: lab copies link
    /// one recording from many folders), its length and when it last
    /// changed — a file rewritten in place is new media.
    File {
        path: PathBuf,
        len: u64,
        modified: Option<std::time::SystemTime>,
    },
    /// Bytes a resolver handed over with no file behind them (a page's
    /// upload, a fetch), by their name, length and a digest of the bytes.
    Bytes {
        name: String,
        len: usize,
        digest: u64,
    },
}

impl MediaKey {
    /// The key of the file at `path`; `None` when it cannot be read.
    #[must_use]
    pub fn of_file(path: &Path) -> Option<Self> {
        let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let meta = std::fs::metadata(&real).ok()?;
        Some(Self::File {
            path: real,
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }

    /// The key of `bytes`, named `name` by the project.
    #[must_use]
    pub fn of_bytes(name: &str, bytes: &[u8]) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        Self::Bytes {
            name: name.to_owned(),
            len: bytes.len(),
            digest: hasher.finish(),
        }
    }
}

/// One file's place in the cache: the source loaded from it while any take
/// plays it. Locked while it loads, so a second take of the file waits for
/// the first's load instead of starting its own.
type Slot = Arc<Mutex<Weak<AudioSource>>>;

/// Loaded sources by the media they were loaded from.
#[derive(Default)]
pub struct SourceCache {
    slots: Mutex<Slots>,
}

#[derive(Default)]
struct Slots {
    by_key: HashMap<MediaKey, Slot>,
    /// Size at which dead slots are swept next: doubling keeps the sweeps
    /// amortised O(1) per insert.
    sweep_at: usize,
}

/// Slots below which the cache never sweeps.
const MIN_SWEEP: usize = 64;

impl SourceCache {
    /// The process's cache: one engine per process, and every song in it
    /// shares its recordings' sources.
    #[must_use]
    pub fn global() -> &'static Self {
        static CACHE: OnceLock<SourceCache> = OnceLock::new();
        CACHE.get_or_init(Self::default)
    }

    /// The source loaded from `key`, loading it with `load` when nothing
    /// plays it now. Takes of one file loading at once wait for the first.
    ///
    /// # Errors
    ///
    /// What `load` returned; nothing is cached, and the next take of the
    /// file tries again.
    pub fn get_or_load<E>(
        &self,
        key: MediaKey,
        load: impl FnOnce() -> Result<AudioSource, E>,
    ) -> Result<Arc<AudioSource>, E> {
        let slot = self.slot(key);
        let mut held = slot.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(source) = held.upgrade() {
            return Ok(source);
        }
        let source = Arc::new(load()?);
        *held = Arc::downgrade(&source);
        Ok(source)
    }

    /// Distinct sources loaded and still played.
    #[must_use]
    pub fn live(&self) -> usize {
        self.lock()
            .by_key
            .values()
            // A slot locked is loading: counted, never waited on.
            .filter(|slot| slot.try_lock().map_or(true, |s| s.strong_count() > 0))
            .count()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Slots> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `key`'s slot, made when it has none — sweeping out the slots of
    /// files nothing plays when the map has doubled since the last sweep.
    fn slot(&self, key: MediaKey) -> Slot {
        let mut slots = self.lock();
        if let Some(slot) = slots.by_key.get(&key) {
            return Arc::clone(slot);
        }
        if slots.by_key.len() >= slots.sweep_at {
            slots.by_key.retain(|_, slot| {
                // Held elsewhere: a load in flight.
                Arc::strong_count(slot) > 1
                    || slot
                        .try_lock()
                        .is_ok_and(|source| source.strong_count() > 0)
            });
            slots.sweep_at = (slots.by_key.len() * 2).max(MIN_SWEEP);
        }
        Arc::clone(slots.by_key.entry(key).or_default())
    }
}

/// What the process's audio sources hold, by how they hold it — read off
/// the audio thread (it is a handful of atomics and one short lock).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioMemory {
    /// Distinct sources the cache has loaded that some take still plays.
    pub sources: u64,
    /// Files mapped, and their size: backed by the page cache, paged in
    /// around the playhead and evictable, not resident memory of ours.
    pub mapped_files: u64,
    pub mapped_bytes: u64,
    /// Decoded chunks of streamed sources (proxies), resident.
    pub streamed_bytes: u64,
    /// Media decoded whole and resident — the preload budget's count,
    /// which the sampler's caches share.
    pub resident_bytes: u64,
}

/// What the process's audio sources hold now.
#[must_use]
pub fn audio_memory() -> AudioMemory {
    #[cfg(not(target_arch = "wasm32"))]
    let mapped = fts_sample::mapped::mapped();
    #[cfg(target_arch = "wasm32")]
    let mapped = MappedNone::default();
    AudioMemory {
        sources: SourceCache::global().live() as u64,
        mapped_files: mapped.files,
        mapped_bytes: mapped.bytes,
        streamed_bytes: super::streamed::resident_bytes(),
        resident_bytes: fts_sample::budget::used_bytes(),
    }
}

/// A page maps nothing.
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
struct MappedNone {
    files: u64,
    bytes: u64,
}

#[cfg(test)]
mod tests;

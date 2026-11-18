//! A map that says what was written to it: [`Journaled`].
//!
//! The engine's items, takes and everything kept per take live in these,
//! so a reader of the song — the window drawing it, the bridge recording
//! its history — can ask what changed since it last looked and read only
//! that, instead of the whole song (27 000 items on a sliced kit) after
//! every edit. Reads are the map's own (`Deref`); every write goes through
//! a method that notes the key. There is no `DerefMut`, so a write that
//! does not say what it touched does not compile.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};

/// Every write, in every journal, numbered in one sequence: a reader's
/// cursor is the number it had read up to.
static SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// The number the next write will take: a reader that has read the song
/// now holds this as its cursor.
#[must_use]
pub fn now() -> u64 {
    SEQUENCE.load(Ordering::Acquire)
}

fn next() -> u64 {
    SEQUENCE.fetch_add(1, Ordering::AcqRel)
}

/// How many writes a journal remembers before it forgets the older half;
/// a reader behind that is told everything changed.
const REMEMBERED: usize = 1 << 18;

/// A `HashMap` that notes each key written — and, while an undo step is
/// open ([`Self::record`]), what each key held before the step first wrote
/// it: an undo step is what it changed, never a copy of the song.
#[derive(Clone, Debug)]
pub struct Journaled<K, V> {
    map: HashMap<K, V>,
    /// Each write's number and key, oldest first.
    log: Vec<(u64, K)>,
    /// Writes numbered below this are not in the log: a reader whose
    /// cursor is older cannot be told what they were.
    floor: u64,
    /// The open undo step's record, if one is open.
    recording: Option<Box<Recording<K, V>>>,
}

/// What an open undo step has written: each key once, with what it held
/// before (`None`: it was not there).
#[derive(Clone, Debug)]
struct Recording<K, V> {
    seen: std::collections::HashSet<K>,
    before: Vec<(K, Option<V>)>,
}

/// One key an undo step changed: what it held before and after (`None`:
/// not there).
#[derive(Clone, Debug)]
pub struct Delta<K, V> {
    pub key: K,
    pub before: Option<V>,
    pub after: Option<V>,
}

/// Which side of an undo step to put back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// As it was before the step: an undo.
    Before,
    /// As the step left it: a redo.
    After,
}

impl<K, V> Default for Journaled<K, V> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            log: Vec::new(),
            floor: 0,
            recording: None,
        }
    }
}

impl<K, V> std::ops::Deref for Journaled<K, V> {
    type Target = HashMap<K, V>;

    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl<'a, K, V> IntoIterator for &'a Journaled<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::collections::hash_map::Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

impl<K: Hash + Eq, V> From<HashMap<K, V>> for Journaled<K, V> {
    /// A map made whole: every key in it is news to every reader.
    fn from(map: HashMap<K, V>) -> Self {
        Self {
            map,
            log: Vec::new(),
            floor: next() + 1,
            recording: None,
        }
    }
}

impl<K: Hash + Eq, V> FromIterator<(K, V)> for Journaled<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self::from(iter.into_iter().collect::<HashMap<K, V>>())
    }
}

impl<K: Hash + Eq + Clone, V: Clone> Journaled<K, V> {
    /// Before `key` is written: what it holds, kept for the open undo step
    /// the first time the step touches it.
    fn keep_before(&mut self, key: &K) {
        let Some(rec) = self.recording.as_mut() else {
            return;
        };
        if rec.seen.insert(key.clone()) {
            rec.before.push((key.clone(), self.map.get(key).cloned()));
        }
    }

    /// Before every key is written: each kept that the step has not yet.
    fn keep_all_before(&mut self) {
        let Some(rec) = self.recording.as_mut() else {
            return;
        };
        for (key, value) in &self.map {
            if rec.seen.insert(key.clone()) {
                rec.before.push((key.clone(), Some(value.clone())));
            }
        }
    }

    /// Open an undo step: from now, what each key held before its first
    /// write is kept. One already open is dropped.
    pub fn record(&mut self) {
        self.recording = Some(Box::new(Recording {
            seen: std::collections::HashSet::new(),
            before: Vec::new(),
        }));
    }

    /// Close the undo step: each key it wrote, as it was and as it is.
    /// Empty when none was open or it wrote nothing.
    #[must_use]
    pub fn recorded(&mut self) -> Vec<Delta<K, V>> {
        let Some(rec) = self.recording.take() else {
            return Vec::new();
        };
        rec.before
            .into_iter()
            .map(|(key, before)| {
                let after = self.map.get(&key).cloned();
                Delta { key, before, after }
            })
            .collect()
    }

    /// Put `deltas` back as they were (`Side::Before`, an undo) or as the
    /// step left them (`Side::After`, a redo): each key written and noted,
    /// so a reader re-reads what changed and nothing else.
    pub fn put_back(&mut self, deltas: &[Delta<K, V>], side: Side) {
        for delta in deltas {
            let value = match side {
                Side::Before => &delta.before,
                Side::After => &delta.after,
            };
            match value {
                Some(value) => {
                    self.insert(delta.key.clone(), value.clone());
                }
                None => {
                    self.remove_key(&delta.key);
                }
            }
        }
    }

    fn remove_key(&mut self, key: &K) -> Option<V> {
        self.keep_before(key);
        let removed = self.map.remove(key);
        if removed.is_some() {
            self.note(key.clone());
        }
        removed
    }

    fn note(&mut self, key: K) {
        if self.log.len() >= REMEMBERED {
            let keep = self.log.split_off(REMEMBERED / 2);
            self.floor = self.log.last().map_or(self.floor, |(at, _)| at + 1);
            self.log = keep;
        }
        self.log.push((next(), key));
    }

    /// Everything written, as far as any reader can tell: what a map put
    /// back whole (an undo's snapshot) says, since its own journal is the
    /// one it was copied with.
    pub fn note_all(&mut self) {
        self.keep_all_before();
        self.log.clear();
        // This write is numbered too: a reader at it is told everything.
        self.floor = next() + 1;
    }

    /// The keys written at or after `cursor`, or `None` when that is more
    /// than the journal remembers (read the whole map).
    #[must_use]
    pub fn written_since(&self, cursor: u64) -> Option<impl Iterator<Item = &K>> {
        if cursor < self.floor {
            return None;
        }
        let from = self.log.partition_point(|(at, _)| *at < cursor);
        Some(self.log[from..].iter().map(|(_, key)| key))
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = K> + ?Sized,
    {
        if self.map.contains_key(key) {
            let owned = key.to_owned();
            self.keep_before(&owned);
            self.note(owned);
        }
        self.map.get_mut(key)
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.keep_before(&key);
        self.note(key.clone());
        self.map.insert(key, value)
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = K> + ?Sized,
    {
        self.remove_key(&key.to_owned())
    }

    pub fn entry(&mut self, key: K) -> Entry<'_, K, V> {
        self.keep_before(&key);
        self.note(key.clone());
        self.map.entry(key)
    }

    /// Keep only what `keep` says to, each key it drops noted.
    pub fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool) {
        let recording = self.recording.is_some();
        let mut dropped = Vec::new();
        self.map.retain(|k, v| {
            let kept = keep(k, v);
            if !kept {
                dropped.push((k.clone(), recording.then(|| v.clone())));
            }
            kept
        });
        for (key, value) in dropped {
            if let Some(rec) = self.recording.as_mut()
                && rec.seen.insert(key.clone())
            {
                rec.before.push((key.clone(), value));
            }
            self.note(key);
        }
    }

    pub fn clear(&mut self) {
        self.note_all();
        self.map.clear();
    }

    pub fn extend(&mut self, items: impl IntoIterator<Item = (K, V)>) {
        for (key, value) in items {
            self.insert(key, value);
        }
    }

    /// Every value, to change: every key is noted as written.
    pub fn values_mut(&mut self) -> std::collections::hash_map::ValuesMut<'_, K, V> {
        self.note_all();
        self.map.values_mut()
    }

    /// Every entry, to change: every key is noted as written.
    pub fn iter_mut(&mut self) -> std::collections::hash_map::IterMut<'_, K, V> {
        self.note_all();
        self.map.iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::{Journaled, Side, now};

    #[test]
    fn a_reader_is_told_what_was_written_since_it_looked() {
        let mut map: Journaled<String, u32> = Journaled::default();
        map.insert("a".into(), 1);
        let cursor = now();
        map.insert("b".into(), 2);
        if let Some(a) = map.get_mut("a") {
            *a = 3;
        }
        let _ = map.get_mut("nobody");
        map.remove("b");
        let seen: Vec<&String> = map.written_since(cursor).expect("remembered").collect();
        assert_eq!(seen, ["b", "a", "b"]);
        assert!(
            map.written_since(now())
                .expect("remembered")
                .next()
                .is_none()
        );
    }

    #[test]
    fn a_change_to_every_value_is_everything() {
        let mut map: Journaled<String, u32> = [("a".to_owned(), 1)].into_iter().collect();
        let cursor = now();
        assert!(map.written_since(cursor).is_some());
        for v in map.values_mut() {
            *v += 1;
        }
        assert!(map.written_since(cursor).is_none(), "everything changed");
        assert!(map.written_since(now()).is_some());
    }

    #[test]
    fn an_undo_step_is_what_it_changed() {
        let mut map: Journaled<String, u32> = [
            ("a".to_owned(), 1),
            ("b".to_owned(), 2),
            ("c".to_owned(), 3),
        ]
        .into_iter()
        .collect();
        map.record();
        if let Some(a) = map.get_mut("a") {
            *a = 10;
        }
        if let Some(a) = map.get_mut("a") {
            *a = 11;
        }
        map.remove("b");
        map.insert("d".into(), 4);
        let step = map.recorded();
        assert_eq!(step.len(), 3, "each key once: {step:?}");
        let state = |m: &Journaled<String, u32>| {
            let mut all: Vec<(String, u32)> = m.iter().map(|(k, v)| (k.clone(), *v)).collect();
            all.sort();
            all
        };
        let after = state(&map);
        let cursor = now();
        map.put_back(&step, Side::Before);
        assert_eq!(
            state(&map),
            [("a".into(), 1), ("b".into(), 2), ("c".into(), 3)]
        );
        let told: Vec<&String> = map.written_since(cursor).expect("remembered").collect();
        assert_eq!(told.len(), 3, "a reader is told only the step's keys");
        map.put_back(&step, Side::After);
        assert_eq!(state(&map), after);
    }
}

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

/// A `HashMap` that notes each key written.
#[derive(Clone, Debug)]
pub struct Journaled<K, V> {
    map: HashMap<K, V>,
    /// Each write's number and key, oldest first.
    log: Vec<(u64, K)>,
    /// Writes numbered below this are not in the log: a reader whose
    /// cursor is older cannot be told what they were.
    floor: u64,
}

impl<K, V> Default for Journaled<K, V> {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            log: Vec::new(),
            floor: 0,
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
        }
    }
}

impl<K: Hash + Eq, V> FromIterator<(K, V)> for Journaled<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        Self::from(iter.into_iter().collect::<HashMap<K, V>>())
    }
}

impl<K: Hash + Eq + Clone, V> Journaled<K, V> {
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
            self.note(key.to_owned());
        }
        self.map.get_mut(key)
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.note(key.clone());
        self.map.insert(key, value)
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = K> + ?Sized,
    {
        let removed = self.map.remove(key);
        if removed.is_some() {
            self.note(key.to_owned());
        }
        removed
    }

    pub fn entry(&mut self, key: K) -> Entry<'_, K, V> {
        self.note(key.clone());
        self.map.entry(key)
    }

    /// Keep only what `keep` says to, each key it drops noted.
    pub fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool) {
        let mut dropped = Vec::new();
        self.map.retain(|k, v| {
            let kept = keep(k, v);
            if !kept {
                dropped.push(k.clone());
            }
            kept
        });
        for key in dropped {
            self.note(key);
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.note_all();
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
    use super::{Journaled, now};

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
}

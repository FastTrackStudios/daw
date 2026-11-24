//! What is selected: a set of ids that remembers the order they were
//! selected in, so "the first selected" means the one picked first.

use std::borrow::Borrow;
use std::collections::HashSet;
use std::hash::Hash;

/// An insertion-ordered set of ids.
#[derive(Clone, Debug)]
pub struct Selection<Id: Clone + Eq + Hash> {
    order: Vec<Id>,
    set: HashSet<Id>,
}

impl<Id: Clone + Eq + Hash> Default for Selection<Id> {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            set: HashSet::new(),
        }
    }
}

impl<Id: Clone + Eq + Hash> PartialEq for Selection<Id> {
    /// The same ids, in whatever order they were picked.
    fn eq(&self, other: &Self) -> bool {
        self.set == other.set
    }
}

impl<Id: Clone + Eq + Hash> Eq for Selection<Id> {}

impl<Id: Clone + Eq + Hash> Selection<Id> {
    /// Just `id`.
    pub fn set_single(&mut self, id: Id) {
        self.clear();
        self.add(id);
    }

    /// Add `id` at the end, if it is not already in. `true` if it was not.
    pub fn add(&mut self, id: Id) -> bool {
        if self.set.insert(id.clone()) {
            self.order.push(id);
            true
        } else {
            false
        }
    }

    /// Take `id` out. `true` if it was in.
    pub fn remove<Q: ?Sized + Hash + Eq>(&mut self, id: &Q) -> bool
    where
        Id: Borrow<Q>,
    {
        if self.set.remove(id) {
            self.order.retain(|each| each.borrow() != id);
            true
        } else {
            false
        }
    }

    /// In if it was out, out if it was in.
    pub fn toggle(&mut self, id: Id) {
        if !self.remove(&id) {
            self.add(id);
        }
    }

    pub fn clear(&mut self) {
        self.order.clear();
        self.set.clear();
    }

    #[must_use]
    pub fn contains<Q: ?Sized + Hash + Eq>(&self, id: &Q) -> bool
    where
        Id: Borrow<Q>,
    {
        self.set.contains(id)
    }

    /// In the order they were selected.
    pub fn iter(&self) -> impl Iterator<Item = &Id> {
        self.order.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The one selected first.
    #[must_use]
    pub fn first(&self) -> Option<&Id> {
        self.order.first()
    }

    /// The ids as a set, for what asks "is this one selected".
    #[must_use]
    pub const fn as_set(&self) -> &HashSet<Id> {
        &self.set
    }

    /// Exactly `ids`: those already selected keep their place, the new
    /// ones follow in the order given.
    pub fn assign<'a>(&mut self, ids: impl IntoIterator<Item = &'a Id>)
    where
        Id: 'a,
    {
        let wanted: Vec<&Id> = ids.into_iter().collect();
        let keep: HashSet<&Id> = wanted.iter().copied().collect();
        self.order.retain(|each| keep.contains(each));
        self.set.retain(|each| keep.contains(each));
        for id in wanted {
            self.add(id.clone());
        }
    }

    /// Everything, in order, leaving it empty.
    pub fn drain(&mut self) -> Vec<Id> {
        self.set.clear();
        std::mem::take(&mut self.order)
    }

    /// What a gesture on `under` acts on: the selection when `under` is
    /// in it, else `under` alone.
    #[must_use]
    pub fn targets(&self, under: &Id) -> Vec<Id> {
        if self.contains(under) {
            self.order.clone()
        } else {
            vec![under.clone()]
        }
    }
}

impl<Id: Clone + Eq + Hash> FromIterator<Id> for Selection<Id> {
    fn from_iter<I: IntoIterator<Item = Id>>(iter: I) -> Self {
        let mut s = Self::default();
        for id in iter {
            s.add(id);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_keeps_the_order_things_were_picked_in() {
        let mut s = Selection::default();
        s.add("b");
        s.add("a");
        s.add("b");
        assert_eq!(s.iter().copied().collect::<Vec<_>>(), ["b", "a"]);
        assert_eq!(s.first(), Some(&"b"));
        s.toggle("b");
        assert_eq!(s.first(), Some(&"a"));
    }

    #[test]
    fn assigning_keeps_the_survivors_in_place() {
        let mut s: Selection<u32> = [3, 1, 2].into_iter().collect();
        s.assign(&[2, 9, 3]);
        assert_eq!(s.iter().copied().collect::<Vec<_>>(), [3, 2, 9]);
    }

    #[test]
    fn a_gesture_acts_on_the_selection_only_from_inside_it() {
        let s: Selection<u32> = [1, 2].into_iter().collect();
        assert_eq!(s.targets(&2), vec![1, 2]);
        assert_eq!(s.targets(&7), vec![7]);
    }
}

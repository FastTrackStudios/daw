//! Cut, copy and paste: things lifted off the timeline, kept **relative to
//! the earliest one and the lowest row**, so a paste lands where it is put
//! and the spacing inside what was copied survives the trip.
//!
//! Not the system clipboard: these surfaces run inside hosts that route
//! keyboard and clipboard unpredictably, and a paste that silently picked
//! up a spreadsheet cell would be worse than one that only ever sees its
//! own kind. A host can bridge to the OS clipboard where it makes sense.

/// A thing on a row with a span — a note, an item.
pub trait Clip: Clone {
    fn start(&self) -> f64;
    fn end(&self) -> f64;
    fn row(&self) -> i32;
    /// The same thing `dt` later and `drows` rows over.
    #[must_use]
    fn shifted(&self, dt: f64, drows: i32) -> Self;
}

/// What was copied, rebased to its own origin.
#[derive(Clone, Debug, PartialEq)]
pub struct Clipboard<T> {
    /// Rebased: the earliest starts at 0, the lowest row is 0; sorted by
    /// start.
    items: Vec<T>,
    /// Where the lowest row was, so a paste can put it back there.
    origin_row: i32,
}

impl<T> Default for Clipboard<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            origin_row: 0,
        }
    }
}

impl<T: Clip> Clipboard<T> {
    /// Capture `items`. Capturing nothing leaves what was there, so a
    /// mis-aimed copy does not destroy it.
    pub fn copy(&mut self, items: impl IntoIterator<Item = T>) -> bool {
        let items: Vec<T> = items.into_iter().collect();
        if items.is_empty() {
            return false;
        }
        let t0 = items.iter().map(Clip::start).fold(f64::MAX, f64::min);
        let row0 = items.iter().map(Clip::row).min().unwrap_or(0);
        let mut rebased: Vec<T> = items.iter().map(|i| i.shifted(-t0, -row0)).collect();
        rebased.sort_by(|a, b| a.start().total_cmp(&b.start()));
        self.items = rebased;
        self.origin_row = row0;
        true
    }

    /// What a paste at `t` with the lowest row at `row` would produce.
    #[must_use]
    pub fn placed(&self, t: f64, row: i32) -> Vec<T> {
        self.items.iter().map(|i| i.shifted(t, row)).collect()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// The row the lowest of them was copied from.
    #[must_use]
    pub const fn origin_row(&self) -> i32 {
        self.origin_row
    }

    /// How long what was copied lasts: the room a paste needs.
    #[must_use]
    pub fn span(&self) -> f64 {
        self.items.iter().map(Clip::end).fold(0.0_f64, f64::max)
    }

    /// What was copied, rebased.
    #[must_use]
    pub fn items(&self) -> &[T] {
        &self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Box3(f64, f64, i32);

    impl Clip for Box3 {
        fn start(&self) -> f64 {
            self.0
        }
        fn end(&self) -> f64 {
            self.1
        }
        fn row(&self) -> i32 {
            self.2
        }
        fn shifted(&self, dt: f64, drows: i32) -> Self {
            Self(self.0 + dt, self.1 + dt, self.2 + drows)
        }
    }

    #[test]
    fn a_copy_keeps_its_spacing_and_lands_where_it_is_put() {
        let mut c = Clipboard::default();
        assert!(c.copy([Box3(12.0, 13.0, 7), Box3(10.0, 11.0, 5)]));
        assert_eq!(c.origin_row(), 5);
        assert_eq!(c.span(), 3.0);
        assert_eq!(
            c.placed(100.0, 2),
            vec![Box3(100.0, 101.0, 2), Box3(102.0, 103.0, 4)]
        );
    }

    #[test]
    fn copying_nothing_keeps_what_was_there() {
        let mut c = Clipboard::default();
        c.copy([Box3(1.0, 2.0, 0)]);
        assert!(!c.copy(Vec::<Box3>::new()));
        assert_eq!(c.len(), 1);
    }
}

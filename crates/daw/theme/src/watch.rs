//! Live reload: noticing that a theme's files changed.
//!
//! A [`Watch`] holds what a theme's layers came to (every file, and when
//! each last changed). [`Watch::changed`] looks again, and is true when a
//! file changed or a folder gained or lost one. It keeps no clock: the
//! host decides how often to look (a view looks a few times a second, on
//! its own frame).

use std::path::PathBuf;
use std::time::SystemTime;

use crate::pack;

/// A theme's files, as last seen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch {
    layers: Vec<PathBuf>,
    stamp: Vec<(PathBuf, Option<SystemTime>)>,
}

impl Watch {
    /// Watch `layers` (files or folders, base first), from how they are now.
    #[must_use]
    pub fn new(layers: Vec<PathBuf>) -> Self {
        let stamp = stamp(&layers);
        Self { layers, stamp }
    }

    /// The layers being watched.
    #[must_use]
    pub fn layers(&self) -> &[PathBuf] {
        &self.layers
    }

    /// The files they come to, in layering order.
    #[must_use]
    pub fn files(&self) -> Vec<PathBuf> {
        self.stamp.iter().map(|(p, _)| p.clone()).collect()
    }

    /// Whether anything changed since the last look; the change is taken.
    pub fn changed(&mut self) -> bool {
        let now = stamp(&self.layers);
        if now == self.stamp {
            return false;
        }
        self.stamp = now;
        true
    }
}

fn stamp(layers: &[PathBuf]) -> Vec<(PathBuf, Option<SystemTime>)> {
    pack::files(layers)
        .into_iter()
        .map(|f| {
            let modified = std::fs::metadata(&f).and_then(|m| m.modified()).ok();
            (f, modified)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::tests::{Scratch, TOKENS};

    #[test]
    fn a_change_an_addition_and_a_removal_are_each_seen_once() {
        let s = Scratch::new("watch");
        s.write("t/a.frame", TOKENS);
        let mut w = Watch::new(vec![s.0.join("t")]);
        assert!(!w.changed());
        s.write("t/b.frame", TOKENS);
        assert!(w.changed());
        assert!(!w.changed());
        std::fs::remove_file(s.0.join("t/b.frame")).expect("remove");
        assert!(w.changed());
        // A rewrite moves the mtime (set it, as a fast rewrite may not).
        let f = std::fs::File::options()
            .write(true)
            .open(s.0.join("t/a.frame"))
            .expect("open");
        f.set_modified(SystemTime::UNIX_EPOCH).expect("mtime");
        assert!(w.changed());
        assert_eq!(w.files(), vec![s.0.join("t/a.frame")]);
    }
}

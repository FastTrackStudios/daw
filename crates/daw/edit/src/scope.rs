//! What an edit acts on: the selection, as tracks and stretches of time,
//! and the profile those tracks are edited with.
//!
//! Select the drums — the kit's folder, a piece's, its folded row, a razor
//! over them, a stretch of time — and press Q: what is selected is a
//! [`Scope`], its tracks pick the [`Profile`], and the profile says what
//! is cut as one (a kit: every mic of every piece) and how.

use facet::Facet;

/// A stretch of project time, seconds.
#[derive(Facet, Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub from: f64,
    pub to: f64,
}

impl Span {
    /// The stretch between `a` and `b`, whichever is first.
    #[must_use]
    pub fn between(a: f64, b: f64) -> Self {
        Self {
            from: a.min(b),
            to: a.max(b),
        }
    }
}

/// What is selected: tracks (by guid, as picked — a folder stands for
/// everything under it, [`expand`]) and stretches of time. No tracks: the
/// stretches alone. No stretches: the whole song.
#[derive(Facet, Clone, Debug, Default, PartialEq)]
pub struct Scope {
    #[facet(default)]
    pub tracks: Vec<String>,
    #[facet(default)]
    pub spans: Vec<Span>,
}

impl Scope {
    /// Nothing selected at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty() && self.spans.is_empty()
    }

    /// The stretches in time order, those that overlap or touch made one.
    #[must_use]
    pub fn merged(&self) -> Vec<Span> {
        let mut spans: Vec<Span> = self
            .spans
            .iter()
            .filter(|s| s.to > s.from)
            .copied()
            .collect();
        spans.sort_by(|a, b| a.from.total_cmp(&b.from));
        let mut out: Vec<Span> = Vec::with_capacity(spans.len());
        for span in spans {
            match out.last_mut() {
                Some(last) if span.from <= last.to => last.to = last.to.max(span.to),
                _ => out.push(span),
            }
        }
        out
    }

    /// Everything the stretches cover, first to last; `None` with none.
    #[must_use]
    pub fn hull(&self) -> Option<Span> {
        let merged = self.merged();
        Some(Span {
            from: merged.first()?.from,
            to: merged.last()?.to,
        })
    }
}

/// A track as a scope reads the song's tree.
#[derive(Clone, Copy, Debug)]
pub struct Node<'a> {
    pub guid: &'a str,
    pub name: &'a str,
    pub parent: Option<&'a str>,
}

/// The tracks `picked` stands for, in the tree's order: each picked track
/// and everything under it.
#[must_use]
pub fn expand(tree: &[Node<'_>], picked: &[String]) -> Vec<String> {
    tree.iter()
        .filter(|n| ancestry(tree, n.guid).any(|a| picked.iter().any(|p| p == a.guid)))
        .map(|n| n.guid.to_owned())
        .collect()
}

/// `guid` and every folder above it, nearest first.
fn ancestry<'t, 'a>(tree: &'t [Node<'a>], guid: &str) -> impl Iterator<Item = Node<'a>> + 't {
    let mut at = tree.iter().find(|n| n.guid == guid).copied();
    // A tree is never deeper than it is long: a cycle stops there.
    let mut left = tree.len();
    std::iter::from_fn(move || {
        let here = at?;
        left = left.checked_sub(1)?;
        at = here
            .parent
            .and_then(|p| tree.iter().find(|n| n.guid == p).copied());
        Some(here)
    })
}

/// How a material is edited.
#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Profile {
    /// A kit: every mic of every piece cut as one, from its hit markers.
    Drums,
}

impl Profile {
    /// Its name, as a model and the status line say it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Drums => "drums",
        }
    }

    /// The profile a track is edited with, by the top-level folder it is
    /// in (the song's template: `Drums`, `Bass`, `Guitars`, …).
    #[must_use]
    pub fn of_track(tree: &[Node<'_>], guid: &str) -> Option<Self> {
        let top = ancestry(tree, guid).last()?;
        top.name
            .trim()
            .eq_ignore_ascii_case("drums")
            .then_some(Self::Drums)
    }
}

/// The one profile the scope's tracks are edited with.
///
/// No tracks — a stretch of time alone — is the kit's, as Q has always
/// been. Tracks of more than one material, or of one with no profile yet,
/// are refused: an edit is one material's.
///
/// # Errors
///
/// A track with no profile (named), or tracks of two.
pub fn profile_of(tree: &[Node<'_>], scope: &Scope) -> Result<Profile, String> {
    let mut found: Option<Profile> = None;
    for guid in &scope.tracks {
        let Some(profile) = Profile::of_track(tree, guid) else {
            let name = tree
                .iter()
                .find(|n| n.guid == *guid)
                .map_or(guid.as_str(), |n| n.name);
            return Err(format!(
                "{name} has no editing profile yet: only the drums are edited this way"
            ));
        };
        match found {
            Some(was) if was != profile => {
                return Err(format!(
                    "the selection is {} and {}: edit one at a time",
                    was.name(),
                    profile.name()
                ));
            }
            _ => found = Some(profile),
        }
    }
    Ok(found.unwrap_or(Profile::Drums))
}

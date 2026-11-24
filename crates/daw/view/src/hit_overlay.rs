//! The kit's hit markers, for the arrangement to draw over the drum rows
//! in Edit mode.
//!
//! The markers live on the recordings (take markers, `crate::hits`);
//! what is drawn is where each one plays now, after every edit, read off
//! the window's thread ([`refresh`]) whenever the hits or the edit
//! change and published here. A piece's marks go on that piece's rows
//! only — the kick's on the kick, the snare's on the snare — while an
//! edit moves the whole kit: the marks say where the hits are, the cuts
//! on every row what was done with them. The fill zones are shaded over
//! the kit, being the kit's.

#[cfg(feature = "native")]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Who put a hit there.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, facet::Facet)]
pub enum Who {
    /// A detector run, unchecked.
    Auto,
    /// Placed (or moved) by a person or an agent.
    Manual,
    /// Found, then checked.
    Approved,
}

/// What has been done to a hit's timing.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, facet::Facet)]
pub enum Edited {
    /// It plays where it was played.
    #[default]
    AsPlayed,
    /// An edit moved it (a quantize).
    Quantized,
    /// A hand placed it (its flag dragged).
    ByHand,
}

/// One hit, where it plays now.
#[derive(Clone, Debug, PartialEq, facet::Facet)]
pub struct Mark {
    /// Its marker's id: what a hand on it edits.
    pub id: String,
    /// Its piece: an index into [`Overlay::pieces`].
    pub piece: usize,
    /// Project seconds.
    pub time: f64,
    pub who: Who,
    /// A ghost note: drawn short.
    pub ghost: bool,
    /// Bleed, or found with too little confidence: one to look at.
    pub doubt: bool,
    /// Moved by an edit, by a hand, or neither.
    pub edited: Edited,
}

/// A fill or exception zone, shaded over the kit.
#[derive(Clone, Copy, Debug, PartialEq, facet::Facet)]
pub struct Shade {
    pub start: f64,
    pub end: f64,
    /// Found and not yet looked at.
    pub found: bool,
}

/// A stretch of a piece's slices played slower or faster than recorded
/// — a slice drawn out over a gap — where it plays now.
#[derive(Clone, Copy, Debug, PartialEq, facet::Facet)]
pub struct Stretch {
    /// Its piece: an index into [`Overlay::pieces`].
    pub piece: usize,
    /// Project seconds.
    pub start: f64,
    pub end: f64,
    /// How fast it plays its take: 1 as recorded, 0.5 half speed.
    pub rate: f64,
}

/// What is drawn.
#[derive(Clone, Debug, Default, PartialEq, facet::Facet)]
pub struct Overlay {
    /// The pieces' names (`Kick`, `Snare`, `Tom 1`).
    pub pieces: Vec<String>,
    /// Each piece's tracks (its mics and trigger), by GUID: a row is a
    /// piece's when one of them is the row's track or under it.
    pub tracks: Vec<Vec<String>>,
    /// In time order.
    pub marks: Vec<Mark>,
    pub zones: Vec<Shade>,
    /// Each piece's stretched stretches, in time order.
    pub stretches: Vec<Stretch>,
    /// Which take the comp plays where, in time order: the takes stitched
    /// together and the seams between them.
    pub comps: Vec<CompSpan>,
    /// The stretches kept as comped (`COMP_KEEP`), in time order, seconds.
    pub kept: Vec<(f64, f64)>,
    /// The takes' ranks and notes.
    pub takes: TakeMarks,
}

/// A take's rank or note where the song plays it ([`crate::take_rank`]).
#[derive(Clone, Debug, PartialEq, facet::Facet)]
pub struct TakeMark {
    /// Song seconds; `to` ends a take region.
    pub from: f64,
    pub to: Option<f64>,
    /// 1..=3 stars, -1 a red X, 0 a note alone.
    pub level: i8,
    pub note: String,
}

/// Whether `marks` (one take's) mark it bad — a red X — over the whole
/// of `(start, end)`, song seconds: down-ranked where it starts, and
/// nothing in it ranking it otherwise; or inside one down-ranked region.
#[must_use]
pub fn rejected(marks: &[TakeMark], (start, end): (f64, f64)) -> bool {
    /// A marker this near the start stands at it.
    const NEAR: f64 = 1e-6;
    let ranks = || marks.iter().filter(|m| m.level != 0);
    let in_region = ranks().any(|m| {
        m.level < 0
            && m.to
                .is_some_and(|to| m.from <= start + NEAR && to >= end - NEAR)
    });
    if in_region {
        return true;
    }
    let standing = ranks()
        .filter(|m| m.to.is_none() && m.from <= start + NEAR)
        .max_by(|a, b| a.from.total_cmp(&b.from))
        .is_some_and(|m| m.level < 0);
    // Ranked up again partway in, at a point or over a region.
    let raised = ranks().filter(|m| m.level > 0).any(|m| match m.to {
        None => m.from > start + NEAR && m.from < end,
        Some(to) => m.from < end && to > start,
    });
    standing && !raised
}

/// Every take's ranks and notes: by item, and by the kit's take (pass).
#[derive(Clone, Debug, Default, PartialEq, facet::Facet)]
pub struct TakeMarks {
    pub by_item: std::collections::HashMap<String, Vec<TakeMark>>,
    pub by_pass: std::collections::HashMap<String, Vec<TakeMark>>,
}

/// A stretch of the song the comp plays from one take.
#[derive(Clone, Debug, PartialEq, facet::Facet)]
pub struct CompSpan {
    /// Project seconds.
    pub start: f64,
    pub end: f64,
    /// The take's name, as its recording is called on the kit's first mic:
    /// what its colour comes from ([`crate::arrangement::take_color`]).
    pub take: String,
}

impl Overlay {
    /// The comp's spans `from..to` touches, each whole — a click
    /// (`from == to`) the one it is in: the stretches a T or Shift+T
    /// comps from another take.
    #[must_use]
    pub fn comps_over(&self, from: f64, to: f64) -> Vec<(f64, f64)> {
        let first = self.comps.partition_point(|c| c.end < from);
        self.comps
            .get(first..)
            .unwrap_or_default()
            .iter()
            .take_while(|c| c.start <= to)
            .filter(|c| c.end > from || (from - to).abs() < f64::EPSILON && c.end >= from)
            .map(|c| (c.start, c.end))
            .collect()
    }

    /// The marks between `from` and `to` seconds.
    #[must_use]
    pub fn between(&self, from: f64, to: f64) -> &[Mark] {
        let a = self.marks.partition_point(|m| m.time < from);
        let b = self.marks.partition_point(|m| m.time <= to);
        self.marks.get(a..b.max(a)).unwrap_or_default()
    }

    /// The pieces a row of track `guid` shows: those with a track that
    /// is it or under it (`parent_of` a track's folder) — one for a
    /// piece's folder or mic, every piece for the kit's folder shut, none
    /// for the cymbals and rooms.
    pub fn pieces_under<'a>(
        &self,
        guid: &str,
        parent_of: impl Fn(&str) -> Option<&'a str>,
    ) -> Vec<usize> {
        let under = |track: &str| {
            let mut at = Some(track);
            // A folder tree is shallow; the bound only guards a cycle.
            for _ in 0..64 {
                match at {
                    Some(t) if t == guid => return true,
                    Some(t) => at = parent_of(t),
                    None => return false,
                }
            }
            false
        };
        self.tracks
            .iter()
            .enumerate()
            .filter(|(_, tracks)| tracks.iter().any(|t| under(t)))
            .map(|(piece, _)| piece)
            .collect()
    }
}

static SHOWN: Mutex<Option<Arc<Overlay>>> = Mutex::new(None);
static VERSION: AtomicU64 = AtomicU64::new(0);

/// From markers taken up to the first frame painted with them: the span
/// `edit.shown`, open until a view says it drew them ([`drawn`]). A newer
/// one replaces an older one no frame showed.
static SHOWING: Mutex<Option<(u64, tracing::Span)>> = Mutex::new(None);

/// Show `overlay` (its marks sorted here).
pub fn publish(mut overlay: Overlay) {
    overlay.marks.sort_by(|a, b| a.time.total_cmp(&b.time));
    let marks = overlay.marks.len();
    if let Ok(mut shown) = SHOWN.lock() {
        *shown = Some(Arc::new(overlay));
    }
    let version = VERSION.fetch_add(1, Ordering::Relaxed).saturating_add(1);
    let span = tracing::info_span!("edit.shown", edit.version = version, edit.marks = marks);
    if let Ok(mut showing) = SHOWING.lock() {
        *showing = Some((version, span));
    }
    // Painted in the next frame, not at the next event.
    crate::wake::wake();
}

/// A view painted the markers of `version` (or later): `edit.shown` ends.
pub fn drawn(version: u64) {
    if let Ok(mut showing) = SHOWING.lock()
        && showing.as_ref().is_some_and(|(v, _)| *v <= version)
    {
        showing.take();
    }
}

/// What is shown, if anything.
#[must_use]
pub fn current() -> Option<Arc<Overlay>> {
    SHOWN.lock().ok().and_then(|s| s.clone())
}

/// Goes up on every change: a view compares it with the one it drew.
#[must_use]
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// A read is running.
#[cfg(feature = "native")]
static BUSY: AtomicBool = AtomicBool::new(false);
/// Asked for again while one ran: the hits may have changed under it.
#[cfg(feature = "native")]
static AGAIN: AtomicBool = AtomicBool::new(false);

/// Read the markers again in the engine, off the caller's thread, and
/// show them to every window (`crate::studio_engine::view_host`) — after
/// anything that moves a hit or a slice; a window reads its tracks'
/// waveform detail again with them. Asked for while a read runs, one more
/// runs after it; never two at once. Nothing with no engine in this
/// process.
pub fn refresh() {
    #[cfg(feature = "native")]
    {
        AGAIN.store(true, Ordering::Release);
        if BUSY.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(runtime) = crate::open::runtime() else {
            BUSY.store(false, Ordering::Release);
            return;
        };
        runtime.spawn_blocking(|| {
            loop {
                AGAIN.store(false, Ordering::Release);
                // Told to every window, which reads its tracks' detail
                // again with it: what moved the hits moved the audio (an
                // action — a quantize over MCP — edits the engine directly;
                // a track read while its edits were landing was held
                // half-edited, the take as played drawn under hits that
                // had moved, for good).
                crate::body::body().overlay_read_current();
                BUSY.store(false, Ordering::Release);
                if !AGAIN.load(Ordering::Acquire) || BUSY.swap(true, Ordering::AcqRel) {
                    break;
                }
            }
        });
    }
}

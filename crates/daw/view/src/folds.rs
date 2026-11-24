//! Which folders are open: the window's own choice over the layout's.
//!
//! The scene decides a first layout (`plan::apply_scene`); a person then
//! opens and shuts folders — from a folder's button in the arrangement or
//! at the foot of its mixer strip — and can fold every top-level folder at
//! once ([`set_tops`]): the whole session as its families, the drums one
//! row and one strip, to see everything that is going on and open only
//! what is being worked on.
//!
//! One state for the window, so the arrangement and the mixer always show
//! the same folders; applied where rows are planned
//! (`studio::plan_rows_with`), so a folder shut here gets its children's
//! items folded onto its row, as a folder the scene shut does. Every
//! change bumps [`generation`], which the views compare each frame to plan
//! their rows again.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

struct State {
    /// A folder's own choice, by guid: open (`true`) or shut.
    chosen: HashMap<String, bool>,
    /// Every top-level folder shut unless chosen open.
    tops: bool,
}

impl Default for State {
    /// Every top-level folder shut: a session opens as its families —
    /// Guide, Drums, Bass, Guitars, Vocals — one row and one strip each,
    /// each folder item showing what is in it. A folder opened is opened
    /// on its own; the Folder switch opens them all.
    fn default() -> Self {
        Self {
            chosen: HashMap::new(),
            tops: true,
        }
    }
}

#[cfg(not(test))]
static STATE: std::sync::LazyLock<Mutex<State>> =
    std::sync::LazyLock::new(|| Mutex::new(State::default()));
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[cfg(not(test))]
fn state() -> std::sync::MutexGuard<'static, State> {
    STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// Under test, each thread's own: a test folding folders must not fold the
// rows another test is planning beside it.
#[cfg(test)]
thread_local! {
    static STATE: &'static Mutex<State> = Box::leak(Box::default());
}

#[cfg(test)]
fn state() -> std::sync::MutexGuard<'static, State> {
    STATE
        .with(|state| *state)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Plan the rows again, as a fold does: something else they are drawn
/// from changed (which tracks show all their lanes).
pub fn touch() {
    changed();
}

fn changed() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// How many times the folds have changed: a view planning rows compares
/// this with the value it planned at.
#[must_use]
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Whether a shut folder's row shows every cut in what it holds — in
/// Edit mode, where the cuts are the work — or the takes whole, the seams
/// joined (`daw_ui::studio::folded::join`), everywhere else: mixing or
/// playing a song, a slip-edited kit is still one take a song long.
static CUTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether shut folders show their cuts.
#[must_use]
pub fn cuts_shown() -> bool {
    CUTS.load(Ordering::Relaxed)
}

/// Show shut folders' cuts or not; a change plans the rows again.
pub fn show_cuts(on: bool) {
    if CUTS.swap(on, Ordering::Relaxed) != on {
        changed();
    }
}

/// Whether the folder at `index` in `rows` (track, depth) shows shut:
/// a folder whose next row is not one of its own. What its fold button
/// draws, and what a click on it undoes.
#[must_use]
pub fn shut(rows: &[(daw_proto::Track, u32)], index: usize) -> bool {
    let Some((track, depth)) = rows.get(index) else {
        return false;
    };
    track.is_folder
        && rows
            .get(index.saturating_add(1))
            .is_none_or(|(_, next)| next <= depth)
}

/// Open the folder `guid` if it is shut (`open` = whether it shows open
/// now), shut it if it is open.
pub fn toggle(guid: &str, open: bool) {
    state().chosen.insert(guid.to_owned(), !open);
    changed();
}

/// Whether every top-level folder is folded.
#[must_use]
pub fn tops() -> bool {
    state().tops
}

/// Fold every top-level folder (`true`), or open them again (`false`).
/// Either way the folders chosen one by one start over: the switch is a
/// fresh overview, not a filter over earlier choices.
pub fn set_tops(on: bool) {
    let mut state = state();
    state.tops = on;
    state.chosen.clear();
    drop(state);
    changed();
}

/// `rows` (tracks and their depths, in order) with the shut folders'
/// contents left out: each folder keeps its row.
#[must_use]
pub fn apply(rows: Vec<(daw_proto::Track, u32)>) -> Vec<(daw_proto::Track, u32)> {
    // Worked out before the lock: a scan of every row, no state in it.
    let parts = parts_and_sides(&rows);
    // The choices copied out, the lock let go before the walk.
    let (chosen, tops) = {
        let state = state();
        (state.chosen.clone(), state.tops)
    };
    let mut out = Vec::with_capacity(rows.len());
    // The depth of the shut folder whose contents are being skipped.
    let mut skipping: Option<u32> = None;
    for (track, depth) in rows {
        if let Some(shut) = skipping {
            if depth > shut {
                continue;
            }
            skipping = None;
        }
        if track.is_folder {
            let open = chosen.get(&track.guid).copied().unwrap_or_else(|| {
                !(shut_by_default(&track)
                    || parts.contains(track.guid.as_str())
                    || tops && depth == 0)
            });
            if !open {
                skipping = Some(depth);
            }
        }
        out.push((track, depth));
    }
    out
}

/// Whether a folder is one whose own folders start shut — each a part of
/// it (a guitars folder's parts, a drums folder's pieces): the app's to
/// say ([`crate::host::Host::shuts_its_folders`]). Opened, the family is a
/// short list of rows, not every mic.
fn shuts_its_folders(name: &str) -> bool {
    crate::host::host().shuts_its_folders(name)
}

/// Whether a folder name is a side of a stereo part: `L`, `R`, `C`.
fn is_side(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "l" | "r" | "c" | "left" | "right" | "center" | "centre"
    )
}

/// The folders that start shut because they are one part played to
/// several sides: a part (`Clean`, `Rhythm`, `Lead Harm` — a folder whose
/// children are its `L`/`R`/`C` sides) and each side. Shut, the part is
/// one row, its folder item showing what is under it; the sides and their
/// mics are there when it is opened. By guid.
fn parts_and_sides(rows: &[(daw_proto::Track, u32)]) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for (i, (track, depth)) in rows.iter().enumerate() {
        if !track.is_folder {
            continue;
        }
        let children = rows
            .iter()
            .skip(i.saturating_add(1))
            .take_while(|(_, d)| d > depth)
            .filter(|(_, d)| *d == depth.saturating_add(1));
        let children: Vec<&daw_proto::Track> = children.map(|(t, _)| t).collect();
        let sides: Vec<&&daw_proto::Track> = children
            .iter()
            .filter(|t| t.is_folder && is_side(&t.name))
            .collect();
        if !sides.is_empty() {
            out.insert(track.guid.clone());
            out.extend(sides.iter().map(|t| t.guid.clone()));
        }
        // A guitars folder's own folders are its parts (`Clean`, `Rhythm`,
        // `Solo`), sides or none; a drums folder's, the kit's pieces: each
        // one row.
        // At any depth: Drums opens to Drum Kit and Process, Drum Kit to
        // its pieces.
        if shuts_its_folders(&track.name) {
            out.extend(
                children
                    .iter()
                    .filter(|t| t.is_folder)
                    .map(|t| t.guid.clone()),
            );
        }
    }
    out
}

/// Whether a folder starts shut until it is opened (the app's buses, its
/// reference material): the app's to say
/// ([`crate::host::Host::shut_by_default`]).
fn shut_by_default(track: &daw_proto::Track) -> bool {
    crate::host::host().shut_by_default(track)
}

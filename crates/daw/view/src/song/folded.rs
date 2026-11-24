//! A collapsed folder's own items, folded from its children's.
//!
//! # What this is for
//!
//! A drum kit is a folder tree because that is how the audio has to be
//! routed: a kick is a Sum with three mics under it, a tom is its mic
//! and its trigger. But the routing is a mix concern, and a mix concern
//! is not what an arrangement is for. While you are editing, a kick is
//! **one thing** — the mics were recorded together, comped together and
//! trimmed together, so what you want on screen is one row per drum with
//! the take on it, and the mics available when you go looking for them.
//!
//! REAPER draws nothing on a closed folder, so folding the kit away
//! today means folding the audio away with it. This is what fills that
//! row: the folder keeps its own track control panel — its name, its
//! fader, its colour, because the mix happens there — and the items on
//! it are its children's.
//!
//! # The rule
//!
//! Every boundary of every child, in order; the row is the spans between
//! them that at least one child is sounding across.
//!
//! One rule, and the two cases the desk cares about fall out of it
//! rather than being special-cased:
//!
//! - **The mics agree**, which is the normal case, because they are one
//!   performance. Their boundaries are the same boundaries, so the spans
//!   are the items, and the row looks exactly like the take.
//! - **They do not**, because one mic was punched in, or a trigger lane
//!   was trimmed on its own. Then the extra boundaries appear as extra
//!   spans, and the row says so. It is not a lie about a clean take and
//!   it is not a refusal to draw; it is the edit that is actually there.
//!
//! [`Span::whole`] is what tells the two apart, for anything downstream
//! that wants to draw the second differently.
//!
//! # Why items and not a picture
//!
//! The arrangement's `folder_item` module folds children's
//! PEAKS onto a column grid and draws one continuous picture across the
//! folder's whole extent. That is the right answer for an overview and
//! the wrong one here: a picture has no edges to grab, no title, and no
//! gaps between takes. These are real items, so the row hit-tests,
//! labels and selects like every other row, and an edit has somewhere to
//! land.
//!
//! # Editing, which is the point and is not built yet
//!
//! **An edit on a folded item is an edit on every child item under it.**
//! Drag the kick's take and its three mics move together; trim its edge
//! and they all trim to the same boundary; delete it and they all go.
//! That is the whole reason this row is worth having — a view you can
//! read but not work in is a view you stop using — and it is decided,
//! not speculative. It is simply not written yet.
//!
//! Everything it needs is here. A folded item's guid carries the folder
//! and the span ([`guid_of`], [`parse_guid`]), and the span carries the
//! real items ([`Span::from`]). So the path is: the hit test lands on a
//! folded guid, [`parse_guid`] says which fold, [`Span::from`] says which
//! items, and the edit is applied to all of them as ONE undo step.
//!
//! What is genuinely undecided is the ragged case — what trimming the
//! edge of a span where [`Span::whole`] is false should do, when only
//! some of the children start there. That, the undo grouping, and what
//! take lanes mean on a row that has no takes of its own are written up
//! on issue #114.
//!
//! Until then, one invariant holds absolutely: a `folded:` guid names
//! nothing in the session and must never reach the backend. It is a view
//! coordinate, and writing one would address an item that does not
//! exist.

use std::collections::HashMap;

use daw_proto::Item;

/// One stretch of a folded row: a span with children sounding across it.
#[derive(Clone, PartialEq, Debug)]
pub struct Span {
    pub start: f64,
    pub end: f64,
    /// The child items sounding across it.
    ///
    /// Kept rather than counted because this is the only record of what
    /// a folded item actually IS. It is also the list an edit works
    /// through: moving, trimming or deleting the folded item means doing
    /// the same to every guid in here, in one undo step — see the module
    /// doc. In project order, so the list is stable between renders and
    /// two equal folds compare equal.
    pub from: Vec<String>,
    /// Whether every child sounding here starts and ends exactly here.
    ///
    /// True is the ordinary case and means the span is a take: one item,
    /// no edges of its own. False means the children disagreed and this
    /// is a piece of the disagreement — worth drawing as a fragment
    /// rather than as a take, because that is what it is.
    pub whole: bool,
}

impl Span {
    /// How long it lasts.
    #[must_use]
    pub fn length(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
}

/// The shortest span worth drawing, in seconds.
///
/// Two children whose boundaries differ by a microsecond are two
/// children that agree; the difference is a rounding in whatever wrote
/// the file, not an edit anybody made. Without a floor here every such
/// pair would produce a third span a micron wide between them, and a
/// clean take would render as three items.
const GRAIN: f64 = 0.001;

/// Fold a folder's children's items into the items its own row shows.
///
/// `children` is one slice per child track, each in position order.
#[must_use]
pub fn spans(children: &[&[Item]]) -> Vec<Span> {
    let children: Vec<Vec<&Item>> = children.iter().map(|lane| lane.iter().collect()).collect();
    spans_of(&children)
}

/// [`spans`], over the children's items borrowed.
fn spans_of(children: &[Vec<&Item>]) -> Vec<Span> {
    // Every edge anybody has, in order and without duplicates. The
    // spans are what lies between consecutive edges, so this is the
    // whole algorithm — the rest is asking who is sounding where.
    let mut edges: Vec<f64> = Vec::new();
    for lane in children {
        for item in lane {
            edges.push(item.position.as_seconds());
            edges.push(item.position.as_seconds() + item.length.as_seconds());
        }
    }
    edges.sort_by(f64::total_cmp);
    edges.dedup_by(|a, b| (*a - *b).abs() < GRAIN);

    // Who covers each span's MIDDLE, not who touches an edge: an item
    // that ends exactly where a span starts is not sounding in it. Each
    // item finds the spans it covers by their middles, sorted as the edges
    // are: a sweep, not every item asked about every span (the kit's 27k
    // items took 220 ms that way).
    let middles: Vec<f64> = edges
        .windows(2)
        .map(|pair| f64::midpoint(pair[0], pair[1]))
        .collect();
    let mut from: Vec<Vec<String>> = vec![Vec::new(); middles.len()];
    let mut whole: Vec<bool> = vec![true; middles.len()];
    for lane in children {
        for item in lane {
            let at = item.position.as_seconds();
            let to = at + item.length.as_seconds();
            let first = middles.partition_point(|m| *m <= at);
            let past = middles.partition_point(|m| *m < to);
            for k in first..past {
                let (start, end) = (edges[k], edges[k + 1]);
                whole[k] &= (at - start).abs() < GRAIN && (to - end).abs() < GRAIN;
                from[k].push(item.guid.clone());
            }
        }
    }
    edges
        .windows(2)
        .zip(from)
        .zip(whole)
        .filter(|((pair, from), _)| pair[1] - pair[0] >= GRAIN && !from.is_empty())
        .map(|((pair, from), whole)| Span {
            start: pair[0],
            end: pair[1],
            from,
            whole,
        })
        .collect()
}

/// The folded spans as items on the folder's own row.
///
/// Real [`Item`]s, and deliberately: everything that draws, labels,
/// hit-tests or selects a lane already works on items, and a second kind
/// of thing on a row would have to be taught to all of it.
///
/// The guid is derived from the folder and the span's place in it, which
/// makes it stable across a re-fold — a shape cache and a selection both
/// key on it — and recoverable, which is how an edit gets from the row
/// back to the mics under it. Nothing in the session has this guid, so
/// it can never be mistaken for a real item and written to REAPER.
#[must_use]
pub fn lane(folder: &daw_proto::Track, spans: &[Span]) -> Vec<Item> {
    spans
        .iter()
        .enumerate()
        .map(|(index, span)| Item {
            guid: guid_of(&folder.guid, index),
            track_guid: folder.guid.clone(),
            index: u32::try_from(index).unwrap_or(u32::MAX),
            position: daw_proto::PositionInSeconds::from_seconds(span.start),
            length: daw_proto::Duration::from_seconds(span.length()),
            // The folder's own colour and the folder's own name, because
            // the row IS the folder: its fader is what moves and its
            // name is what is written beside it. Taking a child's would
            // say the row belonged to one of the mics — a kick whose
            // takes are labelled `In 1` is a kick that looks like its
            // inside mic.
            color: folder.color,
            label: Some(format!("{} {}", folder.name, index.saturating_add(1))),
            ..Item::default()
        })
        .collect()
}

/// The guid a folded item gets.
#[must_use]
pub fn guid_of(folder_guid: &str, index: usize) -> String {
    format!("folded:{folder_guid}:{index}")
}

/// The folder a folded guid came from, and which of its spans.
///
/// The way back. An edit lands on a row, the row hands over a guid, and
/// this says which fold it belongs to — from there [`Span::from`] says
/// which real items have to move.
#[must_use]
pub fn parse_guid(guid: &str) -> Option<(&str, usize)> {
    let rest = guid.strip_prefix("folded:")?;
    let (folder, index) = rest.rsplit_once(':')?;
    Some((folder, index.parse().ok()?))
}

/// Work out every shut folder's fold and put it on the project.
///
/// The one place a window does this, so the painted arrangement and the
/// component tree cannot disagree about what a shut folder shows.
///
/// `shown` is the row list the scene left — a folder is shut when it is
/// on screen and nothing under it is. `on` is
/// `Settings::folded_takes`: off, the folds are cleared and a shut
/// folder is an empty row, which is what REAPER draws.
///
/// Replaces rather than merges: a fold is derived from the session and
/// the view, and keeping a stale one would draw a take where the session
/// no longer has one.
pub fn refold(project: &mut super::project::Project, shown: &[daw_proto::Track], on: bool) {
    refold_lanes(project, shown, on, &|_, _| false);
}

/// [`refold`], with the shut folders `laned` names (asked with every
/// track, and the folder) folded lane by lane: each lane of the tracks
/// under it that have lanes folded on its own, its items on that lane
/// (`fixed_lane`) — every take beside the comp, to look at, hear and comp
/// from, rather than only what plays.
pub fn refold_lanes(
    project: &mut super::project::Project,
    shown: &[daw_proto::Track],
    on: bool,
    laned: &dyn Fn(&[daw_proto::Track], &daw_proto::Track) -> bool,
) {
    project.folds.clear();
    if !on {
        return;
    }
    let tracks = std::mem::take(&mut project.tracks);
    for folder in shut(&tracks, shown) {
        if let Some(fold) = fold_of(project, &tracks, folder, laned(&tracks, folder)) {
            project.folds.insert(folder.guid.clone(), fold);
        }
    }
    project.tracks = tracks;
}

/// `folder`'s fold: what its shut row shows of the tracks under it — lane
/// by lane when `laned` (every take beside the comp), else what they play.
/// `None` when nothing under it holds anything to show. One folder of
/// [`refold_lanes`], to fold again only the folder an edit was under.
#[must_use]
pub fn fold_of(
    project: &super::project::Project,
    tracks: &[daw_proto::Track],
    folder: &daw_proto::Track,
    laned: bool,
) -> Option<Fold> {
    if laned {
        let mut fold = fold_lanes(project, tracks, folder)?;
        let children = under(tracks, &folder.guid);
        fold.gaps = gaps(
            children
                .iter()
                .filter(|t| !t.is_folder && t.lane_count > 1)
                .map(|child| project.lane(&child.guid).iter()),
        );
        return Some(fold);
    }
    // What each child plays: a take on a lane that does not play is an
    // alternative, not part of the folder's sound.
    let playing: Vec<Vec<&Item>> = under(tracks, &folder.guid)
        .iter()
        .map(|child| {
            project
                .lane(&child.guid)
                .iter()
                .filter(|item| super::project::on_playing_lane(child, item))
                .collect()
        })
        .collect();
    let spans = spans_of(&playing);
    if spans.is_empty() {
        return None;
    }
    let items = lane(folder, &spans);
    let gaps = gaps(playing.iter().map(|child| child.iter().copied()));
    Some(Fold { items, spans, gaps })
}

/// A shut folder folded lane by lane: lane `n` of every track under it
/// with lanes (more than one) folded into the spans of the folder's lane
/// `n`. `None` when no lane holds anything.
fn fold_lanes(
    project: &super::project::Project,
    tracks: &[daw_proto::Track],
    folder: &daw_proto::Track,
) -> Option<Fold> {
    let children: Vec<&daw_proto::Track> = under(tracks, &folder.guid)
        .into_iter()
        .filter(|t| !t.is_folder && t.lane_count > 1)
        .collect();
    let count = children.iter().map(|t| t.lane_count).max()?.min(64);
    // Each child's items, lane by lane, in one pass over them.
    let lanes = usize::try_from(count).unwrap_or(64);
    let mut by_lane: Vec<Vec<Vec<&Item>>> = vec![vec![Vec::new(); children.len()]; lanes];
    for (k, child) in children.iter().enumerate() {
        for item in project.lane(&child.guid) {
            let at = usize::try_from(item.fixed_lane.unwrap_or(0)).unwrap_or(usize::MAX);
            if let Some(lane) = by_lane.get_mut(at) {
                lane[k].push(item);
            }
        }
    }
    let mut laned: Vec<(u32, Span)> = Vec::new();
    for (at, on_lane) in (0..count).zip(&by_lane) {
        laned.extend(spans_of(on_lane).into_iter().map(|span| (at, span)));
    }
    (!laned.is_empty()).then(|| laned_fold(folder, laned))
}

/// [`fold_of`] lane by lane again after only the items on `lanes` changed
/// under `folder`: `was` — its fold before, its seams `joined` or not as
/// they are to be now — kept on every other lane, those folded again. An
/// edit to a kit's EDIT lane folds that lane, not the thirty-two beside it.
/// `None` when no lane holds anything.
#[must_use]
pub fn refold_lanes_of(
    project: &super::project::Project,
    tracks: &[daw_proto::Track],
    folder: &daw_proto::Track,
    (was, lanes): (&Fold, &std::collections::BTreeSet<u32>),
    joined: bool,
) -> Option<Fold> {
    let children: Vec<&daw_proto::Track> = under(tracks, &folder.guid)
        .into_iter()
        .filter(|t| !t.is_folder && t.lane_count > 1)
        .collect();
    let count = children.iter().map(|t| t.lane_count).max()?.min(64);
    let mut by_lane: std::collections::BTreeMap<u32, Vec<Span>> = std::collections::BTreeMap::new();
    for (item, span) in was.items.iter().zip(&was.spans) {
        let at = item.fixed_lane.unwrap_or(0);
        if !lanes.contains(&at) {
            by_lane.entry(at).or_default().push(span.clone());
        }
    }
    for &at in lanes.iter().filter(|at| **at < count) {
        let on_lane: Vec<Vec<&Item>> = children
            .iter()
            .map(|child| {
                project
                    .lane(&child.guid)
                    .iter()
                    .filter(|item| item.fixed_lane.unwrap_or(0) == at)
                    .collect()
            })
            .collect();
        let spans = spans_of(&on_lane);
        let spans = if joined { join(&spans) } else { spans };
        if !spans.is_empty() {
            by_lane.insert(at, spans);
        }
    }
    let laned: Vec<(u32, Span)> = by_lane
        .into_iter()
        .flat_map(|(at, spans)| spans.into_iter().map(move |span| (at, span)))
        .collect();
    if laned.is_empty() {
        return None;
    }
    let mut fold = laned_fold(folder, laned);
    fold.gaps = gaps(
        children
            .iter()
            .map(|child| project.lane(&child.guid).iter()),
    );
    Some(fold)
}

/// Spans, each on its lane, as a fold: [`lane`]'s items with their lane
/// set, so a row showing lanes puts each in its lane's strip.
fn laned_fold(folder: &daw_proto::Track, laned: Vec<(u32, Span)>) -> Fold {
    let (lanes, spans): (Vec<u32>, Vec<Span>) = laned.into_iter().unzip();
    let mut items = lane(folder, &spans);
    for (item, at) in items.iter_mut().zip(lanes) {
        item.fixed_lane = Some(at);
    }
    Fold {
        items,
        spans,
        gaps: Vec::new(),
    }
}

/// `spans` with the seams joined.
///
/// Spans that touch or overlap — a cut, a crossfade — are one, so a take
/// edited into slices reads as the take it still is. A gap between them,
/// where nothing sounds, still parts them.
///
/// What a folder row shows when the edits are not what is being looked
/// at (mixing, playing live). A joined span is whole only when it was
/// one span already: its children's edges are not its own.
#[must_use]
pub fn join(spans: &[Span]) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    // What the last span is already folded from, to ask in one step: a
    // take cut into slices joins into one span from every slice, and a
    // scan of `from` for each was quadratic — 26 000 slices of a kit,
    // three seconds a replan.
    let mut has: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for span in spans {
        match out.last_mut() {
            Some(last) if span.start <= last.end + GRAIN => {
                last.end = last.end.max(span.end);
                for guid in &span.from {
                    if has.insert(guid.as_str()) {
                        last.from.push(guid.clone());
                    }
                }
                last.whole = false;
            }
            _ => {
                has.clear();
                has.extend(span.from.iter().map(String::as_str));
                out.push(span.clone());
            }
        }
    }
    out
}

/// Every fold on `project` with its seams joined ([`join`]): after
/// [`refold`], for a window not showing the cuts.
pub fn join_seams(project: &mut super::project::Project) {
    let folders: Vec<daw_proto::Track> = project
        .tracks
        .iter()
        .filter(|t| project.folds.contains_key(&t.guid))
        .cloned()
        .collect();
    for folder in folders {
        if let Some(fold) = project.folds.get_mut(&folder.guid) {
            *fold = joined(&folder, std::mem::take(fold));
        }
    }
}

/// `folder`'s `fold` with its seams joined ([`join`]): one fold of
/// [`join_seams`]. Folded lane by lane, each lane's seams joined on their
/// own, a take never run into the comp under it.
#[must_use]
pub fn joined(folder: &daw_proto::Track, fold: Fold) -> Fold {
    if fold.items.iter().any(|item| item.fixed_lane.is_some()) {
        let mut by_lane: std::collections::BTreeMap<u32, Vec<Span>> =
            std::collections::BTreeMap::new();
        for (item, span) in fold.items.iter().zip(fold.spans) {
            by_lane
                .entry(item.fixed_lane.unwrap_or(0))
                .or_default()
                .push(span);
        }
        let laned = by_lane
            .into_iter()
            .flat_map(|(at, spans)| join(&spans).into_iter().map(move |span| (at, span)))
            .collect();
        let mut out = laned_fold(folder, laned);
        out.gaps = fold.gaps;
        return out;
    }
    let spans = join(&fold.spans);
    Fold {
        items: lane(folder, &spans),
        spans,
        gaps: fold.gaps,
    }
}

/// Every collapsed folder's folded lane, by folder guid.
///
/// `descendants` answers which tracks are under a folder — the caller's
/// job, because it is the caller that knows the tree and the collapse
/// state, and this module is meant to be testable with items written by
/// hand.
#[must_use]
pub fn lanes<'a>(
    folders: impl IntoIterator<Item = &'a daw_proto::Track>,
    descendants: &dyn Fn(&str) -> Vec<&'a [Item]>,
) -> HashMap<String, Vec<Item>> {
    folders
        .into_iter()
        .filter_map(|folder| {
            let children = descendants(&folder.guid);
            let spans = spans(&children);
            (!spans.is_empty()).then(|| (folder.guid.clone(), lane(folder, &spans)))
        })
        .collect()
}

/// One folded row: the boxes it shows, and what they were folded from.
///
/// Held together because they are two views of one answer and a window
/// that had one without the other would either draw items it could not
/// resolve or resolve items it did not draw.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Fold {
    /// What the row shows — real [`Item`]s, so everything that draws a
    /// lane draws these the same way.
    pub items: Vec<Item>,
    /// What each of them was folded from, in the same order.
    pub spans: Vec<Span>,
    /// Where a track under the row goes silent between two of its items
    /// on one lane, by start: what an edit may have torn, worked out
    /// once when the row is folded rather than every frame.
    pub gaps: Vec<Gap>,
}

/// Silence between two items one after another on one lane of one
/// track under a folded row.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Gap {
    /// The lane (`fixed_lane`) it is on.
    pub lane: u32,
    /// Where the item before ends, and the next starts, seconds.
    pub start: f64,
    pub end: f64,
}

/// The gaps in `children` (each one track's items): between two items one
/// after another on the same lane, ends a millisecond or less apart
/// meeting. By start.
#[must_use]
pub fn gaps<'a>(children: impl IntoIterator<Item = impl Iterator<Item = &'a Item>>) -> Vec<Gap> {
    /// Ends this close are one edge.
    const TOUCH: f64 = 1e-3;
    let mut out = Vec::new();
    for items in children {
        let mut by_lane: std::collections::BTreeMap<u32, Vec<(f64, f64)>> =
            std::collections::BTreeMap::new();
        for i in items {
            let at = i.position.as_seconds();
            by_lane
                .entry(i.fixed_lane.unwrap_or(0))
                .or_default()
                .push((at, at + i.length.as_seconds()));
        }
        for (lane, mut spans) in by_lane {
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut reach = f64::NEG_INFINITY;
            for (start, end) in spans {
                if reach.is_finite() && start > reach + TOUCH {
                    out.push(Gap {
                        lane,
                        start: reach,
                        end: start,
                    });
                }
                reach = reach.max(end);
            }
        }
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    out
}

/// What an edit aimed at an item on a folded row really is.
#[derive(Clone, PartialEq, Debug)]
pub enum Spread {
    /// Not a folded item at all. The edit means what it says.
    Direct,
    /// A folded item. The edit belongs to these real items instead —
    /// every one of them, as one step, because on screen they are one
    /// thing and a half-applied edit would be a lie about the take.
    Children(Vec<String>),
    /// A folded item the edit cannot honestly be applied to, and why.
    Refused(&'static str),
}

/// Where an edit aimed at `guid` should actually land.
///
/// `destructive` is whether the edit CHANGES the items — a move, a trim,
/// a split, a delete — as against merely selecting them. The distinction
/// is the whole of the ragged case: selecting every mic under a fragment
/// is unambiguous and harmless, and moving them is neither.
///
/// # Why a fragment is refused
///
/// A span with [`Span::whole`] false exists because the children
/// disagree — one mic was punched in, or a trigger lane was trimmed on
/// its own. Its edges are not any child's edges. Dragging it would have
/// to mean one of three things and there is no way to tell which:
/// move every child (which moves audio outside the fragment the hand
/// took hold of), move only the children that start there (which tears
/// the take apart), or move nothing and pretend.
///
/// So it is refused, with a sentence saying so. A fragment is EVIDENCE
/// of an edit somebody already made, and the honest answer is to open
/// the folder and work on the mic that disagrees.
#[must_use]
pub fn spread(
    folds: &std::collections::HashMap<String, Fold>,
    guid: &str,
    destructive: bool,
) -> Spread {
    let Some((folder, index)) = parse_guid(guid) else {
        return Spread::Direct;
    };
    let Some(span) = folds.get(folder).and_then(|fold| fold.spans.get(index)) else {
        // A folded guid whose fold is gone: the view changed under the
        // gesture. Refusing beats guessing — the alternative is writing
        // an edit addressed to nothing.
        return Spread::Refused("that row was re-folded while you were working on it");
    };
    if destructive && !span.whole {
        return Spread::Refused(
            "the takes under this row do not line up here, so there is no one edit to make — \
             open the folder and work on the track that differs",
        );
    }
    Spread::Children(span.from.clone())
}

/// Every track under `folder`, however deep.
#[must_use]
pub fn under<'a>(tracks: &'a [daw_proto::Track], folder: &str) -> Vec<&'a daw_proto::Track> {
    // By parent links rather than by the depth deltas beside them: a
    // session that has been dragged about has correct parents and
    // occasionally a stale depth, and a wrong answer here puts another
    // drum's take on this drum's row.
    let mut inside: std::collections::HashSet<&str> = std::collections::HashSet::new();
    inside.insert(folder);
    let mut out = Vec::new();
    // One pass in project order works because a child never precedes
    // its parent in a session file.
    for track in tracks {
        let parent = track.parent_guid.as_deref();
        if parent.is_some_and(|p| inside.contains(p)) {
            inside.insert(&track.guid);
            out.push(track);
        }
    }
    out
}

/// The folders on screen whose contents are not.
///
/// A scene that collapses a folder drops its rows and leaves the folder
/// itself, which is how REAPER ends up drawing an empty lane where the
/// kit used to be. These are the rows that want filling.
#[must_use]
pub fn shut<'a>(
    tracks: &'a [daw_proto::Track],
    shown: &[daw_proto::Track],
) -> Vec<&'a daw_proto::Track> {
    let on_screen: std::collections::HashSet<&str> =
        shown.iter().map(|t| t.guid.as_str()).collect();
    shown
        .iter()
        .filter(|track| track.is_folder)
        .filter_map(|track| tracks.iter().find(|t| t.guid == track.guid))
        .filter(|folder| {
            let inside = under(tracks, &folder.guid);
            // A folder whose every child is hidden (a guitar's `Main` with
            // its DI nested under it and hidden) is not shut: it is a track,
            // its own items drawn, not a fold of what nobody is shown.
            inside.iter().any(|t| t.visible_in_tcp)
                && !inside.iter().any(|t| on_screen.contains(t.guid.as_str()))
        })
        .collect()
}

/// Fold several children's peaks into the one the folder's row shows.
///
/// The loudest of them at each point, not the mean. A mean of three
/// mics of one drum is quieter than any of them, which draws a kit as
/// though it had been recorded further away the more microphones were
/// on it; the maximum is never narrower than any child and is what the
/// arrangement's own folder items settled on for the same reason.
///
/// Resampled onto the longest child's grid, because the children are one
/// performance and their peak lists are the same performance at whatever
/// resolution each was read at.
#[must_use]
pub fn wave(children: &[&[f32]]) -> Vec<f32> {
    let width = children.iter().map(|c| c.len()).max().unwrap_or(0);
    if width == 0 {
        return Vec::new();
    }
    (0..width)
        .map(|i| {
            children
                .iter()
                .filter(|child| !child.is_empty())
                .map(|child| {
                    // Where this bin falls in a child of a different
                    // length: the same fraction of the performance.
                    let at = i
                        .saturating_mul(child.len())
                        .checked_div(width)
                        .unwrap_or(0)
                        .min(child.len().saturating_sub(1));
                    child.get(at).copied().unwrap_or(0.0)
                })
                .fold(0.0_f32, f32::max)
        })
        .collect()
}

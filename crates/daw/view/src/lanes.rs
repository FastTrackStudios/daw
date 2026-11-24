//! A track's fixed lanes, as the panel steps through them.
//!
//! REAPER 7 keeps a track's takes, its comp and its edit as lanes; one
//! plays. The panel's lane line names the playing lane and steps to the
//! lane before or after it — among the lanes worth stepping to: those
//! holding items or named for what they are (`COMP`, `EDIT`), never the
//! empty numbered ones a recording leaves between them.

/// The comp's lane name.
pub const COMP: &str = "COMP";
/// The edit's lane name.
pub const EDIT: &str = "EDIT";

/// A lane by name on `track`.
#[must_use]
pub fn lane_named(track: &daw_proto::Track, name: &str) -> Option<u32> {
    track
        .lane_names
        .iter()
        .position(|n| n.trim().eq_ignore_ascii_case(name.trim()))
        .and_then(|i| u32::try_from(i).ok())
        .filter(|&i| i < track.lane_count)
}

/// The lanes of `track` worth stepping to, in order: named, or holding
/// one of `items`.
#[must_use]
pub fn steps(track: &daw_proto::Track, items: &[daw_proto::Item]) -> Vec<u32> {
    (0..track.lane_count.min(64))
        .filter(|&lane| {
            let named = name(track, lane).is_some();
            named || items.iter().any(|i| i.fixed_lane.unwrap_or(0) == lane)
        })
        .collect()
}

/// Lane `lane`'s name, when it has one other than its number.
#[must_use]
pub fn name(track: &daw_proto::Track, lane: u32) -> Option<&str> {
    let name = track.lane_names.get(usize::try_from(lane).ok()?)?.trim();
    (!name.is_empty() && name.parse::<u32>().is_err()).then_some(name)
}

/// The lane that plays (the first, when several do).
#[must_use]
pub fn playing(track: &daw_proto::Track) -> Option<u32> {
    (0..track.lane_count.min(64)).find(|lane| track.lane_play_mask & (1_u64 << lane) != 0)
}

/// What the lane line shows: the playing lane's name, or its number
/// from 1 as REAPER numbers lanes.
#[must_use]
pub fn label(track: &daw_proto::Track) -> String {
    playing(track).map_or_else(
        || "-".to_owned(),
        |lane| name(track, lane).map_or_else(|| lane.saturating_add(1).to_string(), str::to_owned),
    )
}

/// The play mask after a step forward (`next`) or back from the playing
/// lane, round the ends; `None` with nowhere else to go.
#[must_use]
pub fn stepped(track: &daw_proto::Track, items: &[daw_proto::Item], next: bool) -> Option<u64> {
    let steps = steps(track, items);
    if steps.len() < 2 {
        return None;
    }
    let at = playing(track)
        .and_then(|lane| steps.iter().position(|&s| s == lane))
        .unwrap_or(0);
    let count = steps.len();
    let to = if next {
        at.saturating_add(1).checked_rem(count)?
    } else {
        at.checked_sub(1).unwrap_or_else(|| count.saturating_sub(1))
    };
    Some(1_u64 << steps.get(to)?)
}

/// The other tracks `guid`'s media-edit group moves with it, while
/// grouping is on: those following the group it leads.
#[must_use]
pub fn grouped_tracks(tracks: &[daw_proto::Track], guid: &str) -> Vec<String> {
    if !crate::options::GROUPING.get() {
        return Vec::new();
    }
    let Some(lead) = tracks
        .iter()
        .find(|t| t.guid == guid)
        .map(|t| t.grouping.media_edit_lead)
        .filter(|lead| *lead != 0)
    else {
        return Vec::new();
    };
    tracks
        .iter()
        .filter(|t| t.guid != guid && t.grouping.media_edit_follow & lead != 0)
        .map(|t| t.guid.clone())
        .collect()
}

/// The tracks a row's lane line speaks for: a track with lanes itself; a
/// folder the tracks under it that have lanes (all of a kit's mics, say);
/// none for a track with one lane.
#[must_use]
pub fn shown_on<'a>(
    tracks: &'a [daw_proto::Track],
    track: &'a daw_proto::Track,
) -> Vec<&'a daw_proto::Track> {
    if !track.is_folder {
        return if track.lane_count > 1 {
            vec![track]
        } else {
            Vec::new()
        };
    }
    tracks
        .iter()
        .filter(|t| t.lane_count > 1 && !t.is_folder && is_under(tracks, t, &track.guid))
        .collect()
}

/// Whether `track` is somewhere inside the folder `folder`.
fn is_under(tracks: &[daw_proto::Track], track: &daw_proto::Track, folder: &str) -> bool {
    let mut at = track.parent_guid.as_deref();
    for _ in 0..tracks.len() {
        match at {
            Some(guid) if guid == folder => return true,
            Some(guid) => {
                at = tracks
                    .iter()
                    .find(|t| t.guid == guid)
                    .and_then(|t| t.parent_guid.as_deref());
            }
            None => return false,
        }
    }
    false
}

/// What a lane line over `tracks` names: the lane they all play, or
/// `mixed` when they differ.
#[must_use]
pub fn label_of(tracks: &[&daw_proto::Track]) -> String {
    let mut names = tracks.iter().map(|t| label(t));
    let Some(first) = names.next() else {
        return String::new();
    };
    if names.all(|n| n == first) {
        first
    } else {
        "mixed".to_owned()
    }
}

/// The play mask for `track` that plays its lane called `name`.
#[must_use]
pub fn mask_named(track: &daw_proto::Track, name: &str) -> Option<u64> {
    (0..track.lane_count.min(64))
        .find(|&lane| name_or_number(track, lane) == name)
        .map(|lane| 1_u64 << lane)
}

fn name_or_number(track: &daw_proto::Track, lane: u32) -> String {
    name(track, lane).map_or_else(|| lane.saturating_add(1).to_string(), str::to_owned)
}

/// The tracks that show all their lanes; the rest show only the one that
/// plays. The window's own, as its folds are (`crate::folds`): which lanes
/// a person is looking at, not the song's.
#[cfg(not(test))]
fn expanded_set() -> std::sync::MutexGuard<'static, std::collections::HashSet<String>> {
    static EXPANDED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::LazyLock::new(Default::default);
    EXPANDED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// Under test, each thread's own: a test opening lanes must not open the
// rows another test is planning beside it.
#[cfg(test)]
thread_local! {
    static EXPANDED: &'static std::sync::Mutex<std::collections::HashSet<String>> =
        Box::leak(Box::default());
}

#[cfg(test)]
fn expanded_set() -> std::sync::MutexGuard<'static, std::collections::HashSet<String>> {
    EXPANDED
        .with(|set| *set)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether `guid` shows all its lanes (it shows the one that plays until
/// expanded).
#[must_use]
pub fn expanded(guid: &str) -> bool {
    expanded_set().contains(guid)
}

/// Show all of each of `guids`' lanes, or only the playing one again.
///
/// `Some(true)` shows all, `Some(false)` the playing one; `None` toggles — all expanded if any was
/// not, else all collapsed. Whether they show all their lanes now. The
/// rows are planned again.
#[must_use = "whether they show all their lanes now"]
pub fn set_expanded(guids: &[String], open: Option<bool>) -> bool {
    let open = {
        let mut set = expanded_set();
        let open = open.unwrap_or_else(|| guids.iter().any(|g| !set.contains(g)));
        for g in guids {
            if open {
                set.insert(g.clone());
            } else {
                set.remove(g);
            }
        }
        open
    };
    crate::folds::touch();
    open
}

/// What a row's lane line speaks for.
///
/// The tracks shown that it does ([`shown_on`] over `tracks`, live as the panel has them), else those
/// the scene recorded for its row (`recorded`) — a shut folder's, which
/// are under it and not rows.
#[must_use]
pub fn speaks_for<'a>(
    tracks: &'a [daw_proto::Track],
    track: &'a daw_proto::Track,
    recorded: &'a [daw_proto::Track],
) -> Vec<&'a daw_proto::Track> {
    let shown = shown_on(tracks, track);
    if shown.is_empty() {
        recorded.iter().collect()
    } else {
        shown
    }
}

/// What a row's `+` opens.
///
/// The track itself, and the tracks its lane line
/// speaks for (`under`) — on a folder, every track under it with lanes, so
/// the folder's row shows its lanes shut and each of them shows its own
/// open. Empty when it speaks for none.
#[must_use]
pub fn opened_by(track: &daw_proto::Track, under: &[&daw_proto::Track]) -> Vec<String> {
    if under.is_empty() {
        return Vec::new();
    }
    let mut out = vec![track.guid.clone()];
    for t in under {
        if !out.contains(&t.guid) {
            out.push(t.guid.clone());
        }
    }
    out
}

/// What a row's lanes button opens or shuts: the track and the tracks its
/// lane line speaks for (`under`, [`opened_by`]) — and every folder
/// between them and it, by `tracks`' parents.
///
/// A shut folder's row shows its lanes only when the folder itself is
/// opened ([`folder_view`]): the button on an open kit whose pieces are
/// shut opened the mics and the kit, and every piece's shut row went on
/// showing what plays — its EDIT lane filling the row — until the kit was
/// shut over them.
#[must_use]
pub fn opened_with_folders(
    tracks: &[daw_proto::Track],
    track: &daw_proto::Track,
    under: &[&daw_proto::Track],
) -> Vec<String> {
    let mut opened = opened_by(track, under);
    let parent_of = |guid: &str| {
        tracks
            .iter()
            .find(|t| t.guid == guid)
            .and_then(|t| t.parent_guid.as_deref())
    };
    for lane_track in under {
        let mut at = lane_track.parent_guid.as_deref();
        while let Some(folder) = at.filter(|folder| *folder != track.guid) {
            if !opened.iter().any(|g| g == folder) {
                opened.push(folder.to_owned());
            }
            at = parent_of(folder);
        }
    }
    opened
}

/// Whether the row's `+` shows its lanes open: the track's own state.
#[must_use]
pub fn opened(track: &daw_proto::Track) -> bool {
    expanded(&track.guid)
}

/// A folder as its shut row shows its lanes, while they are open.
///
/// The folder with the lanes of the track under it that has the most (a kit's
/// mics are recorded alike, lane for lane) — its count, names and the
/// lane that plays. `None` for a folder showing only what plays, one
/// whose tracks have one lane, or a track that is not a folder.
///
/// What the folder's row is folded by (`folded::refold_lanes`), drawn by
/// and swiped on, so the three agree on which lane is where.
#[must_use]
pub fn folder_view(
    tracks: &[daw_proto::Track],
    folder: &daw_proto::Track,
) -> Option<daw_proto::Track> {
    if !folder.is_folder || !expanded(&folder.guid) {
        return None;
    }
    let most = shown_on(tracks, folder)
        .into_iter()
        .max_by_key(|t| t.lane_count)?;
    Some(daw_proto::Track {
        lane_count: most.lane_count,
        lane_names: most.lane_names.clone(),
        lane_play_mask: most.lane_play_mask,
        lane_display: most.lane_display,
        ..folder.clone()
    })
}

/// Lane `lane`'s name as written, else its number from 1.
///
/// A take's number (`2`) as much as `C1`; else its number from 1 as REAPER numbers lanes: how a lane is found
/// across the tracks of a kit ([`lane_called`]), whose lane numbers can
/// differ where their names do not.
#[must_use]
pub fn called(track: &daw_proto::Track, lane: u32) -> String {
    track
        .lane_names
        .get(usize::try_from(lane).unwrap_or(usize::MAX))
        .map(String::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map_or_else(|| lane.saturating_add(1).to_string(), str::to_owned)
}

/// The lane of `track` called `name`: by its name (any case), else by its
/// number from 1.
#[must_use]
pub fn lane_called(track: &daw_proto::Track, name: &str) -> Option<u32> {
    let want = name.trim();
    (0..track.lane_count.min(64))
        .find(|&lane| {
            track
                .lane_names
                .get(usize::try_from(lane).unwrap_or(usize::MAX))
                .is_some_and(|n| n.trim().eq_ignore_ascii_case(want))
        })
        .or_else(|| {
            want.parse::<u32>()
                .ok()
                .and_then(|n| n.checked_sub(1))
                .filter(|&lane| lane < track.lane_count.min(64))
        })
}

/// The lane a track's edits are made on: its `EDIT` lane, once it has
/// been given one — whatever lane is playing, so a comp is never cut
/// while it is being listened to.
#[must_use]
pub fn edit_lane(track: &daw_proto::Track) -> Option<u32> {
    track
        .lane_names
        .iter()
        .position(|n| n.trim().eq_ignore_ascii_case("EDIT"))
        .and_then(|i| u32::try_from(i).ok())
        .filter(|&i| i < track.lane_count)
}

/// Whether `item` is one of the takes an edit of `track` works on: on its
/// `EDIT` lane if it has one, else on a lane that plays.
#[must_use]
pub fn is_edited(track: &daw_proto::Track, item: &daw_proto::Item) -> bool {
    edit_lane(track).map_or_else(
        || crate::song::project::on_playing_lane(track, item),
        |lane| item.fixed_lane.unwrap_or(0) == lane,
    )
}

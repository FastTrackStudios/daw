//! Routing-graph analysis run once per snapshot (not per block):
//! topological processing order + solo routing mask.

use std::collections::VecDeque;

use super::snapshot::TrackSnapshot;

/// Topological processing order over the ROUTING GRAPH: a track must
/// be fully processed before anything it feeds — its folder parent
/// (children sum upward) and every send destination (busses receive
/// post-fader signal). Depth ordering alone breaks bus mixes: a send
/// landing on an already-processed bus would never be forwarded
/// (the session's whole drum mix flows Drums → DRUM BUS → MIX BUS).
/// Kahn's algorithm; any cycle remainder (feedback loops) appends in
/// index order.
pub(crate) fn topo_order(tracks: &[TrackSnapshot]) -> Vec<usize> {
    let n = tracks.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    let mut edge = |a: usize, b: usize| {
        if a != b {
            adj[a].push(b);
            indeg[b] += 1;
        }
    };
    for (i, t) in tracks.iter().enumerate() {
        if let Some(pi) = t.parent_idx {
            edge(i, pi);
        }
        for snd in &t.sends {
            if let Some(di) = snd.dest_idx {
                edge(i, di);
            }
        }
    }
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut queue: VecDeque<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    while let Some(i) = queue.pop_front() {
        order.push(i);
        for &j in &adj[i] {
            indeg[j] -= 1;
            if indeg[j] == 0 {
                queue.push_back(j);
            }
        }
    }
    if order.len() < n {
        for i in 0..n {
            if indeg[i] > 0 {
                order.push(i);
            }
        }
    }
    order
}

/// Solo routing: which tracks keep passing audio while something is
/// soloed. `None` when nothing is (everything passes). See
/// [`solo_mask`] for the rules.
pub(crate) fn solo_pass(tracks: &[TrackSnapshot]) -> Option<Vec<bool>> {
    if !tracks.iter().any(|t| t.soloed) {
        return None;
    }
    let parents: Vec<Option<usize>> = tracks.iter().map(|t| t.parent_idx).collect();
    let sends: Vec<Vec<usize>> = tracks
        .iter()
        .map(|t| {
            t.sends
                .iter()
                .filter(|snd| !snd.muted)
                .filter_map(|snd| snd.dest_idx)
                .collect()
        })
        .collect();
    let soloed: Vec<bool> = tracks.iter().map(|t| t.soloed).collect();
    Some(solo_mask(&parents, &sends, &soloed))
}

/// The solo mask over the routing graph, REAPER's way:
///
/// - a soloed track plays, and so does everything inside it (soloing a
///   folder solos its children);
/// - so does every track on the way from those to master: folder parents,
///   and the destinations of their (unmuted) sends, and onward from
///   there. A session whose stems reach the mix only through sends to
///   buses (parent send off) went silent when this stopped at the folder
///   parents: the bus was not in the mask, so it was skipped and the
///   soloed audio went nowhere. A track on that path is NOT soloed
///   itself: the other tracks that feed the same bus stay silent;
/// - and so does whatever sends INTO a soloed track (with its own
///   contents), so soloing a bus plays what feeds it.
pub(crate) fn solo_mask(
    parents: &[Option<usize>],
    sends: &[Vec<usize>],
    soloed: &[bool],
) -> Vec<bool> {
    let n = parents.len();
    // The tracks the solo is ABOUT: the soloed ones, plus whatever sends
    // into them (repeatedly: a stem into a bus into a soloed mix bus),
    // plus everything inside any of those.
    let mut source = soloed.to_vec();
    let mut changed = true;
    while changed {
        changed = false;
        for i in 0..n {
            if !source[i] && sends[i].iter().any(|&d| d < n && source[d]) {
                source[i] = true;
                changed = true;
            }
        }
    }
    let inside = |j: usize, of: &[bool]| {
        let mut cur = j;
        for _ in 0..64 {
            match parents[cur] {
                Some(p) if p < n => {
                    if of[p] {
                        return true;
                    }
                    cur = p;
                }
                _ => return false,
            }
        }
        false
    };
    let sources: Vec<bool> = (0..n).map(|j| source[j] || inside(j, &source)).collect();

    // And everything downstream of them, which is the path to master.
    let mut pass = sources.clone();
    let mut queue: VecDeque<usize> = (0..n).filter(|&i| pass[i]).collect();
    while let Some(i) = queue.pop_front() {
        let onward = parents[i].into_iter().chain(sends[i].iter().copied());
        for d in onward {
            if d < n && !pass[d] {
                pass[d] = true;
                queue.push_back(d);
            }
        }
    }
    pass
}

#[cfg(test)]
mod tests {
    use super::solo_mask;

    /// The live-bus shape: stems inside instrument folders with their
    /// parent send off, reaching the mix only through a send to a bus in
    /// a separate BUSES folder.
    ///
    /// 0 Drums (folder)   1 Kick -> 5   2 Snare -> 5
    /// 3 Bass  (folder)   4 Bass -> 6
    /// 7 BUSES (folder)   5 Drum bus    6 Bass bus
    fn live_bus() -> (Vec<Option<usize>>, Vec<Vec<usize>>) {
        let parents = vec![None, Some(0), Some(0), None, Some(3), Some(7), Some(7), None];
        let sends = vec![
            vec![],
            vec![5],
            vec![5],
            vec![],
            vec![6],
            vec![],
            vec![],
            vec![],
        ];
        (parents, sends)
    }

    fn solo(which: &[usize]) -> Vec<bool> {
        let (parents, sends) = live_bus();
        let mut soloed = vec![false; parents.len()];
        for &i in which {
            soloed[i] = true;
        }
        solo_mask(&parents, &sends, &soloed)
    }

    fn passing(mask: &[bool]) -> Vec<usize> {
        (0..mask.len()).filter(|&i| mask[i]).collect()
    }

    #[test]
    fn a_soloed_stem_reaches_master_through_its_send() {
        // Kick, its folder, the drum bus and the BUSES folder, and NOT the
        // snare that shares the bus.
        assert_eq!(passing(&solo(&[1])), vec![0, 1, 5, 7]);
    }

    #[test]
    fn a_soloed_folder_plays_its_stems_through_their_buses() {
        assert_eq!(passing(&solo(&[0])), vec![0, 1, 2, 5, 7]);
    }

    #[test]
    fn a_soloed_bus_plays_what_feeds_it() {
        // Drum bus: the kick and snare that send to it, their folder, the
        // bus and its folder; not the bass.
        assert_eq!(passing(&solo(&[5])), vec![0, 1, 2, 5, 7]);
    }

    #[test]
    fn a_muted_send_is_not_a_route() {
        let (parents, mut sends) = live_bus();
        sends[1].clear(); // the kick's send, muted (filtered by solo_pass)
        let mut soloed = vec![false; parents.len()];
        soloed[1] = true;
        assert_eq!(passing(&solo_mask(&parents, &sends, &soloed)), vec![0, 1]);
    }
}

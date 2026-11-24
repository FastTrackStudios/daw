//! The stretch arithmetic an edit of a slice runs on — the engine's
//! placement (`crate::hit_place`), the quantize (`crate::actions`) and the
//! arrangement's drags (`crate::arrange_edit`) alike: drawing a slice out,
//! squeezing it, stretching around a moved hit, finding a track's comp lane.
//! Nothing here draws.

/// The fastest a stretch plays a take, as a rate: a hit dragged later
/// squeezes the audio before the next one, at most this much.
pub const STRETCH_MOST_RATE: f64 = 4.0;

/// A slice as a stretch edits it.
///
/// Where it plays, how long, and how it plays its take — `(played,
/// source)` seconds, played counted from the slice's start (its stretch
/// markers, or the two ends of it as recorded).
#[derive(Clone, Debug, PartialEq)]
pub struct Warped {
    pub position: f64,
    pub length: f64,
    pub markers: Vec<(f64, f64)>,
}

/// Where `markers` play the take at `t` (played seconds): between two,
/// on the line through them; past either end, at speed.
pub fn source_at(markers: &[(f64, f64)], t: f64) -> f64 {
    let (Some(&(p0, s0)), Some(&(pn, sn))) = (markers.first(), markers.last()) else {
        return t;
    };
    if t <= p0 {
        return s0 - (p0 - t);
    }
    if t >= pn {
        return sn + (t - pn);
    }
    markers
        .windows(2)
        .find_map(|pair| {
            let [(a, sa), (b, sb)] = pair else {
                return None;
            };
            (t >= *a && t <= *b && b > a).then(|| sa + (t - a) / (b - a) * (sb - sa))
        })
        .unwrap_or(t)
}

/// `w` played over `length` instead: everything up to `keep` (its attack)
/// as it was, the rest of its markers spread or squeezed to fit. `None`
/// when that leaves it no longer than its attack, or plays any of it
/// slower than [`TAIL_LEAST_RATE`] or faster than [`STRETCH_MOST_RATE`].
pub fn rescaled(w: &Warped, keep: f64, length: f64) -> Option<Warped> {
    /// The least a slice keeps after its attack, seconds.
    const LEAST: f64 = 0.002;
    let keep = keep.clamp(0.0, w.length);
    if length < keep + LEAST || w.length < keep + LEAST {
        return None;
    }
    let mut markers: Vec<(f64, f64)> = w.markers.clone();
    if !markers.iter().any(|(p, _)| (p - keep).abs() < 1e-9) {
        markers.push((keep, source_at(&w.markers, keep)));
        markers.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    let scale = (length - keep) / (w.length - keep);
    let markers: Vec<(f64, f64)> = markers
        .into_iter()
        .map(|(p, s)| {
            if p <= keep + 1e-9 {
                (p, s)
            } else {
                ((p - keep).mul_add(scale, keep), s)
            }
        })
        .collect();
    let fits = markers.windows(2).all(|pair| {
        let [(a, sa), (b, sb)] = pair else {
            return true;
        };
        if b - a <= 1e-9 {
            return true;
        }
        let rate = (sb - sa) / (b - a);
        (TAIL_LEAST_RATE - 1e-9..=STRETCH_MOST_RATE + 1e-9).contains(&rate)
    });
    fits.then_some(Warped {
        position: w.position,
        length,
        markers,
    })
}

/// A hit moved `by` seconds by stretching, not slipping.
///
/// `before` (the slice before it) is drawn out or squeezed to end where
/// it now starts, `hit` (its slice) starts `by` later and is squeezed or
/// drawn out so it still ends where it did — the hits either side stay
/// where they play, and each slice's attack (`attacks`: how far into each
/// its hit is, played seconds) and the [`STRETCH_KEEP`] after it play as
/// recorded. `None` when either would leave its rates ([`rescaled`]).
#[must_use]
pub fn stretch_around(
    before: &Warped,
    hit: &Warped,
    attacks: (f64, f64),
    by: f64,
) -> Option<(Warped, Warped)> {
    let drawn = rescaled(before, attacks.0 + STRETCH_KEEP, before.length + by)?;
    let mut moved = rescaled(hit, attacks.1 + STRETCH_KEEP, hit.length - by)?;
    moved.position = hit.position + by;
    Some((drawn, moved))
}

/// The comp lane of a track with fixed lanes: the first named `C…` (REAPER
/// 7's `C1`) or `COMP` (the kit's, `drum_lanes::COMP`), else the first
/// lane that plays. `None` for a track without lanes.
#[must_use]
pub fn comp_lane(track: &daw_proto::Track) -> Option<u32> {
    if track.lane_count == 0 {
        return None;
    }
    let named = track.lane_names.iter().position(|name| {
        name.trim().eq_ignore_ascii_case("COMP")
            || name
                .strip_prefix('C')
                .is_some_and(|rest| rest.is_empty() || rest.chars().all(|c| c.is_ascii_digit()))
    });
    named.and_then(|lane| u32::try_from(lane).ok()).or_else(|| {
        (0..track.lane_count.min(64)).find(|lane| track.lane_play_mask & (1_u64 << lane) != 0)
    })
}

/// How much of a slice's start a stretch leaves as it was, seconds: its
/// attack — the part of a hit an ear times it by.
pub const STRETCH_KEEP: f64 = 0.030;

/// The slowest a stretched decay plays, as a rate: slower, the pitch it
/// takes on (the engine stretches by rate) is heard — a slice slowed to
/// a quarter of its speed took the overheads' high end with it.
pub const STRETCH_LEAST_RATE: f64 = 0.85;

/// The slowest the space between hits — a slice's far tail, past its
/// hit — is drawn out, as a rate: past this there is too little left to
/// draw, and the gap stays.
pub const TAIL_LEAST_RATE: f64 = 0.1;

/// `item` drawn out to end at `to` (or as near as it can), where
/// [`STRETCH_LEAST_RATE`] cannot reach all the way: its attack as it was,
/// the hit's body after it ([`STRETCH_BODY`], at most half the decay) at
/// that rate, and only the far tail — the space before the next hit —
/// slowed past it, as slow as it takes down to [`TAIL_LEAST_RATE`]. The
/// markers that say so; `None`, and the item left, when it has no decay
/// to draw.
pub fn draw_out_between(item: &mut daw_proto::Item, to: f64) -> Option<Vec<(f64, f64)>> {
    draw_out_tail(item, to, TAIL_LEAST_RATE)
}

/// [`draw_out_between`], its far tail slowed no further than `least` —
/// or, `least` zero, as slow as it takes: the last resort before a gap.
pub fn draw_out_tail(item: &mut daw_proto::Item, to: f64, least: f64) -> Option<Vec<(f64, f64)>> {
    use daw_proto::primitives::Duration;
    let length = item.length.as_seconds();
    let keep = STRETCH_KEEP.min(length / 2.0);
    let decay = length - keep;
    let body = STRETCH_BODY.min(decay / 2.0);
    let tail = decay - body;
    let body_played = body / STRETCH_LEAST_RATE;
    let farthest = if least > 0.0 {
        keep + body_played + tail / least
    } else {
        f64::INFINITY
    };
    let longer = (to - item.position.as_seconds()).min(farthest);
    if tail <= 0.0 || longer <= keep + body_played + tail {
        return None;
    }
    let offset = item.start_offset.as_seconds();
    item.length = Duration::from_seconds(longer);
    Some(vec![
        (0.0, offset),
        (keep, offset + keep),
        (keep + body_played, offset + keep + body),
        (longer, offset + length),
    ])
}

/// `item` drawn out to end at `to`: its first [`STRETCH_KEEP`] as it was,
/// the rest slowed to reach — the markers that say so, `(take time,
/// source time)`, and the item lengthened. `None`, and the item left, when
/// that would slow it below [`STRETCH_LEAST_RATE`].
/// [`draw_out`] for the engine's own placement ([`crate::hit_place`]).
///
/// Whatever it takes to meet `to`: drawn out whole no slower than
/// [`STRETCH_LEAST_RATE`], else its far tail slower ([`draw_out_between`]),
/// else — a slice too short for either, two hits played closer than they
/// now sit — its tail as slow as it takes. A smeared few milliseconds of
/// decay is heard far less than every mic of the kit cutting out.
pub fn draw_out_markers(item: &mut daw_proto::Item, to: f64) -> Option<Vec<(f64, f64)>> {
    draw_out_reaching(item, to)
}

/// See [`draw_out_markers`].
pub fn draw_out_reaching(item: &mut daw_proto::Item, to: f64) -> Option<Vec<(f64, f64)>> {
    draw_out(item, to)
        .or_else(|| {
            let mut tried = item.clone();
            draw_out_between(&mut tried, to)
                .filter(|_| tried.position.as_seconds() + tried.length.as_seconds() >= to - 1e-9)
                .inspect(|_| *item = tried)
        })
        .or_else(|| draw_out_tail(item, to, 0.0))
}

pub fn draw_out(item: &mut daw_proto::Item, to: f64) -> Option<Vec<(f64, f64)>> {
    draw_out_to(item, to, STRETCH_LEAST_RATE)
}

/// [`draw_out`], slowing it no further than `least` of its rate.
pub fn draw_out_to(item: &mut daw_proto::Item, to: f64, least: f64) -> Option<Vec<(f64, f64)>> {
    use daw_proto::primitives::Duration;
    let length = item.length.as_seconds();
    let longer = to - item.position.as_seconds();
    let keep = STRETCH_KEEP.min(length / 2.0);
    let decay = length - keep;
    // A hair under `least` is `least`: drawn to exactly as far as the
    // rate reaches ([`farthest_drawn`]), the division lands just below.
    if longer <= length || decay <= 0.0 || decay / (longer - keep) < least - 1e-9 {
        return None;
    }
    let offset = item.start_offset.as_seconds();
    item.length = Duration::from_seconds(longer);
    Some(vec![
        (0.0, offset),
        (keep, offset + keep),
        (longer, offset + length),
    ])
}

/// How much of a slice's decay after its attack ([`STRETCH_KEEP`]) is
/// still the hit, seconds of its take: drawn out no slower than
/// [`STRETCH_LEAST_RATE`].
pub const STRETCH_BODY: f64 = 0.030;

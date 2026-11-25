//! What each track costs to render, for a performance meter: the time its
//! items took (media: sources read, resampled, stretched, faded) and the
//! time its FX took, summed over every block the device played and every
//! chunk rendered ahead. Written by the renders without locking or
//! allocating; a reader takes the sums twice and divides the difference
//! by the time between — REAPER's Media CPU and FX CPU columns.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// The most tracks a song's costs are kept for; past it a track's work is
/// still done, only not counted.
pub const TRACKS: usize = 4096;

/// Each track's media and FX time so far, by project track index.
#[derive(Debug)]
pub struct TrackCosts {
    /// Per track: media µs, FX µs, its plugins' blocks called and not
    /// called for being asleep.
    slots: Box<[[AtomicU64; 4]]>,
}

impl Default for TrackCosts {
    fn default() -> Self {
        Self {
            slots: (0..TRACKS)
                .map(|_| std::array::from_fn(|_| AtomicU64::new(0)))
                .collect(),
        }
    }
}

/// One track's media and FX time so far, µs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrackCost {
    pub media_us: u64,
    pub fx_us: u64,
    /// Its plugins' blocks called, and not called for being asleep
    /// ([`crate::plugin_sleep`]): `fx_slept / (fx_ran + fx_slept)` of an
    /// interval is the share of it its chain slept.
    pub fx_ran: u64,
    pub fx_slept: u64,
}

impl TrackCost {
    /// The share of its plugins' blocks between `before` and `self` that
    /// slept, 0..1 (0 with none).
    #[must_use]
    pub fn sleep_fraction(&self, before: &Self) -> f64 {
        let ran = self.fx_ran.saturating_sub(before.fx_ran);
        let slept = self.fx_slept.saturating_sub(before.fx_slept);
        if ran + slept == 0 {
            0.0
        } else {
            slept as f64 / (ran + slept) as f64
        }
    }
}

impl TrackCosts {
    /// `us` more of track `ti`'s items (realtime-safe).
    pub(crate) fn add_media(&self, ti: usize, us: u32) {
        if let Some(slot) = self.slots.get(ti) {
            slot[0].fetch_add(u64::from(us), Relaxed);
        }
    }

    /// `us` more of track `ti`'s FX (realtime-safe).
    pub(crate) fn add_fx(&self, ti: usize, us: u32) {
        if let Some(slot) = self.slots.get(ti) {
            slot[1].fetch_add(u64::from(us), Relaxed);
        }
    }

    /// `ran` and `slept` more of track `ti`'s plugin-blocks (realtime-safe).
    pub(crate) fn add_sleep(&self, ti: usize, ran: u16, slept: u16) {
        if let Some(slot) = self.slots.get(ti) {
            slot[2].fetch_add(u64::from(ran), Relaxed);
            slot[3].fetch_add(u64::from(slept), Relaxed);
        }
    }

    /// The first `n` tracks' sums so far.
    #[must_use]
    pub fn read(&self, n: usize) -> Vec<TrackCost> {
        self.slots
            .iter()
            .take(n)
            .map(|s| TrackCost {
                media_us: s[0].load(Relaxed),
                fx_us: s[1].load(Relaxed),
                fx_ran: s[2].load(Relaxed),
                fx_slept: s[3].load(Relaxed),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{TRACKS, TrackCost, TrackCosts};

    #[test]
    fn a_track_sums_its_media_and_fx_and_past_the_table_is_not_counted() {
        let costs = TrackCosts::default();
        costs.add_media(2, 300);
        costs.add_media(2, 200);
        costs.add_fx(2, 40);
        costs.add_fx(0, 7);
        costs.add_media(TRACKS, 1_000);
        assert_eq!(
            costs.read(3),
            vec![
                TrackCost {
                    media_us: 0,
                    fx_us: 7,
                    ..TrackCost::default()
                },
                TrackCost::default(),
                TrackCost {
                    media_us: 500,
                    fx_us: 40,
                    ..TrackCost::default()
                },
            ]
        );
    }
}

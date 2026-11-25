//! **A chain's plugins asleep** ([`crate::plugin_sleep`]): what one FX of a
//! node hears this block — its input silent or not, a note, what wakes it —
//! and what its block said, for its cell's [`SleepState`].
//!
//! A track whose bus stays clean and whose chain is asleep calls nothing:
//! every plugin is skipped, the bus stays zeros and unmarked, and the node
//! hands nothing on (its parent's sum stays clean too, so the flag travels
//! up the graph without a sample scanned). Realtime-safe: reads and hashes
//! only.
//!
//! [`SleepState`]: crate::plugin_sleep::SleepState

use super::envelope::{Automated, BlockClock, EnvelopeCursor};
use crate::plugin::PluginCell;
use crate::plugin_sleep::{self, Heard, Skip};

/// One FX's block, as its sleep sees it.
pub(super) struct Fx {
    pub(super) input_silent: bool,
    pub(super) events: bool,
    pub(super) key: u64,
}

impl Fx {
    /// What FX `cell` hears this block: the bus (`dirty` says whether it
    /// may hold anything at all) and the sidechain (`None` when nothing
    /// keyed it), its notes, and its parameters — `fixed` values and
    /// automation over `clock`'s block.
    pub(super) fn hear(
        cell: &PluginCell,
        dirty: bool,
        bus: &[f32],
        sidechain: Option<&[f32]>,
        notes: bool,
        fixed: &[(u32, f64)],
        envelopes: &[Automated],
        clock: BlockClock,
    ) -> Self {
        let zero = |s: &[f32]| s.iter().all(|x| *x == 0.0);
        let input_silent = (!dirty || zero(bus)) && sidechain.is_none_or(zero);
        let end = f64::from(clock.frames).mul_add(clock.seconds_per_frame, clock.start);
        // A point inside the block: the lane moves there (a ramp's start, a
        // step), however it reads at the block's ends.
        let moves = envelopes.iter().any(|a| {
            let i = a
                .points
                .partition_point(|p| p.time.as_seconds() < clock.start);
            a.points.get(i).is_some_and(|p| p.time.as_seconds() < end)
        });
        let values = fixed
            .iter()
            .flat_map(|&(id, v)| [u64::from(id), v.to_bits()])
            .chain(envelopes.iter().flat_map(|a| {
                let at = EnvelopeCursor::new(&a.points).eval_at(clock.start);
                [u64::from(a.id), at.map_or(u64::MAX, f64::to_bits)]
            }));
        Self {
            input_silent,
            events: notes || moves,
            key: plugin_sleep::key_of(cell.wake_key(), values),
        }
    }

    /// Whether to call it.
    pub(super) fn skip(&self, cell: &PluginCell) -> Skip {
        cell.sleep().skip(self.input_silent, self.events, self.key)
    }

    /// After it ran (`us` long) and wrote `out_l`/`out_r`: its status
    /// and tail to its sleep. Whether its output was exact zeros.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn ran(
        &self,
        cell: &PluginCell,
        plugin: &mut dyn crate::plugin::PluginInstance,
        out_l: &[f32],
        out_r: &[f32],
        latency: u32,
        us: u32,
    ) -> bool {
        let (zero, quiet) = plugin_sleep::silence(out_l, out_r);
        let status = plugin.process_status();
        if status == plugin_sleep::ProcessStatus::Continue {
            return zero;
        }
        let heard = Heard {
            input_silent: self.input_silent,
            events: self.events,
            output_quiet: quiet,
            frames: u32::try_from(out_l.len()).unwrap_or(u32::MAX),
        };
        cell.sleep()
            .ran(status, heard, || plugin.tail(), latency, self.key, us);
        zero
    }
}

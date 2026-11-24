//! Running an FX over one block with its automation: as parameter events at
//! their frames for a plugin that takes them, as each lane's value at the
//! block's start for one that does not.

use std::sync::atomic::{AtomicU64, Ordering};

use super::envelope::{self, Automated, BlockClock, Schedule};
use crate::plugin::{
    PluginError, PluginEvents, PluginInstance, PluginMidiEvent, PluginNoteExpression,
    PluginParamEvent, TimedPluginEvents,
};

/// One FX's parameter buffers, kept across blocks: allocated where the
/// render scratch is made, never grown on the audio thread.
pub(crate) struct FxParams {
    /// Block-start values, for a plugin without timed events.
    values: Vec<(u32, f64)>,
    /// The block's events, for a plugin with them.
    timed: Vec<PluginParamEvent>,
}

impl Default for FxParams {
    fn default() -> Self {
        Self {
            values: Vec::with_capacity(256),
            timed: Vec::with_capacity(envelope::FX_EVENT_CAPACITY),
        }
    }
}

/// The audio one FX reads and writes this block, de-interleaved.
pub(crate) struct Io<'a> {
    pub(crate) in_l: &'a [f32],
    pub(crate) in_r: &'a [f32],
    pub(crate) out_l: &'a mut [f32],
    pub(crate) out_r: &'a mut [f32],
}

/// What one FX is given this block besides its audio.
#[derive(Clone, Copy)]
pub(crate) struct FxBlock<'a> {
    /// Its stored parameter values.
    pub(crate) fixed: &'a [(u32, f64)],
    /// Its automated parameters.
    pub(crate) envelopes: &'a [Automated],
    pub(crate) clock: BlockClock,
    pub(crate) midi: &'a [PluginMidiEvent],
    pub(crate) note_expressions: &'a [PluginNoteExpression],
}

impl FxParams {
    /// Run `plugin` over one block. What its automation became, when it is
    /// played as timed events, comes back with the plugin's result.
    pub(crate) fn process(
        &mut self,
        plugin: &mut dyn PluginInstance,
        io: Io<'_>,
        block: &FxBlock<'_>,
    ) -> (Result<(), PluginError>, Option<Schedule>) {
        if block.envelopes.is_empty() {
            let events = PluginEvents {
                params: block.fixed,
                midi: block.midi,
                note_expressions: block.note_expressions,
            };
            return (
                plugin.process_block(io.in_l, io.in_r, io.out_l, io.out_r, &events),
                None,
            );
        }
        if plugin.takes_timed_params() {
            let schedule = envelope::schedule_params(
                block.fixed,
                block.envelopes,
                block.clock,
                &mut self.timed,
            );
            let events = TimedPluginEvents {
                params: &self.timed,
                midi: block.midi,
                note_expressions: block.note_expressions,
            };
            let result = plugin.process_block_timed(io.in_l, io.in_r, io.out_l, io.out_r, &events);
            return (result, Some(schedule));
        }
        envelope::params_at(
            block.fixed,
            block.envelopes,
            block.clock.start,
            &mut self.values,
        );
        let events = PluginEvents {
            params: &self.values,
            midi: block.midi,
            note_expressions: block.note_expressions,
        };
        (
            plugin.process_block(io.in_l, io.in_r, io.out_l, io.out_r, &events),
            None,
        )
    }
}

/// Automation events sent to plugins, and lanes cut short for want of room
/// (process-wide).
static EVENTS: AtomicU64 = AtomicU64::new(0);
static DROPPED: AtomicU64 = AtomicU64::new(0);

/// Count what one FX's block was sent.
pub(crate) fn record(schedule: Option<Schedule>) {
    let Some(s) = schedule else {
        return;
    };
    EVENTS.fetch_add(u64::from(s.events), Ordering::Relaxed);
    if s.dropped > 0 {
        DROPPED.fetch_add(u64::from(s.dropped), Ordering::Relaxed);
    }
}

/// How many automation events plugins have been sent, and how many lanes
/// were cut short because a block's events did not fit (process-wide).
#[must_use]
pub fn automation_events() -> (u64, u64) {
    (
        EVENTS.load(Ordering::Relaxed),
        DROPPED.load(Ordering::Relaxed),
    )
}

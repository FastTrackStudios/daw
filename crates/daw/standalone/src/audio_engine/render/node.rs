//! **One track of a block** — what the [scheduler](super::schedule) runs:
//! the track's media into its bus, the sums coming into it (pulled from
//! what feeds it, in the serial loop's order), its FX chain, fader, pan and
//! mute, its meter, taps and probes, and what it hands on — its parent's or
//! master's share and each send's, delayed to meet the rest of their sums
//! ([`super::pdc`]) — left in its own [`Edges`] for the receiver to pull.
//!
//! A node touches only its own track's buffers and its thread's
//! [`Worker`] scratch, and reads only finished tracks' edges, so any
//! number of them run at once. Realtime-safe: no lock but each plugin's own
//! (tried, never waited on), no allocation once the buffers have their
//! size.

#![allow(unsafe_code)]

use super::schedule::{Cells, Schedule};
use super::snapshot::TrackSnapshot;
use super::{
    AUX_PAIRS, EnvelopeCursor, PLUGIN_BUSY_SKIPS, PLUGIN_LOCK_SPINS, RenderSnapshot, Stamp,
    StereoBuffer, add_aux, automate, aux_sized, balance, collect_midi_events_into,
    collect_note_expressions, envelope, graph, item_mix::mix_item_into_bus, offline, part, pdc,
};

/// Where a track's parent share went this block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Share {
    #[default]
    Nowhere,
    Master,
    Parent(usize),
}

/// What a track hands on this block: edge 0 its parent (or master) share,
/// edge `k + 1` its send `k`, each `frames` interleaved stereo frames.
#[derive(Default)]
pub(super) struct Edges {
    pub(super) buf: Vec<f32>,
    /// Which edges carry something this block (the serial loop would have
    /// added them).
    pub(super) live: Vec<bool>,
    pub(super) share: Share,
}

impl Edges {
    fn reset(&mut self, edges: usize, frames: usize) {
        if self.buf.len() != edges * frames * 2 {
            self.buf.resize(edges * frames * 2, 0.0);
        }
        self.live.clear();
        self.live.resize(edges, false);
        self.share = Share::Nowhere;
    }

    pub(super) fn edge(&self, k: usize, frames: usize) -> &[f32] {
        &self.buf[k * frames * 2..(k + 1) * frames * 2]
    }

    fn edge_mut(&mut self, k: usize, frames: usize) -> &mut [f32] {
        &mut self.buf[k * frames * 2..(k + 1) * frames * 2]
    }
}

/// A rendering thread's scratch, sized for the block.
#[derive(Default)]
pub(super) struct Worker {
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    key_l: Vec<f32>,
    key_r: Vec<f32>,
    /// The track's post-fader signal (what its sends and parent take).
    src: Vec<f32>,
    pre_fx_tap: Vec<f32>,
    pre_fader_tap: Vec<f32>,
    delayed: Vec<f32>,
    fx_params: automate::FxParams,
    midi: Vec<crate::plugin::PluginMidiEvent>,
}

impl Worker {
    pub(super) fn size(&mut self, frames: usize) {
        for v in [
            &mut self.in_l,
            &mut self.in_r,
            &mut self.out_l,
            &mut self.out_r,
            &mut self.key_l,
            &mut self.key_r,
        ] {
            if v.len() != frames {
                v.resize(frames, 0.0);
            }
        }
        if self.delayed.len() != frames * 2 {
            self.delayed.resize(frames * 2, 0.0);
        }
    }
}

/// The clock of audio so many samples behind the block.
#[cfg(not(target_arch = "wasm32"))]
pub(super) type LateFn<'a> = dyn Fn(u32) -> crate::plugin::RenderTransport + Sync + 'a;
#[cfg(target_arch = "wasm32")]
pub(super) type LateFn<'a> = dyn Fn(u32) -> crate::plugin::RenderTransport + 'a;

/// What a node did, for the block's profile and the next block's schedule.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NodeStats {
    /// Its FX chain ran this long, µs (`None`: it has none, or it did not
    /// run).
    pub(super) fx_us: Option<u32>,
    pub(super) items_us: u32,
    /// A plugin panicked: its place in the chain, and whether in `prepare`.
    pub(super) panicked: Option<(usize, bool)>,
    /// Its plugins called, and not called for being asleep, this block,
    /// and about how long those would have run, µs ([`super::sleep`]).
    pub(super) fx_ran: u16,
    pub(super) fx_slept: u16,
    pub(super) slept_us: u32,
}

/// Everything a block's nodes share: read-only but for each track's own
/// cells.
pub(super) struct Block<'a> {
    pub(super) snap: &'a RenderSnapshot,
    pub(super) tracks: &'a [TrackSnapshot],
    pub(super) sched: &'a Schedule,
    pub(super) sample_rate: u32,
    pub(super) frames: usize,
    pub(super) rate: f64,
    pub(super) start_samples: f64,
    pub(super) start_seconds: f64,
    pub(super) end_seconds: f64,
    pub(super) frames_per_second: f64,
    pub(super) clock: envelope::BlockClock,
    pub(super) block_transport: crate::plugin::RenderTransport,
    pub(super) transport_late: &'a LateFn<'a>,
    pub(super) plan: &'a pdc::Plan,
    pub(super) latencies: &'a pdc::ChainLatencies,
    pub(super) plugins: Option<&'a crate::plugin::PluginMap>,
    pub(super) panicked_fx: &'a std::collections::HashSet<String>,
    pub(super) meters: &'a crate::metering::Meters,
    /// Rendered ahead: meters go into the feed.
    pub(super) feed_meter: Option<part::FeedMeter>,
    pub(super) costs: Option<&'a crate::audio_engine::perf::TrackCosts>,
    pub(super) offline: Option<&'a offline::Offline>,
    pub(super) tap_point: Option<offline::Tap>,
    pub(super) probing: bool,
    pub(super) asked: &'a [offline::ProbeAt],
    pub(super) probe_tracks: &'a [Option<usize>],
    pub(super) probes: Cells<Vec<f32>>,
    /// Each track's probes: `probe_list[probe_start[t]..probe_start[t + 1]]`.
    pub(super) probe_start: &'a [u32],
    pub(super) probe_list: &'a [u32],
    /// Recording ([`offline::Record`]): each probe's span buffer, and where
    /// this block lands in it (sample index, samples of the
    /// block skipped, samples recorded).
    pub(super) recordings: Cells<std::sync::Arc<[f32]>>,
    pub(super) record_at: Option<(usize, usize, usize)>,
    #[cfg(any(feature = "decode", feature = "audio"))]
    pub(super) live_taps: &'a super::live_tap::LiveTaps,
    #[cfg(any(feature = "decode", feature = "audio"))]
    pub(super) live_plan: [Option<(usize, offline::Probe)>; super::live_tap::SLOTS],
    pub(super) debug: bool,
    pub(super) buses: Cells<StereoBuffer>,
    pub(super) dirty: Cells<bool>,
    pub(super) sidechains: Cells<Vec<f32>>,
    pub(super) keyed: Cells<bool>,
    pub(super) aux: Cells<Vec<f32>>,
    pub(super) aux_live: Cells<bool>,
    pub(super) delays: Cells<Vec<pdc::Delay>>,
    pub(super) monitor_delays: Cells<pdc::Delay>,
    pub(super) taps: Cells<Vec<f32>>,
    pub(super) edges: Cells<Edges>,
    pub(super) midi_buckets: Cells<Vec<crate::plugin::PluginMidiEvent>>,
    pub(super) stats: Cells<NodeStats>,
    pub(super) workers: Cells<Worker>,
}

impl Block<'_> {
    /// Every node of the block, on `pool` and this thread (this thread
    /// alone without one), then the sums into the tracks no node renders
    /// here (another part's, which the feed hands on).
    pub(super) fn render(
        &self,
        #[cfg(not(target_arch = "wasm32"))] pool: Option<&super::parallel::LanePool>,
    ) {
        // SAFETY: the scheduler runs each node once, after every node that
        // feeds it, each thread in a slot of its own.
        let run_node = |ti: usize, slot: usize| unsafe { self.run(ti, slot) };
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(pool) = pool {
            self.sched.run_parallel(
                pool,
                &run_node,
                crate::plugin::RenderFrameGuard::current(),
                crate::plugin::rendering_stopped(),
            );
        } else {
            self.sched.run_serial(&run_node);
        }
        #[cfg(target_arch = "wasm32")]
        self.sched.run_serial(&run_node);
        for ti in 0..self.tracks.len() {
            if self.sched.pos[ti] == super::schedule::NOT_A_NODE {
                // SAFETY: every node is done; this thread alone is here.
                unsafe { self.gather(ti) };
            }
        }
    }

    fn pass(&self, ti: usize) -> graph::Pass {
        match &self.snap.solo_pass {
            Some(p) => p[ti],
            None => graph::Pass::On,
        }
    }

    /// Copy `bus` into every probe (and live tap) on track `ti` at `at`.
    fn probe(&self, ti: usize, at: offline::Probe, bus: &[f32]) {
        let mine = match (self.probe_start.get(ti), self.probe_start.get(ti + 1)) {
            (Some(&a), Some(&b)) => &self.probe_list[a as usize..b as usize],
            _ => &[],
        };
        for &k in mine {
            let k = k as usize;
            if self.asked[k].at != at {
                continue;
            }
            if let Some((start, skip, len)) = self.record_at {
                if k >= self.recordings.len() {
                    continue;
                }
                // SAFETY: a probe reads one track, and only that track's
                // node writes it.
                let rec = unsafe { self.recordings.at(k) };
                let Some(dst) = std::sync::Arc::get_mut(rec) else {
                    continue;
                };
                let skip = skip.min(bus.len());
                let n = len.min(bus.len() - skip);
                dst[start..start + n].copy_from_slice(&bus[skip..skip + n]);
            } else if k < self.probes.len() {
                // SAFETY: as above.
                let dst = unsafe { self.probes.at(k) };
                let n = dst.len().min(bus.len());
                dst[..n].copy_from_slice(&bus[..n]);
            }
        }
        #[cfg(any(feature = "decode", feature = "audio"))]
        for (slot, planned) in self.live_plan.iter().enumerate() {
            if *planned == Some((ti, at)) {
                self.live_taps
                    .write(slot, &bus[..(self.frames * 2).min(bus.len())]);
            }
        }
    }

    /// A track's post-fader peaks this block: onto its meter, or into the
    /// feed for the live render to put there when the block plays.
    fn meter(&self, ti: usize, post: &[f32]) {
        match &self.feed_meter {
            // SAFETY: only track `ti`'s node meters track `ti`.
            Some(feed) => unsafe { feed.meter(ti, post) },
            None => {
                let (l, r) = part::peak(post);
                super::meter_peaks(self.meters, ti, l, r);
            }
        }
    }

    /// Sum what feeds track `ti` into its bus and sidechain, in the order
    /// the serial loop added it. Every source is done.
    ///
    /// # Safety
    /// The caller owns track `ti`'s cells; its sources are finished.
    pub(super) unsafe fn gather(&self, ti: usize) {
        let frames = self.frames;
        // SAFETY: the caller's contract.
        let (bus, dirty, sidechain, keyed) = unsafe {
            (
                self.buses.at(ti),
                self.dirty.at(ti),
                self.sidechains.at(ti),
                self.keyed.at(ti),
            )
        };
        for e in self.sched.ins(ti) {
            let (s, k) = (e.src as usize, e.edge as usize);
            // SAFETY: a finished source's edges are read only.
            let from = unsafe { self.edges.get(s) };
            if !from.live.get(k).copied().unwrap_or(false) {
                continue;
            }
            let add = from.edge(k, frames);
            let to_sidechain = k > 0 && self.tracks[s].sends[k - 1].to_sidechain;
            if k == 0 && from.share != Share::Parent(ti) {
                continue;
            }
            let dst: &mut [f32] = if to_sidechain {
                if sidechain.len() != frames * 2 {
                    sidechain.resize(frames * 2, 0.0);
                }
                *keyed = true;
                sidechain
            } else {
                *dirty = true;
                &mut bus.samples
            };
            for (d, a) in dst.iter_mut().zip(add) {
                *d += *a;
            }
        }
    }

    /// Render track `ti` on the thread holding scratch `slot`.
    ///
    /// # Safety
    /// The scheduler's: `ti` runs once, every track feeding it is done, and
    /// `slot` is this thread's alone.
    #[allow(clippy::too_many_lines)]
    pub(super) unsafe fn run(&self, ti: usize, slot: usize) {
        let t = &self.tracks[ti];
        let frames = self.frames;
        let rate = self.rate;
        let inv_rate = 1.0 / f64::from(self.sample_rate);
        let start_seconds = self.start_seconds;
        // SAFETY: the caller's contract — this thread's slot, this track's
        // cells.
        let (w, st, edges) =
            unsafe { (self.workers.at(slot), self.stats.at(ti), self.edges.at(ti)) };
        *st = NodeStats::default();
        edges.reset(t.sends.len() + 1, frames);
        w.size(frames);

        // 1) The track's media into its bus. What feeds it arrives late (its
        // plugins' delay): its own items play that much later, to meet it.
        let passes = self.pass(ti).passes();
        if passes && !t.items.is_empty() {
            let t_items = Stamp::now();
            // SAFETY: this track's cells.
            let (bus, dirty) = unsafe { (self.buses.at(ti), self.dirty.at(ti)) };
            let late = f64::from(self.plan.input[ti]) * rate / f64::from(self.sample_rate);
            let (item_start, item_end) = (start_seconds - late, self.end_seconds - late);
            for item in &t.items {
                if item.muted {
                    continue;
                }
                // Fixed lanes: only items on PLAYING lanes sound — the rest
                // are alternate takes (REAPER 7 comping).
                if t.lane_play_mask != 0
                    && let Some(lane) = item.fixed_lane
                    && (lane >= 64 || t.lane_play_mask & (1u64 << lane) == 0)
                {
                    continue;
                }
                let Some(audio) = &item.audio else { continue };
                *dirty |= mix_item_into_bus(
                    bus,
                    audio,
                    item,
                    item_start,
                    item_end,
                    self.sample_rate,
                    rate,
                );
            }
            let us = t_items.us();
            if let Some(costs) = self.costs {
                costs.add_media(ti, us);
            }
            st.items_us = us;
            if self.debug {
                let v = rms(&bus.samples);
                if v > 1e-9 {
                    eprintln!("[stage1] {:<24} items={} rms={v:.6}", t.name, t.items.len());
                }
            }
        }
        // An offline render's input probes: the track's own media alone,
        // before children, sends or FX add to it.
        if self.probing {
            // SAFETY: this track's bus.
            let bus = unsafe { self.buses.get(ti) };
            self.probe(ti, offline::Probe::Input, &bus.samples);
        }
        if !passes {
            // Solo-skipped tracks meter as silence.
            self.meter(ti, &[]);
            return;
        }
        // SAFETY: this track's cells, its feeders done.
        unsafe { self.gather(ti) };
        // SAFETY: this track's cells.
        let (bus, dirty, keyed, aux, aux_live, delays, monitor_delay) = unsafe {
            (
                self.buses.at(ti),
                self.dirty.at(ti),
                self.keyed.at(ti),
                self.aux.at(ti),
                self.aux_live.at(ti),
                self.delays.at(ti),
                self.monitor_delays.at(ti),
            )
        };
        let hot = |delays: &[pdc::Delay], monitor: &pdc::Delay| {
            delays.iter().any(pdc::Delay::hot) || monitor.hot()
        };

        // A track an earlier render recorded, played in its place.
        let fed = self
            .offline
            .filter(|o| !o.fed.is_empty())
            .and_then(|o| o.fed.get(&t.guid));
        if let Some(fed) = fed {
            *dirty = true;
            self.latencies.set(ti, fed.latency);
        }
        // Clean bus = all zeros: gain scales nothing, sends add nothing,
        // parent sums add nothing. (A track with FX must still run — synths
        // generate from MIDI.)
        if t.fx_chain.is_empty() && fed.is_none() {
            self.latencies.set(ti, 0);
        }
        if !*dirty && t.fx_chain.is_empty() && !hot(delays, monitor_delay) {
            self.meter(ti, &[]);
            return;
        }

        // Pre-FX send tap: the bus BEFORE the FX chain runs — the track's
        // input (items + children sums + received sends).
        let has_pre_fx_tap = t
            .sends
            .iter()
            .any(|s| s.mode == daw_proto::routing::SendMode::PreFx);
        if self.probing {
            self.probe(ti, offline::Probe::PreFx, &bus.samples);
        }
        if has_pre_fx_tap {
            w.pre_fx_tap.clear();
            w.pre_fx_tap.extend_from_slice(&bus.samples);
        }

        // FX chain: pipe the bus through every loaded plugin in order — a
        // folder's FX process the children's mix, a bus's its received
        // sends (REAPER's signal flow).
        if !t.fx_chain.is_empty() {
            let t_fx = Stamp::now();
            let dirty_in = *dirty;
            self.chain(ti, t, w, st, bus, dirty, keyed, aux, aux_live);
            // A chain that put out exact zeros leaves the bus clean, unless
            // the track hands on more than its output: a pre-FX send's tap,
            // a plugin's own outputs, a recording played in its place.
            *dirty |= fed.is_some() || *aux_live || (has_pre_fx_tap && dirty_in);
            let us = t_fx.us();
            st.fx_us = Some(us);
            if let Some(costs) = self.costs {
                costs.add_fx(ti, us);
                costs.add_sleep(ti, st.fx_ran, st.fx_slept);
            }
        }
        if !*dirty && !hot(delays, monitor_delay) {
            self.meter(ti, &[]);
            return;
        }

        // An offline render's pre-fader tap: post-FX, before the gain.
        if self.tap_point == Some(offline::Tap::PreFader) && ti < self.taps.len() {
            // SAFETY: this track's tap.
            unsafe { self.taps.at(ti) }.copy_from_slice(&bus.samples[..frames * 2]);
        }
        if self.probing {
            self.probe(ti, offline::Probe::PreFader, &bus.samples);
        }

        // Pre-fader send tap: post-FX, before the gain stage.
        let has_pre_fader_tap = t
            .sends
            .iter()
            .any(|s| s.mode == daw_proto::routing::SendMode::PostFx);
        if has_pre_fader_tap {
            w.pre_fader_tap.clear();
            w.pre_fader_tap.extend_from_slice(&bus.samples);
        }

        self.fader(t, bus, inv_rate);

        // What it hands on (its sends, its parent share) is its post-fader
        // signal.
        w.src.clear();
        w.src.extend_from_slice(&bus.samples);
        if let Some(fed) = fed {
            fed.read(self.start_samples.max(0.0) as u64, &mut w.src);
        }

        // Meter the post-fader signal (its children summed in already).
        self.meter(ti, &w.src);
        if self.tap_point == Some(offline::Tap::PostFader) && ti < self.taps.len() {
            // SAFETY: this track's tap.
            unsafe { self.taps.at(ti) }.copy_from_slice(&w.src[..frames * 2]);
        }
        if self.probing {
            self.probe(ti, offline::Probe::PostFader, &w.src);
        }
        if self.debug {
            let v = rms(&w.src);
            if v > 1e-9 {
                eprintln!(
                    "[stage2] {:<24} rms={v:.6} vol={:.3} ps={} pg={} hw={} sends={}",
                    t.name,
                    t.volume,
                    t.parent_send,
                    if t.parent_idx.is_some() {
                        "ok"
                    } else if t.has_parent_guid {
                        "MISS"
                    } else {
                        "none"
                    },
                    t.hw_out,
                    t.sends.len()
                );
            }
        }

        self.sends(
            ti,
            t,
            w,
            edges,
            delays,
            has_pre_fx_tap,
            has_pre_fader_tap,
            inv_rate,
        );
        self.share(ti, t, w, edges, delays, monitor_delay);
    }

    /// Run track `ti`'s FX chain over its bus.
    #[allow(clippy::too_many_arguments)]
    fn chain(
        &self,
        ti: usize,
        t: &TrackSnapshot,
        w: &mut Worker,
        st: &mut NodeStats,
        bus: &mut StereoBuffer,
        dirty: &mut bool,
        keyed: &mut bool,
        aux: &mut Vec<f32>,
        aux_live: &mut bool,
    ) {
        let frames = self.frames;
        // MIDI + note-expression events for this block, to every plugin in
        // the chain (REAPER's default) — non-MIDI plugins ignore them.
        collect_midi_events_into(
            &mut w.midi,
            t,
            self.start_seconds,
            self.end_seconds,
            self.frames_per_second,
            frames,
        );
        // Live MIDI pushed for this track this block, merged in order.
        if ti < self.midi_buckets.len() {
            // SAFETY: this track's bucket.
            let bucket = unsafe { self.midi_buckets.at(ti) };
            if !bucket.is_empty() {
                w.midi.append(bucket);
                w.midi.sort_by_key(|e| e.offset);
            }
        }
        let note_expr_events = collect_note_expressions(
            t,
            self.start_seconds,
            self.end_seconds,
            self.frames_per_second,
            frames,
        );
        // Mute is applied at the fader, after the chain, so a muted track's
        // plugins still run (tails, meters): say so to the plugins that
        // need to know.
        let _muted = crate::plugin::TrackMutedGuard::enter(t.muted);
        // ...unless nothing would hear them: only a pre-fader (post-FX)
        // send taps a muted track. Then its chain is not run at all.
        let heard_while_muted = t
            .sends
            .iter()
            .any(|s| s.mode == daw_proto::routing::SendMode::PostFx);
        let chain = if t.muted && !heard_while_muted {
            &[][..]
        } else {
            t.fx_chain.as_slice()
        };
        if t.aux_taps {
            aux_sized(aux, frames);
        }
        let tempo = self.snap.tempo_map.tempo_at(self.start_seconds);
        let mut chain_latency = 0u32;
        for (i, fx_guid) in chain.iter().enumerate() {
            // What goes into FX `i` is what came out of `i - 1`.
            if self.probing {
                self.probe(ti, offline::Probe::FxIn(i), &bus.samples);
                if let Some(before) = i.checked_sub(1) {
                    self.probe(ti, offline::Probe::FxOut(before), &bus.samples);
                }
            }
            if !t.fx_enabled.get(i).copied().unwrap_or(true) {
                continue;
            }
            // `None` = the map was busy this block: render dry rather than
            // block.
            let Some(cell) = self.plugins.and_then(|p| p.cell(fx_guid)) else {
                continue;
            };
            let Some(mut plugin) = cell.try_lock_spinning(PLUGIN_LOCK_SPINS) else {
                PLUGIN_BUSY_SKIPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                continue; // a control thread has it: this block dry
            };
            let plugin = &mut **plugin;
            if self.panicked_fx.contains(fx_guid) || st.panicked.is_some_and(|(p, _)| p == i) {
                continue; // panicked earlier — permanently bypassed
            }
            // Prepared ahead (at load) at another rate than this render's:
            // prepared again, here.
            let wrong_rate = plugin
                .prepared_rate()
                .is_some_and(|r| r != f64::from(self.sample_rate));
            if !plugin.is_prepared() || wrong_rate {
                // Lazy prepare on first use; bypass on failure. A panic is
                // caught and the plugin bypassed from then on.
                let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    plugin.prepare(
                        f64::from(self.sample_rate),
                        (frames as u32).max(super::PREPARE_FRAMES),
                    )
                }));
                match prepared {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) => continue,
                    Err(_) => {
                        st.panicked.get_or_insert((i, true));
                        continue;
                    }
                }
            }
            // Its clock: its audio as late as the track's input and the FX
            // before it make it.
            crate::plugin::set_render_transport((self.transport_late)(
                self.plan.input[ti].saturating_add(chain_latency),
            ));
            let latency = plugin.latency();
            chain_latency = chain_latency.saturating_add(latency);
            // Asleep, and nothing to wake it: not called, its output the
            // silence it heard (`super::sleep`). Only a plugin that ever
            // said it may sleep is asked.
            let sleepy = plugin.process_status() != crate::plugin_sleep::ProcessStatus::Continue
                || cell.sleep().is_asleep();
            let heard = sleepy.then(|| {
                // SAFETY: this track's sidechain, gathered already.
                let sidechain = (*keyed && ti < self.sidechains.len())
                    .then(|| unsafe { self.sidechains.get(ti) }.as_slice());
                super::sleep::Fx::hear(
                    cell,
                    *dirty,
                    &bus.samples[..frames * 2],
                    sidechain,
                    !w.midi.is_empty() || !note_expr_events.is_empty(),
                    t.fx_params.get(i).map(Vec::as_slice).unwrap_or(&[]),
                    t.fx_envs.get(i).map(Vec::as_slice).unwrap_or(&[]),
                    self.clock,
                )
            });
            if let Some(crate::plugin_sleep::Skip::Asleep { saved_us }) =
                heard.as_ref().map(|h| h.skip(cell))
            {
                st.fx_slept = st.fx_slept.saturating_add(1);
                st.slept_us = st.slept_us.saturating_add(saved_us);
                continue;
            }
            let t_plugin = Stamp::now();
            // De-interleave → process → re-interleave.
            for f in 0..frames {
                w.in_l[f] = bus.samples[f * 2];
                w.in_r[f] = bus.samples[f * 2 + 1];
            }
            w.out_l.fill(0.0);
            w.out_r.fill(0.0);
            if *keyed && ti < self.sidechains.len() {
                // SAFETY: this track's sidechain, gathered already.
                let sidechain = unsafe { self.sidechains.get(ti) };
                for f in 0..frames {
                    w.key_l[f] = sidechain[f * 2];
                    w.key_r[f] = sidechain[f * 2 + 1];
                }
                plugin.set_sidechain(&w.key_l, &w.key_r);
            }
            let Worker {
                in_l,
                in_r,
                out_l,
                out_r,
                fx_params,
                midi,
                ..
            } = &mut *w;
            let block = automate::FxBlock {
                fixed: t.fx_params.get(i).map(Vec::as_slice).unwrap_or(&[]),
                envelopes: t.fx_envs.get(i).map(Vec::as_slice).unwrap_or(&[]),
                clock: self.clock,
                midi: midi.as_slice(),
                note_expressions: &note_expr_events,
            };
            plugin.set_tempo(tempo);
            // A panicking plugin bypasses (the bus keeps its pre-plugin
            // signal) instead of unwinding across the audio callback.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let io = automate::Io {
                    in_l: &in_l[..frames],
                    in_r: &in_r[..frames],
                    out_l: &mut out_l[..frames],
                    out_r: &mut out_r[..frames],
                };
                fx_params.process(plugin, io, &block)
            }));
            match result {
                Ok((Ok(()), schedule)) => automate::record(schedule),
                Ok((Err(crate::plugin::PluginError::BlockTooLarge), _)) => {
                    super::PLUGIN_TOO_LARGE_SKIPS
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    continue;
                }
                Ok((Err(_), _)) => continue,
                Err(_) => {
                    st.panicked.get_or_insert((i, false));
                    continue;
                }
            }
            st.fx_ran = st.fx_ran.saturating_add(1);
            let us = t_plugin.us();
            let zero = match &heard {
                Some(h) => h.ran(
                    cell,
                    plugin,
                    &w.out_l[..frames],
                    &w.out_r[..frames],
                    latency,
                    us,
                ),
                None => crate::plugin_sleep::silence(&w.out_l[..frames], &w.out_r[..frames]).0,
            };
            for f in 0..frames {
                bus.samples[f * 2] = w.out_l[f];
                bus.samples[f * 2 + 1] = w.out_r[f];
            }
            if t.aux_taps {
                *aux_live |= add_aux(&*plugin, aux, frames);
            }
            // A plugin ran — it may synthesize (MIDI → audio). Exact zeros
            // leave the bus clean: a sum of silent tracks stays clean.
            *dirty = !zero;
        }
        if let (true, Some(last)) = (self.probing, chain.len().checked_sub(1)) {
            self.probe(ti, offline::Probe::FxOut(last), &bus.samples);
        }
        crate::plugin::set_render_transport(self.block_transport);
        // A muted chain not run keeps the latency it had.
        if !chain.is_empty() {
            self.latencies.set(ti, chain_latency);
        }
    }

    /// Gain + pan + polarity, per-frame envelopes, VCA leads.
    fn fader(&self, t: &TrackSnapshot, bus: &mut StereoBuffer, inv_rate: f64) {
        let tracks = self.tracks;
        let (start_seconds, rate) = (self.start_seconds, self.rate);
        // VCA grouping (user guide §5.16): a follower's volume is dB-added
        // with every shared-group lead's fader + volume envelope; the lead's
        // pan offsets the follower's; the lead's mute — button or envelope —
        // silences followers.
        let vca_leads: Vec<&TrackSnapshot> = if t.vca_follow != 0 {
            tracks
                .iter()
                .filter(|l| l.vca_lead & t.vca_follow != 0)
                .collect()
        } else {
            Vec::new()
        };
        let vca_lead_muted = vca_leads.iter().any(|l| l.muted);
        let mut vca_cursors: Vec<(
            Option<EnvelopeCursor>,
            Option<EnvelopeCursor>,
            Option<EnvelopeCursor>,
        )> = vca_leads
            .iter()
            .map(|l| {
                (
                    l.volume_env.as_ref().map(|p| EnvelopeCursor::new(p)),
                    l.pan_env.as_ref().map(|p| EnvelopeCursor::new(p)),
                    l.mute_env.as_ref().map(|p| EnvelopeCursor::new(p)),
                )
            })
            .collect();
        let mut c_vol = t.volume_env.as_ref().map(|p| EnvelopeCursor::new(p));
        let mut c_pvol = t.volume_prefx_env.as_ref().map(|p| EnvelopeCursor::new(p));
        let mut c_pan = t.pan_env.as_ref().map(|p| EnvelopeCursor::new(p));
        let mut c_ppan = t.pan_prefx_env.as_ref().map(|p| EnvelopeCursor::new(p));
        let mut c_mute = t.mute_env.as_ref().map(|p| EnvelopeCursor::new(p));
        // No envelope and no VCA lead: the gains are the same on every frame.
        let fixed = c_vol.is_none()
            && c_pvol.is_none()
            && c_pan.is_none()
            && c_ppan.is_none()
            && c_mute.is_none()
            && vca_leads.is_empty();
        if fixed {
            let vol = t.volume * 1.0 * 1.0;
            let pan = (t.pan + 0.0 + 0.0).clamp(-1.0, 1.0);
            if t.muted || vca_lead_muted {
                bus.samples[..bus.frames * 2].fill(0.0);
            } else {
                let sign = if t.phase_inverted { -1.0 } else { 1.0 };
                // The same balance law as the per-frame path below.
                let (lg, rg) = balance(pan);
                let lg = (lg * vol * sign) as f32;
                let rg = (rg * vol * sign) as f32;
                for fr in bus.samples[..bus.frames * 2].chunks_exact_mut(2) {
                    fr[0] *= lg;
                    fr[1] *= rg;
                }
            }
            return;
        }
        for frame in 0..bus.frames {
            let time = start_seconds + frame as f64 * rate * inv_rate;
            let v_main = c_vol.as_mut().and_then(|c| c.eval_at(time)).unwrap_or(1.0);
            let v_prefx = c_pvol.as_mut().and_then(|c| c.eval_at(time)).unwrap_or(1.0);
            // Read and the writing modes: the envelope is the volume;
            // Trim/Read: the fader trims it.
            let fader = if t.volume_replaces { 1.0 } else { t.volume };
            let mut vol = fader * v_main * v_prefx;
            let p_main = c_pan
                .as_mut()
                .and_then(|c| c.eval_at(time))
                .map(|v| (v - 0.5) * 2.0)
                .unwrap_or(0.0);
            let p_prefx = c_ppan
                .as_mut()
                .and_then(|c| c.eval_at(time))
                .map(|v| (v - 0.5) * 2.0)
                .unwrap_or(0.0);
            let knob = if t.pan_replaces { 0.0 } else { t.pan };
            let mut pan = knob + p_main + p_prefx;
            let mut vca_env_muted = false;
            for (li, lead) in vca_leads.iter().enumerate() {
                let (c_lv, c_lp, c_lm) = &mut vca_cursors[li];
                let lv = c_lv.as_mut().and_then(|c| c.eval_at(time)).unwrap_or(1.0);
                vol *= lead.volume * lv;
                let lp = c_lp
                    .as_mut()
                    .and_then(|c| c.eval_at(time))
                    .map(|v| (v - 0.5) * 2.0)
                    .unwrap_or(0.0);
                pan += lead.pan + lp;
                if c_lm
                    .as_mut()
                    .and_then(|c| c.eval_at(time))
                    .map(|v| v > 0.5)
                    .unwrap_or(false)
                {
                    vca_env_muted = true;
                }
            }
            let pan = pan.clamp(-1.0, 1.0);
            let env_muted = c_mute
                .as_mut()
                .and_then(|c| c.eval_at(time))
                .map(|v| v > 0.5)
                .unwrap_or(false);
            let muted = t.muted || env_muted || vca_lead_muted || vca_env_muted;
            if muted {
                bus.samples[frame * 2] = 0.0;
                bus.samples[frame * 2 + 1] = 0.0;
                continue;
            }
            // Polarity invert folds into the gain sign.
            let sign = if t.phase_inverted { -1.0 } else { 1.0 };
            let (lg, rg) = balance(pan);
            let (lg, rg) = (lg * vol * sign, rg * vol * sign);
            bus.samples[frame * 2] *= lg as f32;
            bus.samples[frame * 2 + 1] *= rg as f32;
        }
    }

    /// Each send's signal into its edge, gained, panned and delayed to meet
    /// the rest of its destination's sum.
    #[allow(clippy::too_many_arguments)]
    fn sends(
        &self,
        ti: usize,
        t: &TrackSnapshot,
        w: &mut Worker,
        edges: &mut Edges,
        delays: &mut [pdc::Delay],
        has_pre_fx_tap: bool,
        has_pre_fader_tap: bool,
        inv_rate: f64,
    ) {
        let frames = self.frames;
        // SAFETY: this track's aux, read only from here.
        let (aux, aux_live) = unsafe { (self.aux.get(ti), *self.aux_live.get(ti)) };
        for (si, snd) in t.sends.iter().enumerate() {
            if snd.muted {
                continue;
            }
            if snd.dest_idx.is_none() {
                continue;
            }
            let delay = self.plan.edges[ti].get(si + 1).copied().unwrap_or(0) as usize;
            // From a plugin's own outputs (3/4, 5/6): taken post-FX, and
            // through the fader when the send is post-fader — the track's
            // mute and level, not its envelopes.
            let mut from_aux = 1.0_f32;
            let send_src: &[f32] = if snd.from_pair > 0 {
                if snd.mode == daw_proto::routing::SendMode::PostFader {
                    if t.muted {
                        continue;
                    }
                    from_aux = t.volume as f32;
                }
                let span = (snd.from_pair - 1) * frames * 2..snd.from_pair * frames * 2;
                match Some(aux).filter(|_| aux_live).and_then(|a| a.get(span)) {
                    Some(pair) => pair,
                    None => continue,
                }
            } else {
                match snd.mode {
                    daw_proto::routing::SendMode::PreFx if has_pre_fx_tap => &w.pre_fx_tap,
                    daw_proto::routing::SendMode::PostFx if has_pre_fader_tap => &w.pre_fader_tap,
                    _ => &w.src,
                }
            };
            // Late enough to meet the rest of what its destination sums.
            let send_src: &[f32] = match delays.get_mut(si + 1) {
                Some(line) if delay > 0 || line.engaged() => {
                    line.run(delay, send_src, &mut w.delayed, frames);
                    &w.delayed[..frames * 2]
                }
                _ => send_src,
            };
            let out = edges.edge_mut(si + 1, frames);
            let mut c_vol = snd.volume_env.as_ref().map(|p| EnvelopeCursor::new(p));
            let mut c_pan = snd.pan_env.as_ref().map(|p| EnvelopeCursor::new(p));
            let mut c_mute = snd.mute_env.as_ref().map(|p| EnvelopeCursor::new(p));
            for frame in 0..frames {
                let time = self.start_seconds + frame as f64 * self.rate * inv_rate;
                let env_muted = c_mute
                    .as_mut()
                    .and_then(|c| c.eval_at(time))
                    .map(|v| v > 0.5)
                    .unwrap_or(false);
                if env_muted {
                    // Adding -0.0 leaves any sum exactly as it was.
                    out[frame * 2] = -0.0;
                    out[frame * 2 + 1] = -0.0;
                    continue;
                }
                let v_env = c_vol.as_mut().and_then(|c| c.eval_at(time)).unwrap_or(1.0);
                let vol = snd.volume * v_env;
                let p_off = c_pan
                    .as_mut()
                    .and_then(|c| c.eval_at(time))
                    .map(|v| (v - 0.5) * 2.0)
                    .unwrap_or(0.0);
                let pan = (snd.pan + p_off).clamp(-1.0, 1.0);
                let (lg, rg) = balance(pan);
                let (lg, rg) = ((lg * vol) as f32 * from_aux, (rg * vol) as f32 * from_aux);
                out[frame * 2] = send_src[frame * 2] * lg;
                out[frame * 2 + 1] = send_src[frame * 2 + 1] * rg;
            }
            edges.live[si + 1] = true;
        }
    }

    /// The parent (or master) share: not for a track that, under a solo,
    /// only feeds what is soloed; straight to master for one soloed with
    /// no way there of its own (`graph::Pass`). Delayed to meet the rest of
    /// that sum.
    fn share(
        &self,
        ti: usize,
        t: &TrackSnapshot,
        w: &mut Worker,
        edges: &mut Edges,
        delays: &mut [pdc::Delay],
        monitor_delay: &mut pdc::Delay,
    ) {
        let frames = self.frames;
        let mode = self.pass(ti);
        let out = edges.edge_mut(0, frames);
        let (delay, line) = if mode == graph::Pass::Monitor {
            (self.plan.to_master[ti] as usize, Some(monitor_delay))
        } else {
            (
                self.plan.edges[ti].first().copied().unwrap_or(0) as usize,
                delays.first_mut(),
            )
        };
        match line {
            Some(line) if delay > 0 || line.engaged() => line.run(delay, &w.src, out, frames),
            _ => out.copy_from_slice(&w.src[..frames * 2]),
        }
        edges.share = if mode == graph::Pass::Monitor {
            Share::Master
        } else if mode != graph::Pass::Feed && t.parent_send {
            match t.parent_idx {
                Some(pi) => Share::Parent(pi),
                None => Share::Master,
            }
        } else if mode != graph::Pass::Feed && t.hw_out {
            // No parent send but routed to hardware — v0 sums hw outs
            // straight to the master device buffer.
            Share::Master
        } else {
            Share::Nowhere
        };
        edges.live[0] = edges.share != Share::Nowhere;
        if self.debug && edges.share == Share::Master {
            let v = rms(edges.edge(0, frames));
            if v > 1e-9 {
                eprintln!("[->master] {:<24} rms={v:.6}", t.name);
            }
        }
    }
}

fn rms(s: &[f32]) -> f64 {
    (s.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>() / s.len().max(1) as f64).sqrt()
}

const _: usize = AUX_PAIRS;

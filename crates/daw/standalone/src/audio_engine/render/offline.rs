//! Offline renders of a song that leave the song — and its playback —
//! alone: a [`ProjectRenderer`] on its own copies of the song's plugins,
//! with its own solos and bypasses, tapping each track's signal.
//!
//! The song's renders run its live plugin instances, so a bounce on them
//! would push frames through the plugins playback is using (and fight it
//! for their locks). An offline render runs [`copy_plugins`]' instances
//! instead: one per FX, made the way the FX was and carrying the live
//! one's state. Its solos and bypasses ([`Offline::solo`],
//! [`Offline::bypass_fx`]) change its own snapshot of the song, never the
//! song. Every track's signal can be tapped ([`Tap`]) after each block
//! ([`ProjectRenderer::with_taps`]), each late by its own latency
//! ([`ProjectRenderer::track_latencies`]); what reaches master is late by
//! [`ProjectRenderer::latency`].

use std::collections::HashSet;
use std::sync::Arc;

use super::snapshot::TrackSnapshot;
use super::{ProjectRenderer, RenderSnapshot};
use crate::plugin::{PluginInstance, Plugins};
use crate::sync::Standalone;

/// Where an offline render taps each track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tap {
    /// After the FX, before the fader, pan and mute.
    PreFader,
    /// After the fader: what the track puts into its parent.
    PostFader,
}

/// What an offline render changes, for itself only.
#[derive(Default)]
pub struct Offline {
    /// The plugins it runs, by FX guid ([`copy_plugins`]).
    pub plugins: Arc<Plugins>,
    /// FX (by guid) it renders bypassed.
    pub bypass_fx: HashSet<String>,
    /// The tracks (by guid) it renders soloed, in place of the song's own
    /// solos; `None` keeps the song's.
    pub solo: Option<HashSet<String>>,
    /// Where it taps each track, if it does.
    pub tap: Option<Tap>,
}

impl Offline {
    /// A track's snapshot as this render has it.
    pub(super) fn adjust(&self, t: &mut TrackSnapshot) {
        if !self.bypass_fx.is_empty() {
            for (on, fx) in t.fx_enabled.iter_mut().zip(&t.fx_chain) {
                if self.bypass_fx.contains(fx) {
                    *on = false;
                }
            }
        }
        if let Some(solo) = &self.solo {
            t.soloed = solo.contains(&t.guid);
        }
    }
}

/// What [`copy_plugins`] made.
#[derive(Debug, Default)]
pub struct Copies {
    /// FX copied, their state with them.
    pub copied: u32,
    /// Of them, how many carried a state blob.
    pub with_state: u32,
    /// The state carried, bytes.
    pub state_bytes: u64,
    /// FX that would not be copied (rendered dry), by guid, and why.
    pub failed: Vec<(String, String)>,
}

/// Makes a plugin for an FX by its chain name, in place of the usual way
/// (its bundle path, then the FX factory) — `None` for that.
pub type Make<'a> = &'a dyn Fn(&str) -> Option<Box<dyn PluginInstance>>;

/// Rewrites a live instance's state (by the FX's chain name) before its
/// copy loads it: a plugin whose instances find each other through
/// something process-wide (a named session) given a name of its own, so
/// a copy never joins what the live instances share.
pub type Restate<'a> = &'a dyn Fn(&str, Vec<u8>) -> Vec<u8>;

/// How [`copy_plugins`] makes its copies.
#[derive(Default, Clone, Copy)]
pub struct Copying<'a> {
    pub make: Option<Make<'a>>,
    pub restate: Option<Restate<'a>>,
}

/// A copy of every plugin instance on song `project_guid`: each FX's
/// plugin made again (`how.make`, else the way the engine makes it), the
/// live one's state (through `how.restate`) loaded into it. Reading a live instance's state takes its
/// lock for as long as the plugin's save does; the render playing it
/// renders that plugin dry for a block if that is longer than its spin.
pub fn copy_plugins(
    daw: &Standalone,
    project_guid: &str,
    how: Copying<'_>,
) -> (Arc<Plugins>, Copies) {
    let plugins = Arc::new(Plugins::default());
    let mut copies = Copies::default();
    for (guid, name) in hosted(daw, project_guid) {
        let state = daw.save_plugin_state(&guid).unwrap_or_default();
        match copy_plugin(daw, &name, state, how) {
            Ok((copy, bytes)) => {
                if bytes > 0 {
                    copies.with_state += 1;
                    copies.state_bytes += bytes as u64;
                }
                plugins.write().insert(guid, copy);
                copies.copied += 1;
            }
            Err(why) => copies.failed.push((guid, why)),
        }
    }
    (plugins, copies)
}

/// Every FX on song `project_guid` with a live plugin instance: its guid
/// and its chain name.
pub fn hosted(daw: &Standalone, project_guid: &str) -> Vec<(String, String)> {
    daw.read_project(project_guid, |p| {
        p.fx_chains
            .values()
            .flatten()
            .map(|e| (e.fx.guid.clone(), e.fx.name.clone()))
            .collect::<Vec<_>>()
    })
    .unwrap_or_default()
    .into_iter()
    .filter(|(guid, _)| daw.has_plugin_instance(guid))
    .collect()
}

/// One copy: the FX `name`'s plugin made again (`how.make`, else the way
/// the engine makes it), `state` (a live instance's, through
/// `how.restate`) loaded into it. The copy and the state's size.
///
/// # Errors
/// It would not be made, or refused its state.
pub fn copy_plugin(
    daw: &Standalone,
    name: &str,
    state: Vec<u8>,
    how: Copying<'_>,
) -> Result<(Box<dyn PluginInstance>, usize), String> {
    let mut copy = how
        .make
        .and_then(|m| m(name))
        .or_else(|| crate::fx::instantiate(daw, name))
        .ok_or_else(|| format!("{name}: would not be made again"))?;
    // A built-in without a state of its own: its parameters are the
    // song's, sent every block.
    if state.is_empty() {
        return Ok((copy, 0));
    }
    let state = match how.restate {
        Some(restate) => restate(name, state),
        None => state,
    };
    copy.load_state(&state)
        .map_err(|e| format!("{name}: refused its state: {e}"))?;
    copy.restart_if_requested();
    Ok((copy, state.len()))
}

impl ProjectRenderer {
    /// A renderer of song `project_guid` for an offline render: its own
    /// plugins, solos, bypasses and taps (`offline`). Never realtime.
    pub fn offline(
        daw: &Standalone,
        project_guid: &str,
        sample_rate: u32,
        offline: Offline,
    ) -> Self {
        let mut renderer = Self::new(daw, project_guid, sample_rate);
        // An offline render is not the song playing: no meters.
        renderer.set_metering(false);
        renderer.offline = Some(offline);
        renderer
    }

    /// Ready an offline render to start at full delay compensation: its
    /// plugins activated at `block` frames, each track's chain latency
    /// asked of them. The song's latency (what reaches master is that
    /// late). A renderer rendering without this plans its first block
    /// with no latencies, and is compensated from its second.
    pub fn prime(&self, block: u32) -> u32 {
        let Some(snap) = self.snapshot() else {
            return 0;
        };
        let plugins = self
            .offline
            .as_ref()
            .map_or(&self.daw.plugin_instances, |o| &o.plugins);
        for (ti, t) in snap.tracks.iter().enumerate() {
            let mut latency = 0u32;
            for (i, fx) in t.fx_chain.iter().enumerate() {
                if !t.fx_enabled.get(i).copied().unwrap_or(true) {
                    continue;
                }
                latency = latency.saturating_add(
                    plugins
                        .with(fx, |p| {
                            if !p.is_prepared()
                                && p.prepare(f64::from(self.sample_rate), block).is_err()
                            {
                                return 0;
                            }
                            p.latency()
                        })
                        .unwrap_or(0),
                );
            }
            self.latencies.set(ti, latency);
        }
        let mut plan = super::pdc::Plan::default();
        super::pdc::plan(
            &snap.tracks,
            &snap.order,
            |ti| self.latencies.get(ti),
            &mut plan,
        );
        self.song_latency
            .store(plan.master, std::sync::atomic::Ordering::Relaxed);
        plan.master
    }

    /// Each track's latency at its tap, samples, by track index: what it
    /// puts out is that late (its input's lateness plus its chain).
    pub fn track_latencies(&self) -> Vec<u32> {
        let Some(snap) = self.snapshot() else {
            return Vec::new();
        };
        let mut plan = super::pdc::Plan::default();
        super::pdc::plan(
            &snap.tracks,
            &snap.order,
            |ti| self.latencies.get(ti),
            &mut plan,
        );
        plan.output
    }

    /// Run `f` on the last block's taps: per track (by index), its signal
    /// that block, interleaved stereo — silence for a track with nothing.
    /// Empty when the render does not tap.
    pub fn with_taps<R>(&self, f: impl FnOnce(&[Vec<f32>]) -> R) -> R {
        let scratch = self
            .scratch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&scratch.taps)
    }

    /// The tracks this render sees, by index: their guids.
    pub fn track_guids(&self) -> Vec<String> {
        self.snapshot()
            .map(|s: Arc<RenderSnapshot>| s.tracks.iter().map(|t| t.guid.clone()).collect())
            .unwrap_or_default()
    }
}

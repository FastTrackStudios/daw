//! Sync, backend-generic **handles** over the daw service traits — the
//! ergonomic surface for in-process consumers.
//!
//! The async client side already has this shape (`daw_control::Project` /
//! `TrackHandle` over vox callers); this module is its synchronous twin,
//! generic over *any* backend implementing the `#[architect::rpc]` service
//! traits — `Standalone`, the REAPER in-process services, a test mock. That
//! is the architect pattern: program to the trait bundle, and the same
//! calling code runs over every backend.
//!
//! What it replaces (the dialect every in-process consumer used to write):
//!
//! ```text
//! <Standalone as Tracks>::set_volume(&daw, ProjectContext::Current,
//!     TrackRef::guid(&guid), 0.9)?;                       // UFCS: four traits
//!                                                          // have set_volume
//! let idx = <Standalone as FxChains>::add(&daw, ctx.clone(), "slot")?;
//! let fx  = <Standalone as FxChains>::get(&daw, ctx, idx)…  // the add+get dance
//! ```
//!
//! with:
//!
//! ```text
//! use daw_proto::handle::DawHandle as _;
//!
//! let project = daw.current();                 // ProjectHandle
//! let track = project.add_track("Keys")?;      // TrackHandle
//! track.set_volume(0.9)?;
//! track.mute(true)?;
//! let slot = track.add_fx_slot("keys")?;       // add + get, one call
//! track.send_to(bus.guid()).post_fx().replace_master_send().apply()?;
//! ```
//!
//! Handles are cheap (a backend ref + guid) and deliberately thin: every
//! method is one service call, no caching, no state — the ergonomics live in
//! naming, defaulted contexts, and the fused multi-call rituals (fx-slot
//! reservation, bus routing, folder-depth bookkeeping via [`TrackTree`]).

use crate::error::{DawError, DawResult};
use crate::routing::Routing;
use crate::track::Tracks;
use crate::{
    Fx, FxChainContext, ProjectContext, RecordInput, RouteLocation, RouteRef, SendMode, Track,
    TrackRef,
};

impl<D> Clone for ProjectHandle<'_, D> {
    fn clone(&self) -> Self {
        Self { daw: self.daw, ctx: self.ctx.clone() }
    }
}

impl<D> Clone for TrackHandle<'_, D> {
    fn clone(&self) -> Self {
        Self { daw: self.daw, ctx: self.ctx.clone(), guid: self.guid.clone() }
    }
}

/// Entry point: `daw.current()` / `daw.project(guid)` for any backend
/// implementing [`Tracks`]. Import as `use daw_proto::handle::DawHandle as _;`.
pub trait DawHandle: Tracks + Sized {
    /// A handle on the current project.
    fn current(&self) -> ProjectHandle<'_, Self> {
        ProjectHandle { daw: self, ctx: ProjectContext::Current }
    }

    /// A handle on a specific project.
    fn project(&self, guid: impl Into<String>) -> ProjectHandle<'_, Self> {
        ProjectHandle { daw: self, ctx: ProjectContext::Project(guid.into()) }
    }
}

impl<D: Tracks + Sized> DawHandle for D {}

/// A project, bound to its backend — makes tracks and scopes every child
/// handle to this project's context.
pub struct ProjectHandle<'d, D> {
    daw: &'d D,
    ctx: ProjectContext,
}

impl<'d, D: Tracks> ProjectHandle<'d, D> {
    pub fn context(&self) -> ProjectContext {
        self.ctx.clone()
    }

    /// Add a track (appended); returns its handle.
    pub fn add_track(&self, name: &str) -> DawResult<TrackHandle<'d, D>> {
        let guid = self.daw.add(self.ctx.clone(), name, None)?;
        Ok(self.track(guid))
    }

    /// A handle on an existing track by guid.
    pub fn track(&self, guid: impl Into<String>) -> TrackHandle<'d, D> {
        TrackHandle { daw: self.daw, ctx: self.ctx.clone(), guid: guid.into() }
    }

    /// Every track, in mixer order.
    pub fn tracks(&self) -> Vec<Track> {
        self.daw.all(self.ctx.clone())
    }

    /// The master track's handle, if the backend exposes one.
    pub fn master(&self) -> Option<TrackHandle<'d, D>> {
        self.daw.master(self.ctx.clone()).map(|t| self.track(t.guid))
    }

    /// Remove every track.
    pub fn remove_all_tracks(&self) -> DawResult<()> {
        self.daw.remove_all(self.ctx.clone())
    }

    /// A [`TrackTree`] builder over this project — folder hierarchies with
    /// the REAPER depth convention handled for you.
    pub fn tree(&self) -> TrackTree<'d, D> {
        TrackTree { project: ProjectHandle { daw: self.daw, ctx: self.ctx.clone() }, open: 0, last: None }
    }
}

/// One track, bound to its backend + project. Every mixer/routing/FX op is a
/// single unambiguous method — no UFCS, no context/ref wrapping.
pub struct TrackHandle<'d, D> {
    daw: &'d D,
    ctx: ProjectContext,
    guid: String,
}

impl<'d, D> TrackHandle<'d, D> {
    pub fn guid(&self) -> &str {
        &self.guid
    }

    fn track_ref(&self) -> TrackRef {
        TrackRef::Guid(self.guid.clone())
    }
}

impl<'d, D: Tracks> TrackHandle<'d, D> {
    /// Full track state, if the track still exists.
    pub fn info(&self) -> Option<Track> {
        self.daw.get(self.ctx.clone(), self.track_ref())
    }

    /// Post-FX fader gain (linear; 1.0 = unity).
    pub fn set_volume(&self, volume: f64) -> DawResult<()> {
        self.daw.set_volume(self.ctx.clone(), self.track_ref(), volume)
    }

    /// Pan (−1..1).
    pub fn set_pan(&self, pan: f64) -> DawResult<()> {
        self.daw.set_pan(self.ctx.clone(), self.track_ref(), pan)
    }

    pub fn mute(&self, muted: bool) -> DawResult<()> {
        self.daw.set_muted(self.ctx.clone(), self.track_ref(), muted)
    }

    pub fn solo(&self, soloed: bool) -> DawResult<()> {
        self.daw.set_soloed(self.ctx.clone(), self.track_ref(), soloed)
    }

    pub fn rename(&self, name: &str) -> DawResult<()> {
        self.daw.rename(self.ctx.clone(), self.track_ref(), name)
    }

    /// Arm the track to record/monitor hardware audio input `channel` — what
    /// makes a live engine open an input stream and feed this track's bus.
    pub fn arm_audio_input(&self, channel: u32) -> DawResult<()> {
        self.daw
            .set_record_input(self.ctx.clone(), self.track_ref(), RecordInput::Audio { channel })
    }

    /// REAPER folder-depth change (positive opens a folder, a negative depth
    /// on the last child closes levels). Prefer [`ProjectHandle::tree`],
    /// which does this bookkeeping for you.
    pub fn set_folder_depth(&self, depth: i32) -> DawResult<()> {
        self.daw.set_folder_depth(self.ctx.clone(), self.track_ref(), depth)
    }

    /// Remove the track.
    pub fn remove(&self) -> DawResult<()> {
        self.daw.remove(self.ctx.clone(), self.track_ref())
    }
}

impl<'d, D: crate::FxChains> TrackHandle<'d, D> {
    fn fx_ctx(&self) -> FxChainContext {
        FxChainContext::track(self.guid.clone())
    }

    /// Reserve one FX slot (the add + guid-fetch dance, fused). The returned
    /// handle's guid is constant for the project's life — swapping the
    /// instance behind it never rebuilds a renderer snapshot.
    pub fn add_fx_slot(&self, label: &str) -> DawResult<FxSlotHandle<'d, D>> {
        let index = self.daw.add(self.fx_ctx(), label)?;
        let fx = self
            .daw
            .get(self.fx_ctx(), index)
            .ok_or_else(|| DawError::Internal(format!("fx slot {label:?} vanished after add")))?;
        Ok(FxSlotHandle { daw: self.daw, track_guid: self.guid.clone(), index, fx })
    }

    /// Enable/bypass the FX at `index` (bypass = `enabled(false)`).
    pub fn set_fx_enabled(&self, index: u32, enabled: bool) -> DawResult<()> {
        self.daw.set_enabled(self.fx_ctx(), index, enabled)
    }
}

impl<'d, D: Routing + Tracks> TrackHandle<'d, D> {
    /// Start a send from this track — finish with
    /// [`SendBuilder::apply`]. `dest` is the destination track's guid.
    pub fn send_to(&self, dest: impl Into<String>) -> SendBuilder<'d, '_, D> {
        SendBuilder {
            track: self,
            dest: dest.into(),
            mode: None,
            volume: None,
            replace_master: false,
        }
    }
}

/// A reserved FX slot on a track.
pub struct FxSlotHandle<'d, D> {
    daw: &'d D,
    track_guid: String,
    index: u32,
    fx: Fx,
}

impl<'d, D> FxSlotHandle<'d, D> {
    /// The slot's guid (constant for the project's life).
    pub fn guid(&self) -> &str {
        &self.fx.guid
    }

    pub fn into_guid(self) -> String {
        self.fx.guid
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn track_guid(&self) -> &str {
        &self.track_guid
    }
}

impl<'d, D: crate::FxChains> FxSlotHandle<'d, D> {
    /// Enable/bypass this slot.
    pub fn set_enabled(&self, enabled: bool) -> DawResult<()> {
        self.daw
            .set_enabled(FxChainContext::track(self.track_guid.clone()), self.index, enabled)
    }
}

/// Fluent send construction — fuses the add-send / set-mode /
/// detach-master-send / set-volume ritual into one readable chain:
///
/// ```text
/// mic.send_to(bus.guid()).post_fx().replace_master_send().apply()?;
/// ```
#[must_use = "call .apply() to create the send"]
pub struct SendBuilder<'d, 't, D> {
    track: &'t TrackHandle<'d, D>,
    dest: String,
    mode: Option<SendMode>,
    volume: Option<f64>,
    replace_master: bool,
}

impl<'d, 't, D: Routing + Tracks> SendBuilder<'d, 't, D> {
    /// Tap post-FX, post-fader (the bus-mic convention).
    pub fn post_fx(mut self) -> Self {
        self.mode = Some(SendMode::PostFx);
        self
    }

    /// Explicit send mode.
    pub fn mode(mut self, mode: SendMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Initial send level (linear).
    pub fn volume(mut self, volume: f64) -> Self {
        self.volume = Some(volume);
        self
    }

    /// Disable the track's parent/master send — the track's ONLY output
    /// becomes this send (bus-mic routing).
    pub fn replace_master_send(mut self) -> Self {
        self.replace_master = true;
        self
    }

    /// Create the send; returns its route index on the source track.
    pub fn apply(self) -> DawResult<u32> {
        let ctx = self.track.ctx.clone();
        let src = self.track.track_ref();
        let daw = self.track.daw;
        let idx = daw
            .add_send(ctx.clone(), src.clone(), TrackRef::Guid(self.dest.clone()))
            .ok_or_else(|| {
                DawError::Internal(format!(
                    "add_send {} → {} failed",
                    self.track.guid, self.dest
                ))
            })?;
        if let Some(mode) = self.mode {
            daw.set_send_mode(ctx.clone(), src.clone(), RouteRef::Index(idx), mode)?;
        }
        if let Some(volume) = self.volume {
            <D as Routing>::set_volume(
                daw,
                ctx.clone(),
                RouteLocation::send(src.clone(), RouteRef::Index(idx)),
                volume,
            )?;
        }
        if self.replace_master {
            daw.set_parent_send_enabled(ctx, src, false)?;
        }
        Ok(idx)
    }
}

/// Folder-hierarchy builder over the REAPER depth convention (positive depth
/// opens a folder; a negative depth on the **last child** closes levels).
/// Callers speak in `folder` / `track` / `end`; the builder owns the depth
/// bookkeeping that is otherwise fiddly to hand-roll (`-1` vs `-2` closes).
///
/// ```text
/// let mut tree = daw.current().tree();
/// let rig = tree.folder("Worship")?;      // opens
/// tree.folder("Keys")?;                    // nested folder
/// tree.track("Piano")?;
/// tree.track("Pad")?;
/// tree.end()?;                             // closes "Keys" on "Pad"
/// tree.folder("Organ")?;
/// tree.track("Organ")?;
/// tree.finish()?;                          // closes everything still open
/// ```
pub struct TrackTree<'d, D> {
    project: ProjectHandle<'d, D>,
    /// Folders currently open (unclosed positive depths).
    open: u32,
    /// The most recently added track + the closes accumulated on it.
    last: Option<(TrackHandle<'d, D>, i32)>,
}

impl<'d, D: Tracks> TrackTree<'d, D> {
    /// Flush the pending depth adjustment on the previous track.
    fn flush(&mut self) -> DawResult<()> {
        if let Some((track, depth)) = self.last.take() {
            if depth != 0 {
                track.set_folder_depth(depth)?;
            }
        }
        Ok(())
    }

    /// Open a folder track (children follow until [`end`](Self::end)).
    pub fn folder(&mut self, name: &str) -> DawResult<TrackHandle<'d, D>> {
        self.flush()?;
        let track = self.project.add_track(name)?;
        self.open += 1;
        self.last = Some((track.clone(), 1));
        Ok(track)
    }

    /// Add a plain track at the current level.
    pub fn track(&mut self, name: &str) -> DawResult<TrackHandle<'d, D>> {
        self.flush()?;
        let track = self.project.add_track(name)?;
        self.last = Some((track.clone(), 0));
        Ok(track)
    }

    /// Close the innermost open folder (the close lands on the most recent
    /// track, per the depth convention).
    pub fn end(&mut self) -> DawResult<()> {
        if self.open == 0 {
            return Err(DawError::Internal("TrackTree::end with no open folder".into()));
        }
        let Some((_, depth)) = self.last.as_mut() else {
            return Err(DawError::Internal("TrackTree::end before any track".into()));
        };
        *depth -= 1;
        self.open -= 1;
        Ok(())
    }

    /// Close every folder still open and flush. Call exactly once, last.
    pub fn finish(mut self) -> DawResult<()> {
        while self.open > 0 {
            self.end()?;
        }
        self.flush()
    }
}

#[cfg(test)]
mod tests {
    // The handles are pure delegation; behavior tests live with the
    // backends (daw-standalone's Tracks tests, signal-sampler's lane-layout
    // test drives TrackTree through a real Standalone).
}

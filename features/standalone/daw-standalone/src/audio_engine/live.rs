//! Live-monitor capability for [`AudioEngine`] — one input-armed track with a
//! swappable FX chain.
//!
//! The DAW audio engine is "just live capable": [`AudioEngine::open_live`]
//! stands up the whole "single live-monitor track" ritual that every live-input
//! consumer (guitar amp modeler, vocal-chain monitor, etc.) would otherwise
//! hand-assemble out of [`Standalone`] + [`AudioEngine`] internals — seed a tiny
//! project, add an input-armed track, reserve a fixed set of FX slots, open the
//! engine with a live input stream, install the per-track meter bank, and roll
//! the transport — and returns an `AudioEngine` carrying that live state. The
//! live methods ([`set_chain`](AudioEngine::set_chain),
//! [`input_peak`](AudioEngine::input_peak), …) act on it; on a plain playback
//! engine (no [`open_live`](AudioEngine::open_live)) they are no-ops.
//!
//! The result is **one input-armed track in a tiny daw project**: a single
//! track whose record input is a hardware channel
//! ([`RecordInput::Audio`](daw_proto::track::RecordInput::Audio)), whose FX
//! chain is whatever the caller installs via
//! [`set_chain`](AudioEngine::set_chain), routed to master. daw's
//! [`AudioEngine`] opens the output device + a live input stream and, every
//! block, mixes the armed input channel into the track's bus, runs its FX
//! chain, and outputs the result.
//!
//! ## Instant, GigPerformer-style chain switching
//!
//! The track reserves a fixed number of FX slots up front (constant guids, so
//! the project's `fx_chain` never changes → the renderer never rebuilds its
//! snapshot). The renderer pulls each slot's boxed [`PluginInstance`] out of
//! [`Standalone`]'s `plugin_instances` map **per block, under a lock**, so
//! swapping a *pre-prepared* box in under that lock
//! ([`Standalone::insert_plugin_instance`]) is glitch-free.
//! [`set_chain`](AudioEngine::set_chain) does exactly that: it drops the
//! caller's pre-prepared boxes into the reserved slot guids and fills the rest
//! with identity pass-throughs, returning the displaced boxes for off-thread
//! drop.
//!
//! ## Metering
//!
//! daw's renderer writes one post-fader peak per track into a [`Meters`] bank;
//! that is the **output** meter ([`output_peak`](AudioEngine::output_peak)). The
//! first reserved FX slot is an [`InputProbe`] pass-through that records the
//! **input** peak it sees (pre-FX) into a shared atomic
//! ([`input_peak`](AudioEngine::input_peak)).
//!
//! ## Example
//!
//! ```no_run
//! use daw_standalone::audio_engine::AudioEngine;
//! use daw_audio_io::AudioIoPrefs;
//!
//! // Open a live monitor on input channel 0 with up to 8 FX slots.
//! let prefs = AudioIoPrefs::default();
//! let engine = AudioEngine::open_live(&prefs, 0, 8).expect("open live engine");
//!
//! // Build a chain of pre-`prepare`d PluginInstances and install it.
//! // (each box must already be prepared at `engine.sample_rate()` /
//! //  `engine.prepare_block()`)
//! let chain: Vec<Box<dyn daw_standalone::plugin::PluginInstance>> = Vec::new();
//! let _displaced = engine.set_chain(chain); // drop off-thread
//!
//! // Read meters / drive the output level.
//! let _in = engine.input_peak();
//! let _out = engine.output_peak();
//! engine.set_output_gain_db(-3.0);
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use daw_audio_io::AudioIoPrefs;
use daw_proto::{
    FxChainContext, FxChains, ProjectContext, ProjectInfo, RecordInput, TrackRef, Tracks,
};

use crate::audio_engine::mixer::AudioEngine;
use crate::metering::Meters;
use crate::plugin::{
    PluginDescriptor, PluginError, PluginEvents, PluginFormat, PluginInstance, PluginParamInfo,
};
use crate::sync::Standalone;
use crate::transport_engine::{PlayStateRepr, TransportShared};

/// Block size [`AudioEngine::open_live`] prepares its built-in probe/identity
/// instances for, and the size callers should `prepare` their own FX chain
/// boxes at. daw's
/// callback block is normally 64–1024 frames; preparing for a larger ceiling
/// keeps the chain safe against bigger backend buffers without per-block
/// re-preparation.
pub const LIVE_PREPARE_BLOCK: u32 = 4096;

/// Project / track names for the live rig's tiny daw project.
const LIVE_PROJECT_NAME: &str = "Live Rig";
const LIVE_TRACK_NAME: &str = "Live In";

fn db_to_lin(db: f32) -> f32 {
    if db == 0.0 { 1.0 } else { 10f32.powf(db / 20.0) }
}

// ── Shared input-meter state written by the audio-thread probe ───────────────

/// Shared atomic the [`InputProbe`] writes the per-block input peak into, read
/// by [`AudioEngine::input_peak`]. Held by both the engine's [`LiveState`]
/// (reader) and the probe instance (audio-thread writer). Lock-free: a single
/// `f32`-as-bits atomic.
#[derive(Debug, Default)]
pub(crate) struct InputMeterShared {
    /// Latest block input peak (linear), as `f32` bits.
    peak: AtomicU32,
}

impl InputMeterShared {
    fn store(&self, peak: f32) {
        self.peak.store(peak.to_bits(), Ordering::Relaxed);
    }
    fn load(&self) -> f32 {
        f32::from_bits(self.peak.load(Ordering::Relaxed))
    }
}

/// A unity pass-through [`PluginInstance`] that records the input peak it sees.
/// Sits at slot 0 of the live track so the UI gets a pre-FX input meter (daw's
/// renderer only meters the post-fader *output* per track). Pure copy + max —
/// cheap.
pub struct InputProbe {
    shared: Arc<InputMeterShared>,
    prepared: bool,
}

impl InputProbe {
    fn new(shared: Arc<InputMeterShared>) -> Self {
        Self { shared, prepared: true }
    }
}

impl PluginInstance for InputProbe {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: "daw.live.input_probe".into(),
            name: "Input".into(),
            vendor: "daw".into(),
            version: String::new(),
            format: PluginFormat::Synthetic,
        }
    }
    fn params(&mut self) -> Vec<PluginParamInfo> {
        Vec::new()
    }
    fn param_value(&mut self, _id: u32) -> Option<f64> {
        None
    }
    fn value_to_text(&mut self, _id: u32, _v: f64) -> Option<String> {
        None
    }
    fn text_to_value(&mut self, _id: u32, _t: &str) -> Option<f64> {
        None
    }
    fn latency(&mut self) -> u32 {
        0
    }
    fn prepare(&mut self, _sr: f64, _bs: u32) -> Result<(), PluginError> {
        self.prepared = true;
        Ok(())
    }
    fn is_prepared(&self) -> bool {
        self.prepared
    }
    fn process_block(
        &mut self,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
        _events: &PluginEvents<'_>,
    ) -> Result<(), PluginError> {
        let frames = out_l.len().min(out_r.len()).min(in_l.len()).min(in_r.len());
        let mut pk = 0.0f32;
        for i in 0..frames {
            out_l[i] = in_l[i];
            out_r[i] = in_r[i];
            pk = pk.max(in_l[i].abs()).max(in_r[i].abs());
        }
        self.shared.store(pk);
        Ok(())
    }
    fn deactivate(&mut self) {
        self.prepared = false;
    }
}

/// A unity pass-through [`PluginInstance`] used to fill a live track's unused FX
/// slots (chains shorter than the reserved slot count). Copies input → output.
pub struct Identity {
    prepared: bool,
}

impl Identity {
    /// A fresh identity pass-through (starts in the prepared state).
    pub fn new() -> Self {
        Self { prepared: true }
    }
}

impl Default for Identity {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginInstance for Identity {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: "daw.live.identity".into(),
            name: "—".into(),
            vendor: "daw".into(),
            version: String::new(),
            format: PluginFormat::Synthetic,
        }
    }
    fn params(&mut self) -> Vec<PluginParamInfo> {
        Vec::new()
    }
    fn param_value(&mut self, _id: u32) -> Option<f64> {
        None
    }
    fn value_to_text(&mut self, _id: u32, _v: f64) -> Option<String> {
        None
    }
    fn text_to_value(&mut self, _id: u32, _t: &str) -> Option<f64> {
        None
    }
    fn latency(&mut self) -> u32 {
        0
    }
    fn prepare(&mut self, _sr: f64, _bs: u32) -> Result<(), PluginError> {
        self.prepared = true;
        Ok(())
    }
    fn is_prepared(&self) -> bool {
        self.prepared
    }
    fn process_block(
        &mut self,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
        _events: &PluginEvents<'_>,
    ) -> Result<(), PluginError> {
        let frames = out_l.len().min(out_r.len()).min(in_l.len()).min(in_r.len());
        out_l[..frames].copy_from_slice(&in_l[..frames]);
        out_r[..frames].copy_from_slice(&in_r[..frames]);
        Ok(())
    }
    fn deactivate(&mut self) {
        self.prepared = false;
    }
}

/// Live-monitor state owned by an [`AudioEngine`] opened via
/// [`AudioEngine::open_live`]. Holds the input-armed [`Standalone`] track, its
/// post-fader meter bank, the reserved FX-slot guids, and the input-probe
/// atomic. Present only on a live engine; a plain playback engine carries
/// `None` and its live methods are no-ops.
pub(crate) struct LiveState {
    pub(crate) daw: Standalone,
    #[allow(dead_code)]
    pub(crate) shared: Arc<TransportShared>,
    pub(crate) meters: Arc<Meters>,
    #[allow(dead_code)]
    pub(crate) project_guid: String,
    pub(crate) track_guid: String,
    /// Fixed fx-guids for the reserved slots. Index 0 is the [`InputProbe`];
    /// `1..` carry the installed chain (identities fill the unused tail).
    pub(crate) slot_guids: Vec<String>,
    /// Input-meter atomic written by the [`InputProbe`] at slot 0.
    pub(crate) input_meter: Arc<InputMeterShared>,
}

/// A fresh identity pass-through box, prepared at `sr` for [`LIVE_PREPARE_BLOCK`].
fn fresh_identity(sr: f64) -> Box<dyn PluginInstance> {
    let mut id = Identity::new();
    let _ = id.prepare(sr, LIVE_PREPARE_BLOCK);
    Box::new(id)
}

/// Live-monitor capability folded onto [`AudioEngine`]: open an input-armed
/// track with a swappable FX chain and metering. The DAW audio engine is "just
/// live capable" — [`open_live`](AudioEngine::open_live) returns an engine with
/// `live: Some(_)`; the rest of the live API ([`set_chain`](AudioEngine::set_chain),
/// [`input_peak`](AudioEngine::input_peak), [`output_peak`](AudioEngine::output_peak),
/// [`set_output_gain_db`](AudioEngine::set_output_gain_db),
/// [`prepare_block`](AudioEngine::prepare_block)) is meaningful only for such an
/// engine and no-ops / returns `0.0` on a plain playback engine.
impl AudioEngine {
    /// Open a live-monitor engine: one input-armed track tapping input
    /// `channel` of the device in `prefs`, with `slot_count` reserved FX slots
    /// (initially identity pass-throughs), routed to master, transport rolling.
    /// Opens the output device + a live input stream. The returned engine has
    /// its live state installed (`live: Some(_)`).
    ///
    /// `prefs.want_input` is forced on (a live engine is duplex by definition).
    /// `slot_count` is the number of *chain* slots available to
    /// [`set_chain`](Self::set_chain); an extra hidden slot holds the input
    /// probe. The resolved device sample rate is available via
    /// [`sample_rate`](Self::sample_rate).
    pub fn open_live(
        prefs: &AudioIoPrefs,
        channel: u32,
        slot_count: usize,
    ) -> Result<Self, String> {
        let daw = Standalone::new();

        // 1. Seed a one-track project; make it current so the FX-chain service
        //    (which resolves the current project) targets it.
        let project_guid = format!(
            "live-rig-{:x}-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
            std::process::id(),
        );
        daw.seed_project(ProjectInfo {
            guid: project_guid.clone(),
            name: LIVE_PROJECT_NAME.to_string(),
            path: String::new(),
        });
        daw.set_current_project(&project_guid);

        let track_guid =
            <Standalone as Tracks>::add(&daw, ProjectContext::Current, LIVE_TRACK_NAME, None)
                .map_err(|e| format!("live rig: add track failed: {e}"))?;

        // 2. Arm the track to monitor the hardware input channel — this is what
        //    makes daw's engine open a live input stream and feed the bus.
        <Standalone as Tracks>::set_record_input(
            &daw,
            ProjectContext::Current,
            TrackRef::guid(track_guid.as_str()),
            RecordInput::Audio { channel },
        )
        .map_err(|e| format!("live rig: set record input failed: {e}"))?;

        // 3. Reserve the fixed FX slots on the track (constant guids). Slot 0 is
        //    the input probe; the rest start as identity pass-throughs.
        let fx_ctx = FxChainContext::track(track_guid.clone());
        let mut slot_guids = Vec::with_capacity(slot_count + 1);
        for i in 0..=slot_count {
            let idx = <Standalone as FxChains>::add(&daw, fx_ctx.clone(), &format!("live-slot-{i}"))
                .map_err(|e| format!("live rig: reserve fx slot {i} failed: {e}"))?;
            let guid = <Standalone as FxChains>::get(&daw, fx_ctx.clone(), idx)
                .map(|fx| fx.guid)
                .ok_or_else(|| format!("live rig: fx slot {i} vanished after add"))?;
            slot_guids.push(guid);
        }

        // 4. Build the shared transport + audio engine with live input.
        let req_rate = prefs.sample_rate_opt().unwrap_or(48_000);
        let shared = Arc::new(TransportShared::new(req_rate, 120.0));
        let mut io = prefs.clone();
        io.want_input = true;
        let mut engine =
            AudioEngine::with_project_prefs(daw.clone(), project_guid.clone(), shared.clone(), &io)
                .map_err(|e| format!("live rig: audio engine failed: {e}"))?;
        let sample_rate = engine.sample_rate();

        // Per-track meter bank (one cell, post-fader output peak). Install AFTER
        // the engine opens so it's sized for the device; the renderer reads
        // `daw.meters()` fresh each block.
        let meters = Meters::new(1);
        daw.set_meters(meters.clone());

        // 5. Populate the reserved slots: input probe at 0, identities elsewhere.
        let input_meter = Arc::new(InputMeterShared::default());
        {
            let mut probe = InputProbe::new(input_meter.clone());
            let _ = probe.prepare(sample_rate as f64, LIVE_PREPARE_BLOCK);
            daw.insert_plugin_instance(slot_guids[0].clone(), Box::new(probe));
            for guid in &slot_guids[1..] {
                daw.insert_plugin_instance(guid.clone(), fresh_identity(sample_rate as f64));
            }
        }

        // 6. Roll the transport so the renderer runs (and the live input flows
        //    through the chain to master) every block.
        shared.set_play_state(PlayStateRepr::Playing);

        // 7. Install the live state on the engine and hand it back.
        engine.live = Some(LiveState {
            daw,
            shared,
            meters,
            project_guid,
            track_guid,
            slot_guids,
            input_meter,
        });
        Ok(engine)
    }

    /// Replace the reserved live slots' FX with `chain` (each [`PluginInstance`]
    /// must be pre-`prepare`d by the caller at [`sample_rate`](Self::sample_rate)
    /// / [`prepare_block`](Self::prepare_block); the remaining slots are filled
    /// with identities). Glitch-free: swaps boxes under the renderer's per-block
    /// lock. Returns the displaced boxes for off-thread drop.
    ///
    /// A `chain` longer than the reserved slot count is truncated (the extra
    /// boxes are returned in the displaced vec, undropped). **Only meaningful
    /// for an engine opened via [`open_live`](Self::open_live)** — on a plain
    /// playback engine this returns `chain` unchanged.
    #[must_use = "the returned boxes should be dropped off the audio thread"]
    pub fn set_chain(
        &self,
        chain: Vec<Box<dyn PluginInstance>>,
    ) -> Vec<Box<dyn PluginInstance>> {
        let Some(live) = self.live.as_ref() else {
            return chain;
        };
        let chain_guids = &live.slot_guids[1..]; // slot 0 is the input probe
        let sr = self.sample_rate() as f64;
        let mut displaced = Vec::new();
        let mut it = chain.into_iter();
        for guid in chain_guids {
            let new_box = match it.next() {
                Some(b) => b,
                None => fresh_identity(sr),
            };
            if let Some(old) = live.daw.insert_plugin_instance(guid.clone(), new_box) {
                displaced.push(old);
            }
        }
        // Any chain boxes beyond the reserved slot count: hand them back too.
        displaced.extend(it);
        displaced
    }

    /// Pre-FX input peak (linear) — from the built-in [`InputProbe`] at slot 0.
    /// Only meaningful for an engine opened via [`open_live`](Self::open_live);
    /// `0.0` otherwise.
    pub fn input_peak(&self) -> f32 {
        self.live.as_ref().map(|l| l.input_meter.load()).unwrap_or(0.0)
    }

    /// Post-fader output peak (linear) — the live track's meter cell. Only
    /// meaningful for an engine opened via [`open_live`](Self::open_live); `0.0`
    /// otherwise.
    pub fn output_peak(&self) -> f32 {
        self.live
            .as_ref()
            .and_then(|l| l.meters.cell(0))
            .map(|c| c.peak(0).max(c.peak(1)))
            .unwrap_or(0.0)
    }

    /// Set the live track's post-fader output gain (dB → linear volume). No-op
    /// on a plain playback engine (not opened via [`open_live`](Self::open_live)).
    pub fn set_output_gain_db(&self, db: f32) {
        if let Some(live) = self.live.as_ref() {
            let _ = <Standalone as Tracks>::set_volume(
                &live.daw,
                ProjectContext::Current,
                TrackRef::guid(live.track_guid.as_str()),
                db_to_lin(db) as f64,
            );
        }
    }

    /// The block size callers should `prepare` their live FX chain boxes for.
    pub fn prepare_block(&self) -> u32 {
        LIVE_PREPARE_BLOCK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(probe: &mut dyn PluginInstance, signal: &[f32]) -> Vec<f32> {
        let mut ol = vec![0.0f32; signal.len()];
        let mut or = vec![0.0f32; signal.len()];
        probe
            .process_block(signal, signal, &mut ol, &mut or, &PluginEvents::EMPTY)
            .unwrap();
        ol
    }

    #[test]
    fn identity_passthrough_copies_input() {
        let mut id = Identity::new();
        let il = [0.1, -0.2, 0.3];
        let out = drain(&mut id, &il);
        assert_eq!(out.as_slice(), il.as_slice());
    }

    #[test]
    fn input_probe_records_block_peak() {
        let shared = Arc::new(InputMeterShared::default());
        let mut probe = InputProbe::new(shared.clone());
        let il = [0.1, -0.8, 0.3];
        let out = drain(&mut probe, &il);
        assert!((shared.load() - 0.8).abs() < 1e-6, "probe records block peak");
        assert_eq!(out.as_slice(), il.as_slice(), "probe passes input through");
    }

    #[test]
    fn db_to_lin_is_unity_at_zero() {
        assert_eq!(db_to_lin(0.0), 1.0);
        assert!((db_to_lin(-6.0206) - 0.5).abs() < 1e-4);
    }
}

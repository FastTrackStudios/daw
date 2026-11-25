//! Format-agnostic plugin abstraction.
//!
//! The renderer + Effects service talk to plugins through
//! [`PluginInstance`]. CLAP is the only format wired today; VST3 and
//! LV2 backends slot in by implementing the same trait.
//!
//! The trait is intentionally minimal — it covers what the project
//! renderer needs (prepare / process_block / param query) plus the
//! UI surface (descriptor, value↔text). Format-specific extras
//! (state chunks, view embedding, MIDI ports) live as inherent
//! methods on the concrete backends.

use std::fmt;
use std::path::Path;

/// Plugin format identifier — used by `Effects::add` to pick the
/// right backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PluginFormat {
    Clap,
    Vst3,
    Lv2,
    /// Synthetic placeholder (8 generic params, no DSP). Default
    /// when no real backend is available.
    #[default]
    Synthetic,
}

impl PluginFormat {
    /// Sniff the format from a path or identifier string.
    /// - `*.clap` → Clap
    /// - `*.vst3` → Vst3
    /// - `*.lv2` → Lv2
    /// - everything else → Synthetic
    pub fn from_path_or_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".clap") {
            Self::Clap
        } else if lower.ends_with(".vst3") {
            Self::Vst3
        } else if lower.ends_with(".lv2") {
            Self::Lv2
        } else {
            Self::Synthetic
        }
    }
}

/// Plugin-format-neutral descriptor.
#[derive(Clone, Debug, Default)]
pub struct PluginDescriptor {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub format: PluginFormat,
}

/// A MIDI event scheduled at a sample offset inside the current
/// process block. Used to drive virtual instruments (CLAPi/VST3i)
/// and to feed effect plugins that respond to CC/program changes.
#[derive(Clone, Debug)]
pub struct PluginMidiEvent {
    /// Sample offset from the start of the block (0..block_size).
    pub offset: u32,
    pub message: daw_proto::MidiEvent,
}

/// One per-note expression event scheduled at a sample offset.
/// Surfaces CLAP's `NoteExpressionEvent` and VST3's
/// `NoteExpressionValueEvent` through a single host-side type — the
/// renderer emits these alongside MIDI; backends translate to the
/// format-native event.
/// A parameter event at a frame of the block: what
/// [`PluginInstance::process_block_param_events`] takes. Sample-accurate,
/// and with modulation as well as values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PluginParamEvent {
    pub offset: u32,
    pub param_id: u32,
    pub kind: PluginParamEventKind,
}

/// A value, or a modulation offset that keeps the parameter's own value
/// (`CLAP_EVENT_PARAM_MOD`). Both in the plugin's parameter domain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PluginParamEventKind {
    Value(f64),
    Mod(f64),
}

#[derive(Clone, Copy, Debug)]
pub struct PluginNoteExpression {
    pub offset: u32,
    pub channel: u8,
    /// Target note pitch, or `0xFF` for "any active voice".
    pub note: u8,
    pub dimension: daw_proto::midi::NoteExpressionDim,
    pub value: f64,
}

/// Per-block inputs to [`PluginInstance::process_block`].
///
/// Borrowed slices so the renderer can pass scratch buffers without
/// extra allocation. An empty `PluginEvents` is the common case
/// for static effects with no automation or note input.
#[derive(Clone, Copy, Debug, Default)]
pub struct PluginEvents<'a> {
    /// `(param_id, plain_value)` changes applied at sample 0.
    pub params: &'a [(u32, f64)],
    /// Sorted-by-offset MIDI events for this block.
    pub midi: &'a [PluginMidiEvent],
    /// Sorted-by-offset per-note expression events for this block.
    pub note_expressions: &'a [PluginNoteExpression],
}

/// Per-block inputs to [`PluginInstance::process_block_timed`]: what
/// [`PluginEvents`] carries, with each parameter change at its frame.
///
/// `params` is sorted by offset, at most one event per parameter per
/// frame, each value in the parameter's plain domain. It restates every
/// parameter the block touches at frame 0, so a plugin that loses its
/// event state between blocks still holds the right values.
#[derive(Clone, Copy, Debug, Default)]
pub struct TimedPluginEvents<'a> {
    pub params: &'a [PluginParamEvent],
    /// Sorted-by-offset MIDI events for this block.
    pub midi: &'a [PluginMidiEvent],
    /// Sorted-by-offset per-note expression events for this block.
    pub note_expressions: &'a [PluginNoteExpression],
}

impl<'a> PluginEvents<'a> {
    pub const EMPTY: PluginEvents<'static> = PluginEvents {
        params: &[],
        midi: &[],
        note_expressions: &[],
    };
}

/// One parameter exposed by the plugin.
#[derive(Clone, Debug)]
pub struct PluginParamInfo {
    pub id: u32,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
}

/// Factory turning a built-in FX name/ident into a live DSP
/// [`PluginInstance`].
///
/// The built-in FX themselves live ABOVE this crate (`signal-fx` wraps
/// the DSP cores, and depends on the `daw` facade — a direct dep here
/// would be a cycle), so the factory is **injected**: the app installs
/// one via `Standalone::set_fx_factory`, and `Effects::add` consults
/// it for any name the plugin-file backends (CLAP/VST3) don't claim —
/// the same `plugin_instances` seam signal rigs already use via
/// `Standalone::insert_plugin_instance`.
pub trait FxFactory: Send + Sync {
    /// Catalog of creatable built-ins. Appended to
    /// `Effects::list_installed` so browsers can offer them.
    fn installed(&self) -> Vec<daw_proto::fx::InstalledFx>;

    /// Instantiate `name_or_ident` at `sample_rate` (the instance is
    /// re-`prepare`d at the stream's real rate by the renderer).
    /// `None` if the name isn't one of this factory's built-ins.
    fn create(&self, name_or_ident: &str, sample_rate: f64) -> Option<Box<dyn PluginInstance>>;

    /// Whether `name_or_ident` is one of this factory's built-ins —
    /// without instantiating it.
    ///
    /// The project loader asks this before it goes looking for a plugin
    /// bundle on disk (which a built-in never has), so an FX a saved
    /// project names by its factory name comes back through
    /// [`create`](Self::create). The default matches the
    /// [`installed`](Self::installed) catalog by name or ident; a factory
    /// that recognises more spellings than it lists overrides it.
    fn provides(&self, name_or_ident: &str) -> bool {
        self.installed()
            .iter()
            .any(|fx| fx.name == name_or_ident || fx.ident == name_or_ident)
    }
}

/// A boxed, format-neutral plugin instance.
///
/// All methods take `&mut self` because plugin internals are
/// inherently stateful (sample rate, activation state, parameter
/// values). The renderer holds these inside a `Mutex` and acquires
/// it per audio callback.
pub trait PluginInstance: Send {
    /// Metadata about the plugin (id / name / vendor / format).
    fn descriptor(&self) -> PluginDescriptor;

    /// All parameters this plugin exposes.
    fn params(&mut self) -> Vec<PluginParamInfo>;

    /// Current value of a parameter, if the plugin can report it.
    fn param_value(&mut self, id: u32) -> Option<f64>;

    /// Set a parameter (plain value) in the plugin itself, now, whether
    /// or not it is processing: the plugin's own value, its state and
    /// what it reports follow at once. Control threads only. Whether the
    /// plugin took it; `false` (the default) for a plugin whose
    /// parameters are only the values sent with each block.
    fn set_param_value(&mut self, _id: u32, _value: f64) -> bool {
        false
    }

    /// Format the value for display (e.g. `"-12 dB"`). `None` if the
    /// plugin doesn't provide a formatter.
    fn value_to_text(&mut self, id: u32, value: f64) -> Option<String>;

    /// Inverse of `value_to_text` — parse `"−12 dB"` back to a value.
    fn text_to_value(&mut self, id: u32, text: &str) -> Option<f64>;

    /// A stepped parameter's choices — a mode, a switch, a scene: each
    /// its value normalized along the parameter's range (0..1) and its
    /// name. What a host lists to pick from and parses a name against.
    /// Empty (the default) for a continuous parameter.
    fn param_choices(&mut self, _id: u32) -> Vec<(f64, String)> {
        Vec::new()
    }

    /// Reported plugin latency in samples (0 if not reported). The
    /// renderer's delay compensation reads it on the audio thread, every
    /// block: a backend whose plugin API answers this on a main thread
    /// only (CLAP) keeps the last answer and returns that.
    fn latency(&mut self) -> u32;

    /// Service a restart the plugin asked for (its latency changed while
    /// it was active): deactivate and activate it again, its latency asked
    /// anew. Control threads only — a state load, an FX edit. Whether it
    /// was restarted; nothing to do by default.
    fn restart_if_requested(&mut self) -> bool {
        false
    }

    /// Activate the plugin at the given audio config. Must be called
    /// before any `process_block` calls.
    fn prepare(&mut self, sample_rate: f64, block_size: u32) -> Result<(), PluginError>;

    /// Whether `prepare` has been called and the plugin is ready.
    fn is_prepared(&self) -> bool;

    /// The sample rate it was prepared at, when the backend says: a plugin
    /// activated ahead of its first block (at load) at a rate the renderer
    /// does not run at is prepared again there. `None` (the default) is
    /// never re-prepared for its rate.
    fn prepared_rate(&self) -> Option<f64> {
        None
    }

    /// Whether this plugin may process on a render worker thread, at the
    /// same time as other tracks' plugins. Only a plugin that shares no
    /// per-block state with any other track may say yes: the guide
    /// instruments, which hand cues between tracks keyed by
    /// [`render_cycle`], must run on the render thread in track order. A
    /// track is rendered in parallel only when every plugin on it says yes.
    fn parallel_safe(&self) -> bool {
        false
    }

    /// Render one block. `events` carries per-block parameter changes
    /// and MIDI input (needed for virtual instruments like CLAPi /
    /// VST3i, which produce audio from note events rather than
    /// processing an audio input).
    fn process_block(
        &mut self,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
        events: &PluginEvents<'_>,
    ) -> Result<(), PluginError>;

    /// [`process_block`](Self::process_block) with sample-accurate
    /// parameter events: values and modulation offsets, each at its frame
    /// (`params` and `midi` each sorted by offset). Signal's CLAP backend
    /// drives a block's CLAP form with it (signal#26).
    ///
    /// The default takes values at frame 0 only, and refuses the rest: a
    /// backend that can't place an event or modulate says so rather than
    /// playing it at the wrong frame.
    fn process_block_param_events(
        &mut self,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
        params: &[PluginParamEvent],
        midi: &[PluginMidiEvent],
    ) -> Result<(), PluginError> {
        let mut values = Vec::with_capacity(params.len());
        for p in params {
            match p.kind {
                PluginParamEventKind::Value(v) if p.offset == 0 => values.push((p.param_id, v)),
                _ => return Err(PluginError::UnsupportedFormat(self.descriptor().format)),
            }
        }
        let events = PluginEvents {
            params: &values,
            midi,
            note_expressions: &[],
        };
        self.process_block(in_l, in_r, out_l, out_r, &events)
    }

    /// Whether [`process_block_timed`](Self::process_block_timed) places
    /// each parameter event at its frame. The renderer then plays an
    /// automation lane as events inside the block (sample-accurate);
    /// otherwise it sends each lane's value at the block's start through
    /// [`process_block`](Self::process_block). `false` by default.
    fn takes_timed_params(&self) -> bool {
        false
    }

    /// [`process_block`](Self::process_block) with each parameter change
    /// at its frame. Called only on a plugin whose
    /// [`takes_timed_params`](Self::takes_timed_params) is `true`; the
    /// default refuses, rather than playing the events at the wrong frame.
    fn process_block_timed(
        &mut self,
        _in_l: &[f32],
        _in_r: &[f32],
        _out_l: &mut [f32],
        _out_r: &mut [f32],
        _events: &TimedPluginEvents<'_>,
    ) -> Result<(), PluginError> {
        Err(PluginError::UnsupportedFormat(self.descriptor().format))
    }

    /// What the plugin said of its last block (CLAP's process status):
    /// whether the renderer may stop calling it while it hears silence
    /// ([`crate::plugin_sleep`]). `Continue` (the default) is never put to
    /// sleep.
    fn process_status(&self) -> crate::plugin_sleep::ProcessStatus {
        crate::plugin_sleep::ProcessStatus::Continue
    }

    /// Its tail now (`clap_plugin_tail`), asked from the audio thread after
    /// a block that said [`ProcessStatus::Tail`](crate::plugin_sleep::ProcessStatus::Tail).
    fn tail(&mut self) -> crate::plugin_sleep::Tail {
        crate::plugin_sleep::Tail::Infinite
    }

    /// The plugin's `com.fasttrackstudio.signal/1`, if it has it.
    #[cfg(feature = "clap-host")]
    fn signal(&self) -> Option<daw_clap_ext::host::SignalPluginClient> {
        None
    }

    /// Run what the plugin asked its host's main thread to (a CLAP's
    /// `request_callback`). The embedder calls it from its main thread now
    /// and then.
    fn pump_main_thread(&mut self) {}

    /// Whether the plugin asked for [`pump_main_thread`](Self::pump_main_thread)
    /// since it last ran (a CLAP's `request_callback`). `false` from a
    /// backend that never asks.
    fn wants_main_thread(&self) -> bool {
        false
    }

    /// Where the host delivers the events this plugin subscribed to
    /// (`com.fasttrackstudio.host-events/1`), if it has it. Main thread.
    #[cfg(feature = "clap-host")]
    fn event_sink(&self) -> Option<daw_clap_ext::host::EventSink> {
        None
    }

    /// Bind the plugin's `com.fasttrackstudio.host-timeline/1` to its
    /// track's timeline (or unbind it with `None`). `false` from a backend
    /// or instance the extension wasn't offered to.
    #[cfg(feature = "clap-host")]
    fn bind_timeline(
        &mut self,
        timeline: Option<std::sync::Arc<dyn daw_clap_ext::host::HostTimeline>>,
    ) -> bool {
        let _ = timeline;
        false
    }

    /// Whether [`bind_timeline`](Self::bind_timeline) bound it to a track.
    #[cfg(feature = "clap-host")]
    fn timeline_bound(&self) -> bool {
        false
    }

    /// Increases each time the plugin renames or re-describes its
    /// parameters (a CLAP's rescan of info or text): re-read
    /// [`params`](Self::params) when it moves. 0 from a backend that never
    /// does.
    fn param_info_revision(&self) -> u64 {
        0
    }

    /// What the plugin tells its host between calls, readable from any
    /// thread without the plugin's lock ([`PluginNotices`]): kept by its
    /// cell when it is loaded, so the host knows what changed without
    /// waiting for a render to let the plugin go. `None` (the default)
    /// from a backend that tells nothing.
    fn notices(&self) -> Option<std::sync::Arc<PluginNotices>> {
        None
    }

    /// The sidechain for the next [`process_block`](Self::process_block)
    /// only: what the track received on channels 3/4 this block (a
    /// drum's trigger keying its gate). Not called on a block with
    /// nothing there, so the plugin hears silence. Ignored by default —
    /// a plugin without a sidechain input has nowhere to put it.
    fn set_sidechain(&mut self, _left: &[f32], _right: &[f32]) {}

    /// An output beyond the main pair, after a
    /// [`process_block`](Self::process_block): pair 0 is the track's
    /// channels 3/4, pair 1 its 5/6 — what a send from those channels
    /// takes (a kit's overhead mics on 3/4, sent into the overheads).
    /// `None` for a pair the plugin does not have, which is every pair by
    /// default.
    fn aux_out(&self, _pair: usize) -> Option<(&[f32], &[f32])> {
        None
    }

    /// The song's tempo for the next [`process_block`](Self::process_block),
    /// bpm, from the project's tempo map at the block's start: what a
    /// plugin's note-valued times read (a synced delay, a reverb's tail or
    /// pre-delay in notes). A renderer that knows the song calls it before
    /// every block; a plugin with no use for a tempo ignores it.
    fn set_tempo(&mut self, _bpm: f64) {}

    /// Restore plugin state from a host-defined byte blob. The
    /// blob format is backend-specific — for round-trip safety the
    /// blob must have been produced by [`save_state`] from a plugin
    /// of the same backend, or built from a host-side parser (see
    /// [`crate::rpp_state`] for REAPER `.rpp` chunk decoding).
    ///
    /// Calling this on an *activated* plugin is allowed but
    /// implementation-defined — VST3 plugins generally tolerate it,
    /// CLAP plugins typically need a deactivate/load/activate cycle
    /// for some param changes to apply.
    fn load_state(&mut self, _state: &[u8]) -> Result<(), PluginError> {
        Err(PluginError::UnsupportedFormat(self.descriptor().format))
    }

    /// Save plugin state to a host-defined byte blob suitable for
    /// passing back to [`load_state`] on a fresh instance of the same
    /// backend.
    fn save_state(&mut self) -> Result<Vec<u8>, PluginError> {
        Err(PluginError::UnsupportedFormat(self.descriptor().format))
    }

    /// Open the plugin's editor UI, if the backend/plugin supports it.
    fn open_gui(&mut self) -> Result<(), PluginError> {
        Err(PluginError::UnsupportedFormat(self.descriptor().format))
    }

    /// Close a previously-opened plugin editor UI. Backends that do
    /// not support plugin editors may treat this as a no-op.
    fn close_gui(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    /// Release activation. Idempotent — calling twice is fine.
    fn deactivate(&mut self);

    /// Clear what the plugin carries from the audio it processed — its
    /// buffers, voices, envelopes, filters' memory — staying activated, so
    /// the next block starts as from silence without the cost of
    /// activating again. Whether it could: `false` (the default) for a
    /// plugin that cannot, which its caller then deactivates instead.
    fn reset(&mut self) -> bool {
        false
    }

    /// Downcast hook for native (built-in Rust DSP) instances whose control
    /// surface isn't expressed as host params. Hosts that mutate a concrete
    /// instance in place (e.g. Signal's NAM amp trims via
    /// [`Standalone::with_plugin_instance`](crate::sync::Standalone::with_plugin_instance))
    /// downcast through this. Defaults to `None` for hosted CLAP/VST3 plugins,
    /// which expose their parameters through the host param path instead.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
}

/// The largest block a renderer ever hands a plugin: the render ahead's
/// run-start chunks are 4096 frames, a device block can reach 8192. A
/// plugin is activated for this many whatever block it is first run at,
/// so a bigger block later is never refused.
pub const PREPARE_FRAMES: u32 = 8192;

/// Errors any plugin backend can raise.
#[derive(Debug)]
pub enum PluginError {
    /// Path didn't resolve or the bundle couldn't be opened.
    LoadFailed(String),
    /// `process_block` called before `prepare`.
    NotActivated,
    /// Activation failed (plugin rejected the config).
    ActivateFailed(String),
    /// Block size exceeded the prepared max.
    BlockTooLarge,
    /// Backend reported an error during processing.
    ProcessFailed(String),
    /// Backend not compiled in (e.g. `vst3` backend requested
    /// without the `vst3-host` feature).
    UnsupportedFormat(PluginFormat),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LoadFailed(s) => write!(f, "plugin load failed: {s}"),
            Self::NotActivated => write!(f, "plugin not activated"),
            Self::ActivateFailed(s) => write!(f, "plugin activation failed: {s}"),
            Self::BlockTooLarge => write!(f, "process block exceeds prepared size"),
            Self::ProcessFailed(s) => write!(f, "plugin process failed: {s}"),
            Self::UnsupportedFormat(fmt_) => {
                write!(f, "plugin format not supported in this build: {fmt_:?}")
            }
        }
    }
}

impl std::error::Error for PluginError {}

pub use thread_serialized::ThreadSerialized;

#[allow(unsafe_code)]
mod thread_serialized {
    /// Marker wrapper that asserts (via unsafe) that the wrapped value
    /// is safe to send between threads as long as all access is
    /// serialized externally (e.g. through a `Mutex`).
    ///
    /// Lives here (always-on) so format-specific host modules can share
    /// it without depending on each other's feature gates.
    pub struct ThreadSerialized<T>(T);

    impl<T> ThreadSerialized<T> {
        /// Construct a `ThreadSerialized<T>` wrapper.
        ///
        /// # Safety
        ///
        /// The caller must guarantee that the wrapped value is only ever
        /// accessed from one thread at a time. The DAW enforces this by
        /// storing instances inside `Mutex<HashMap<..>>`.
        pub unsafe fn new(value: T) -> Self {
            Self(value)
        }
    }

    unsafe impl<T> Send for ThreadSerialized<T> {}

    impl<T> core::ops::Deref for ThreadSerialized<T> {
        type Target = T;
        fn deref(&self) -> &T {
            &self.0
        }
    }
    impl<T> core::ops::DerefMut for ThreadSerialized<T> {
        fn deref_mut(&mut self) -> &mut T {
            &mut self.0
        }
    }
}

/// Format-aware instantiator. Tries to load `name_or_path` using the
/// first backend that recognizes the format. Returns
/// `PluginError::UnsupportedFormat` if no backend can handle it.
///
/// `Synthetic` returns `Ok(None)` — caller falls back to the
/// existing synthetic FX path (see `crate::fx`).
pub fn load_plugin(name_or_path: &str) -> Result<Option<Box<dyn PluginInstance>>, PluginError> {
    load_plugin_with(name_or_path, &LoadOptions::default())
}

/// What [`load_plugin_with`] offers the plugin.
#[derive(Clone, Default)]
pub struct LoadOptions {
    /// The host's actions, offered to a CLAP as
    /// `com.fasttrackstudio.host-actions/1` (signal#26).
    #[cfg(feature = "clap-host")]
    pub actions: Option<std::sync::Arc<dyn daw_clap_ext::host::HostActions>>,
    /// The host's automation, offered to a CLAP as
    /// `com.fasttrackstudio.host-automation/1`: its own parameters' lanes.
    #[cfg(feature = "clap-host")]
    pub automation: Option<std::sync::Arc<dyn daw_clap_ext::automation::HostAutomation>>,
    /// Offer a CLAP `com.fasttrackstudio.host-timeline/1`; the embedder
    /// binds each instance to its track after it is placed
    /// (`PluginInstance::bind_timeline`).
    #[cfg(feature = "clap-host")]
    pub timeline: bool,
}

/// [`load_plugin`], offering the plugin what `options` holds.
pub fn load_plugin_with(
    name_or_path: &str,
    options: &LoadOptions,
) -> Result<Option<Box<dyn PluginInstance>>, PluginError> {
    let format = PluginFormat::from_path_or_name(name_or_path);
    match format {
        PluginFormat::Synthetic => Ok(None),
        PluginFormat::Clap => load_clap(name_or_path, options),
        PluginFormat::Vst3 => load_vst3(name_or_path),
        PluginFormat::Lv2 => Err(PluginError::UnsupportedFormat(format)),
    }
}

#[cfg(feature = "clap-host")]
fn load_clap(
    path: &str,
    options: &LoadOptions,
) -> Result<Option<Box<dyn PluginInstance>>, PluginError> {
    use crate::audio_engine::plugin_host::ClapHost;
    let mut host = ClapHost::default();
    if let Some(actions) = &options.actions {
        host = host.with_actions(actions.clone());
    }
    if let Some(automation) = &options.automation {
        host = host.with_automation(automation.clone());
    }
    if options.timeline {
        host = host.with_timeline();
    }
    let plugin = host
        .load(Path::new(path), 0)
        .map_err(|e| PluginError::LoadFailed(format!("{e:?}")))?;
    // Wrap so the instance is `Send` for storage in the shared
    // `Mutex<HashMap<..>>`. Safety: all access goes through the
    // mutex, so concurrent thread access is impossible.
    Ok(Some(Box::new(plugin.into_send())))
}

#[cfg(not(feature = "clap-host"))]
fn load_clap(
    _path: &str,
    _options: &LoadOptions,
) -> Result<Option<Box<dyn PluginInstance>>, PluginError> {
    Err(PluginError::UnsupportedFormat(PluginFormat::Clap))
}

#[cfg(feature = "vst3-host")]
fn load_vst3(path: &str) -> Result<Option<Box<dyn PluginInstance>>, PluginError> {
    use crate::audio_engine::vst3_host::Vst3Host;
    let plugin = Vst3Host::new()
        .load(Path::new(path), 0)
        .map_err(|e| PluginError::LoadFailed(format!("{e:?}")))?;
    Ok(Some(Box::new(plugin.into_send())))
}

#[cfg(not(feature = "vst3-host"))]
fn load_vst3(_path: &str) -> Result<Option<Box<dyn PluginInstance>>, PluginError> {
    Err(PluginError::UnsupportedFormat(PluginFormat::Vst3))
}

// Convenience so `Path` import on the cfg(feature) branch is used.
#[allow(dead_code)]
fn _path_import(_: &Path) {}

// ── Render context ──────────────────────────────────────────────────────

std::thread_local! {
    static RENDER_FRAME: std::cell::Cell<Option<(u64, u64)>> = const { std::cell::Cell::new(None) };
}

/// Every block any renderer renders gets the next number, so "this block"
/// is unambiguous even when a loop or a seek renders the same frames again.
static RENDER_CYCLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Where on the timeline the block being processed starts, in output
/// samples — `Some` only while the project renderer is calling plugins.
///
/// `PluginEvents` carries no transport, and a plugin normally needs none.
/// This is for the few that must agree with each other about *when*: the
/// guide instruments on the Count and Guide tracks, where a spoken cue
/// takes the place of the count note under it.
#[must_use]
pub fn render_frame() -> Option<u64> {
    RENDER_FRAME
        .with(std::cell::Cell::get)
        .map(|(_, frame)| frame)
}

/// Which render pass this is — a number no other block shares. Plugins
/// that pass information between tracks within a block key it by this,
/// not by [`render_frame`]: the same frames are rendered again on every
/// loop pass and after a seek back.
#[must_use]
pub fn render_cycle() -> Option<u64> {
    RENDER_FRAME
        .with(std::cell::Cell::get)
        .map(|(cycle, _)| cycle)
}

/// The song's clock at the start of the block a plugin is processing —
/// what a hosted plugin's transport says (a CLAP's
/// `clap_event_transport`): the song time of the audio it is handed, so a
/// plugin on a track delay compensation holds back (its input late behind
/// its feeders' latency, or behind the FX before it) is told where that
/// audio is in the song, as every other plugin is of its own. `Some` only
/// while the project renderer is calling plugins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderTransport {
    /// The block's start on the timeline, seconds.
    pub seconds: f64,
    /// The same, in quarter-note beats along the tempo map.
    pub beats: f64,
    /// The tempo there, bpm.
    pub bpm: f64,
    /// Where the bar the block starts in begins, beats.
    pub bar_start_beats: f64,
    /// That bar's number, from 0.
    pub bar: i32,
    pub numerator: u16,
    pub denominator: u16,
    /// The timeline moves under the block (it is rendered at a frozen
    /// position otherwise: input monitored while stopped).
    pub playing: bool,
}

std::thread_local! {
    static RENDER_TRANSPORT: std::cell::Cell<Option<RenderTransport>> =
        const { std::cell::Cell::new(None) };
}

std::thread_local! {
    /// This thread is rendering a block the transport does not roll
    /// through (live input played through while stopped).
    static STOPPED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the block being rendered on this thread is a stopped one
/// ([`StoppedRender`]): its plugins are told the transport isn't playing.
#[must_use]
pub fn rendering_stopped() -> bool {
    STOPPED.with(std::cell::Cell::get)
}

/// Marks the renders on this thread, for its life, as of a stopped
/// transport: a plugin's `IS_PLAYING` is clear there, so a plugin that
/// plays a timeline (host-timeline/1) knows to stay live.
pub(crate) struct StoppedRender(bool);

impl StoppedRender {
    pub(crate) fn enter() -> Self {
        Self(STOPPED.with(|c| c.replace(true)))
    }
}

impl Drop for StoppedRender {
    fn drop(&mut self) {
        STOPPED.with(|c| c.set(self.0));
    }
}

/// Tell the plugins called next on this thread `transport`: the render
/// loop's, before each plugin, its clock held back by how late its audio
/// is.
pub(crate) fn set_render_transport(transport: RenderTransport) {
    RENDER_TRANSPORT.with(|c| c.set(Some(transport)));
}

/// The song's clock for the block being processed ([`RenderTransport`]).
#[must_use]
pub fn render_transport() -> Option<RenderTransport> {
    RENDER_TRANSPORT.with(std::cell::Cell::get)
}

/// A render's context on its thread, for a worker of the same block to
/// [`adopt`](RenderFrameGuard::adopt): the block's frame and its clock.
pub(crate) type RenderContext = (Option<(u64, u64)>, Option<RenderTransport>);

/// Publishes [`render_frame`] (and, once set, [`render_transport`]) for the
/// life of the guard.
pub(crate) struct RenderFrameGuard;

impl RenderFrameGuard {
    pub(crate) fn enter(start_frame: u64) -> Self {
        let cycle = RENDER_CYCLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        RENDER_FRAME.with(|c| c.set(Some((cycle, start_frame))));
        Self
    }
}

impl RenderFrameGuard {
    /// This thread's render context, for a worker to [`adopt`](Self::adopt).
    pub(crate) fn current() -> RenderContext {
        (
            RENDER_FRAME.with(std::cell::Cell::get),
            RENDER_TRANSPORT.with(std::cell::Cell::get),
        )
    }

    /// Publish the render thread's context on a worker rendering part of
    /// the same block — the same cycle, not a new one.
    pub(crate) fn adopt(context: RenderContext) -> Self {
        RENDER_FRAME.with(|c| c.set(context.0));
        RENDER_TRANSPORT.with(|c| c.set(context.1));
        Self
    }

    /// Publish the block's clock ([`render_transport`]) until the guard
    /// drops.
    pub(crate) fn set_transport(&self, transport: RenderTransport) {
        RENDER_TRANSPORT.with(|c| c.set(Some(transport)));
    }
}

impl Drop for RenderFrameGuard {
    fn drop(&mut self) {
        RENDER_FRAME.with(|c| c.set(None));
        RENDER_TRANSPORT.with(|c| c.set(None));
    }
}

std::thread_local! {
    static TRACK_MUTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the track whose FX chain is being processed is muted. Mute is
/// applied at the fader, after the chain, so a muted track's plugins still
/// run — this is how one knows its output will not be heard.
#[must_use]
pub fn track_muted() -> bool {
    TRACK_MUTED.with(std::cell::Cell::get)
}

/// Publishes [`track_muted`] while a track's chain is processed.
pub(crate) struct TrackMutedGuard;

impl TrackMutedGuard {
    pub(crate) fn enter(muted: bool) -> Self {
        TRACK_MUTED.with(|c| c.set(muted));
        Self
    }
}

impl Drop for TrackMutedGuard {
    fn drop(&mut self) {
        TRACK_MUTED.with(|c| c.set(false));
    }
}

/// What a plugin tells its host between calls, read from any thread
/// without its lock: a CLAP's `request_callback`, its parameter rescans,
/// and whether what its state holds may have changed (its
/// `clap_host_state.mark_dirty`, a parameter value or gesture it sent
/// from its own side).
#[derive(Debug, Default)]
pub struct PluginNotices {
    /// Bumped on every rescan of parameter info or text (a renamed
    /// parameter): an embedder showing names re-reads them when it moves.
    pub(crate) param_info: std::sync::atomic::AtomicU64,
    /// Bumped on every rescan of parameter values.
    pub(crate) param_values: std::sync::atomic::AtomicU64,
    /// The plugin asked for its main-thread callback (`request_callback`).
    pub(crate) callback: std::sync::atomic::AtomicBool,
    /// Bumped whenever the plugin says its state changed: what a saved
    /// copy of its state is checked against.
    pub(crate) state: std::sync::atomic::AtomicU64,
    /// Bumped on every `request_process`: an asleep plugin asking to be
    /// called again ([`crate::plugin_sleep`]).
    pub(crate) wake: std::sync::atomic::AtomicU64,
}

impl PluginNotices {
    /// See the field: moves on every rescan of parameter info or text.
    #[must_use]
    pub fn param_info(&self) -> u64 {
        self.param_info.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Whether the plugin asked for its main-thread callback since it last
    /// ran.
    #[must_use]
    pub fn wants_main_thread(&self) -> bool {
        self.callback.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Moves whenever the plugin says its state changed.
    #[must_use]
    pub fn state(&self) -> u64 {
        self.state.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Moves on every `request_process` the plugin made.
    #[must_use]
    pub fn wake(&self) -> u64 {
        self.wake.load(std::sync::atomic::Ordering::Acquire)
    }

    /// The plugin's state changed (it said so, or sent a value of its own).
    pub fn mark_state_dirty(&self) {
        self.state.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }
}

/// Where each [`PluginCell`]'s [`PluginCell::instance`] comes from.
static NEXT_INSTANCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// One loaded plugin, behind a lock of its own: a render takes it for the
/// plugin's `process_block`, a control thread for a parameter or its state.
/// Two renders running disjoint tracks — the ahead render and the realtime
/// one — never meet on a lock, where one lock over every plugin made them
/// take turns.
///
/// Beside the lock, what can be read without it: the instance's own id,
/// its [`PluginNotices`], how often a control thread reached it, and
/// whether its editor is open — what says whether its state may have
/// changed ([`PluginCell::state_key`]).
pub struct PluginCell {
    plugin: std::sync::Mutex<Box<dyn PluginInstance>>,
    /// This instance, apart from every other loaded in the process.
    instance: u64,
    notices: Option<std::sync::Arc<PluginNotices>>,
    /// Its `signal/1`, kept from the instance as it was loaded: read
    /// without its lock, valid while the cell lives.
    #[cfg(feature = "clap-host")]
    signal: Option<daw_clap_ext::host::SignalPluginClient>,
    /// Bumped every time a control thread takes the plugin to change it
    /// ([`PluginCell::lock`]; a render's [`PluginCell::try_lock_spinning`]
    /// and a read's [`PluginCell::lock_reading`] do not).
    touched: std::sync::atomic::AtomicU64,
    /// Its editor is open: edits there may reach its state unseen.
    gui_open: std::sync::atomic::AtomicBool,
    /// Whether the renderer calls it ([`crate::plugin_sleep`]).
    sleep: crate::plugin_sleep::SleepState,
}

/// What says whether a plugin's state may have changed: two keys equal
/// mean the same instance, untouched by any control thread between them,
/// with nothing said by the plugin, its editor closed throughout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PluginStateKey {
    pub instance: u64,
    pub touched: u64,
    /// The plugin's [`PluginNotices::state`]; `None` from a backend that
    /// tells nothing.
    pub reported: Option<u64>,
    pub gui_open: bool,
}

impl PluginStateKey {
    /// Whether a state read under `self` still holds at `now`.
    #[must_use]
    pub fn still(&self, now: &Self) -> bool {
        self == now && !now.gui_open
    }
}

impl PluginCell {
    /// Locked once here, on the control thread that builds it: on macOS
    /// `std::sync::Mutex` allocates its pthread mutex on the first lock, and
    /// that first lock would otherwise be the render's `try_lock_spinning`
    /// on a freshly rebuilt plugin — an allocation on the audio thread
    /// (sampler's `rt_alloc` caught it; Linux's futex mutex never allocates).
    #[must_use]
    pub fn new(plugin: Box<dyn PluginInstance>) -> Self {
        let notices = plugin.notices();
        #[cfg(feature = "clap-host")]
        let signal = plugin.signal();
        let cell = Self {
            plugin: std::sync::Mutex::new(plugin),
            instance: NEXT_INSTANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            notices,
            #[cfg(feature = "clap-host")]
            signal,
            touched: std::sync::atomic::AtomicU64::new(0),
            gui_open: std::sync::atomic::AtomicBool::new(false),
            sleep: crate::plugin_sleep::SleepState::default(),
        };
        drop(cell.lock_reading());
        cell
    }

    /// The plugin back out of its cell.
    #[must_use]
    pub fn into_inner(self) -> Box<dyn PluginInstance> {
        self.plugin
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The plugin, through a cell nobody else can reach (the map held for
    /// writing): no lock taken.
    pub fn get_mut(&mut self) -> &mut Box<dyn PluginInstance> {
        *self.touched.get_mut() += 1;
        self.plugin
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The plugin, waiting for whoever has it, to change it (a parameter,
    /// a state, an action): counted as a touch, so a saved copy of its
    /// state is read again. Control threads only — a render never waits
    /// ([`PluginCell::try_lock_spinning`]).
    pub fn lock(&self) -> std::sync::MutexGuard<'_, Box<dyn PluginInstance>> {
        self.touched
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.lock_reading()
    }

    /// The plugin, waiting for whoever has it, only to read it (its state
    /// saved, its parameters' values and texts): no touch counted.
    pub fn lock_reading(&self) -> std::sync::MutexGuard<'_, Box<dyn PluginInstance>> {
        self.plugin
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The plugin's own id, apart from every instance loaded before or
    /// after it under the same guid.
    #[must_use]
    pub const fn instance(&self) -> u64 {
        self.instance
    }

    /// What the plugin told its host, read without its lock.
    #[must_use]
    pub fn notices(&self) -> Option<&std::sync::Arc<PluginNotices>> {
        self.notices.as_ref()
    }

    /// The plugin's `signal/1`, read without its lock: its thread-safe
    /// calls (revisions, meter values) may be made while the cell lives,
    /// that is while the map is held and its generation has not moved.
    #[cfg(feature = "clap-host")]
    #[must_use]
    pub const fn signal(&self) -> Option<daw_clap_ext::host::SignalPluginClient> {
        self.signal
    }

    /// Its sleep: whether the renderer calls it this block.
    #[must_use]
    pub const fn sleep(&self) -> &crate::plugin_sleep::SleepState {
        &self.sleep
    }

    /// What wakes it besides its audio and notes: a control thread's touch,
    /// its editor open, its `request_process` ([`crate::plugin_sleep`]'s
    /// key). Read without its lock.
    #[must_use]
    pub fn wake_key(&self) -> u64 {
        let gui = self.gui_open.load(std::sync::atomic::Ordering::Acquire);
        crate::plugin_sleep::key_of(
            self.instance,
            [
                self.touched.load(std::sync::atomic::Ordering::Acquire),
                self.notices.as_ref().map_or(0, |n| n.wake()),
                // An open editor wakes it every block: what it shows moves.
                if gui { self.sleep.skipped() + 1 } else { 0 },
            ],
        )
    }

    /// Its editor opened or closed.
    pub fn set_gui_open(&self, open: bool) {
        self.gui_open
            .store(open, std::sync::atomic::Ordering::Release);
        self.touched
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }

    /// Whether its state may have changed since another key: see
    /// [`PluginStateKey`]. Read without the plugin's lock.
    #[must_use]
    pub fn state_key(&self) -> PluginStateKey {
        PluginStateKey {
            instance: self.instance,
            touched: self.touched.load(std::sync::atomic::Ordering::Acquire),
            reported: self.notices.as_ref().map(|n| n.state()),
            gui_open: self.gui_open.load(std::sync::atomic::Ordering::Acquire),
        }
    }

    /// The plugin, trying `spins` times (a control thread's hold is a
    /// parameter write, microseconds) — `None` while it is still held: the
    /// render bypasses it for the block rather than wait.
    pub fn try_lock_spinning(
        &self,
        spins: usize,
    ) -> Option<std::sync::MutexGuard<'_, Box<dyn PluginInstance>>> {
        for _ in 0..spins.max(1) {
            match self.plugin.try_lock() {
                Ok(guard) => return Some(guard),
                Err(std::sync::TryLockError::Poisoned(p)) => return Some(p.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) => std::hint::spin_loop(),
            }
        }
        None
    }
}

/// The loaded plugins by FX guid. Its methods trade in plugins, so whoever
/// holds the map for writing moves them in and out as before plugins had
/// cells of their own.
#[derive(Default)]
pub struct PluginMap {
    plugins: std::collections::HashMap<String, PluginCell>,
    /// Bumped on every change that may put another instance under a guid
    /// (an insert, a removal, a plugin reached mutably): a holder of the
    /// map for reading who kept something from an instance (a CLAP
    /// extension's client) knows it still points at a live plugin while
    /// this has not moved.
    generation: u64,
}

impl PluginMap {
    /// Put `plugin` under `fx_guid`; the one it replaces, if any.
    pub fn insert(
        &mut self,
        fx_guid: String,
        plugin: Box<dyn PluginInstance>,
    ) -> Option<Box<dyn PluginInstance>> {
        self.generation = self.generation.wrapping_add(1);
        self.plugins
            .insert(fx_guid, PluginCell::new(plugin))
            .map(PluginCell::into_inner)
    }

    pub fn remove(&mut self, fx_guid: &str) -> Option<Box<dyn PluginInstance>> {
        self.generation = self.generation.wrapping_add(1);
        self.plugins.remove(fx_guid).map(PluginCell::into_inner)
    }

    pub fn get_mut(&mut self, fx_guid: &str) -> Option<&mut Box<dyn PluginInstance>> {
        self.generation = self.generation.wrapping_add(1);
        self.plugins.get_mut(fx_guid).map(PluginCell::get_mut)
    }

    /// The cell under `fx_guid`, for a holder of the map for reading.
    #[must_use]
    pub fn cell(&self, fx_guid: &str) -> Option<&PluginCell> {
        self.plugins.get(fx_guid)
    }

    #[must_use]
    pub fn contains_key(&self, fx_guid: &str) -> bool {
        self.plugins.contains_key(fx_guid)
    }

    /// See [`PluginMap::generation`]'s field: moves whenever an instance
    /// under a guid may have been replaced or dropped.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Every FX guid with a loaded plugin.
    pub fn guids(&self) -> impl Iterator<Item = &str> {
        self.plugins.keys().map(String::as_str)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

/// Every loaded plugin. Renders hold the map for reading over a block —
/// two at once, the ahead render and the realtime one — and each plugin
/// by its own cell; a structural change (an insert, a removal, a chain
/// swapped whole) holds it for writing, between blocks.
#[derive(Default)]
pub struct Plugins(std::sync::RwLock<PluginMap>);

/// How long a writer rests between tries for the map. It never queues for
/// it: a queued writer turns the next reader away, and a reader is an audio
/// callback.
const WRITER_REST: std::time::Duration = std::time::Duration::from_micros(50);

impl Plugins {
    /// The map for reading, waiting while a writer has it. Control threads.
    pub fn read(&self) -> std::sync::RwLockReadGuard<'_, PluginMap> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The map for reading, trying `spins` times — a render's way in:
    /// `None` while a writer still has it.
    pub fn try_read_spinning(
        &self,
        spins: usize,
    ) -> Option<std::sync::RwLockReadGuard<'_, PluginMap>> {
        for _ in 0..spins.max(1) {
            match self.0.try_read() {
                Ok(guard) => return Some(guard),
                Err(std::sync::TryLockError::Poisoned(p)) => return Some(p.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) => std::hint::spin_loop(),
            }
        }
        None
    }

    /// The map for writing. Tries rather than queues (see [`WRITER_REST`]);
    /// a render holds it a block, so this waits about a block at most.
    pub fn write(&self) -> std::sync::RwLockWriteGuard<'_, PluginMap> {
        loop {
            match self.0.try_write() {
                Ok(guard) => return guard,
                Err(std::sync::TryLockError::Poisoned(p)) => return p.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(WRITER_REST),
            }
        }
    }

    /// Whether a holder panicked (every way in recovers from it).
    #[must_use]
    pub fn is_poisoned(&self) -> bool {
        self.0.is_poisoned()
    }

    /// Run `f` on the plugin under `fx_guid`, if there is one: its own lock
    /// taken, the map only read — a render of other tracks goes on.
    pub fn with<R>(
        &self,
        fx_guid: &str,
        f: impl FnOnce(&mut dyn PluginInstance) -> R,
    ) -> Option<R> {
        let map = self.read();
        let mut plugin = map.cell(fx_guid)?.lock();
        Some(f(plugin.as_mut()))
    }

    /// [`Plugins::with`], only to read the plugin (its state saved, a
    /// value or text asked): no touch counted ([`PluginCell::lock_reading`]).
    pub fn with_reading<R>(
        &self,
        fx_guid: &str,
        f: impl FnOnce(&mut dyn PluginInstance) -> R,
    ) -> Option<R> {
        let map = self.read();
        let mut plugin = map.cell(fx_guid)?.lock_reading();
        Some(f(plugin.as_mut()))
    }

    /// [`Plugins::with_reading`], never waiting: `Err(Busy)` while a render
    /// (or anyone) has the plugin, `Ok(None)` when there is none.
    ///
    /// # Errors
    /// [`PluginBusy`] while the plugin is held.
    pub fn try_with_reading<R>(
        &self,
        fx_guid: &str,
        f: impl FnOnce(&mut dyn PluginInstance) -> R,
    ) -> Result<Option<R>, PluginBusy> {
        let map = self.read();
        let Some(cell) = map.cell(fx_guid) else {
            return Ok(None);
        };
        let mut plugin = cell.try_lock_spinning(1).ok_or(PluginBusy)?;
        Ok(Some(f(plugin.as_mut())))
    }
}

/// A plugin held by someone else (a render running it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PluginBusy;

#[cfg(test)]
mod plugins_tests {
    use super::*;

    /// Counts its blocks.
    struct Probe(u32);

    impl PluginInstance for Probe {
        fn descriptor(&self) -> PluginDescriptor {
            PluginDescriptor {
                id: "probe".into(),
                name: "Probe".into(),
                vendor: String::new(),
                version: String::new(),
                format: PluginFormat::Synthetic,
            }
        }
        fn params(&mut self) -> Vec<PluginParamInfo> {
            Vec::new()
        }
        fn param_value(&mut self, _: u32) -> Option<f64> {
            Some(f64::from(self.0))
        }
        fn value_to_text(&mut self, _: u32, _: f64) -> Option<String> {
            None
        }
        fn text_to_value(&mut self, _: u32, _: &str) -> Option<f64> {
            None
        }
        fn latency(&mut self) -> u32 {
            0
        }
        fn prepare(&mut self, _: f64, _: u32) -> Result<(), PluginError> {
            Ok(())
        }
        fn is_prepared(&self) -> bool {
            true
        }
        fn process_block(
            &mut self,
            _: &[f32],
            _: &[f32],
            _: &mut [f32],
            _: &mut [f32],
            _: &PluginEvents<'_>,
        ) -> Result<(), PluginError> {
            self.0 += 1;
            Ok(())
        }
        fn deactivate(&mut self) {}
    }

    /// Two readers hold the map at once and each runs its own plugin: the
    /// ahead render and the realtime one never take turns. A plugin in use
    /// is turned away, not waited for.
    #[test]
    fn two_renders_read_the_map_and_run_their_own_plugins() {
        let plugins = Plugins::default();
        plugins.write().insert("a".into(), Box::new(Probe(0)));
        plugins.write().insert("b".into(), Box::new(Probe(0)));
        let first = plugins.try_read_spinning(1).expect("a reader");
        let second = plugins
            .try_read_spinning(1)
            .expect("a second reader beside it");
        let a = first.cell("a").and_then(|c| c.try_lock_spinning(1));
        let b = second.cell("b").and_then(|c| c.try_lock_spinning(1));
        assert!(a.is_some() && b.is_some());
        assert!(
            second
                .cell("a")
                .and_then(|c| c.try_lock_spinning(1))
                .is_none()
        );
    }

    /// A plugin's state key moves when a control thread reaches it to
    /// change it, when it says its state changed, or when its editor opens
    /// — never for a read, a render, or a try turned away.
    #[test]
    fn the_state_key_moves_on_a_change_and_holds_for_a_read() {
        let plugins = Plugins::default();
        plugins.write().insert("a".into(), Box::new(Probe(0)));
        let key = |p: &Plugins| p.read().cell("a").unwrap().state_key();
        let start = key(&plugins);
        assert_eq!(start.reported, None, "a probe tells nothing");
        plugins.with_reading("a", |p| p.param_value(0));
        assert!(start.still(&key(&plugins)), "a read touches nothing");
        {
            let map = plugins.read();
            let _render = map.cell("a").unwrap().try_lock_spinning(1).unwrap();
            assert_eq!(
                plugins.try_with_reading("a", |p| p.param_value(0)),
                Err(PluginBusy),
                "a busy plugin is not waited for"
            );
        }
        assert!(start.still(&key(&plugins)), "a render touches nothing");
        plugins.with("a", |p| p.set_param_value(0, 1.0));
        let touched = key(&plugins);
        assert!(!start.still(&touched), "a change is a touch");
        plugins.read().cell("a").unwrap().set_gui_open(true);
        let open = key(&plugins);
        assert!(!open.still(&open), "an open editor never holds still");
        plugins.read().cell("a").unwrap().set_gui_open(false);
        assert!(!touched.still(&key(&plugins)));
    }

    /// A plugin that tells (its notices) moves its key by saying so.
    #[test]
    fn a_plugin_marking_its_state_dirty_moves_its_key() {
        struct Telling(std::sync::Arc<PluginNotices>, Probe);
        impl PluginInstance for Telling {
            fn descriptor(&self) -> PluginDescriptor {
                self.1.descriptor()
            }
            fn params(&mut self) -> Vec<PluginParamInfo> {
                Vec::new()
            }
            fn param_value(&mut self, _: u32) -> Option<f64> {
                None
            }
            fn value_to_text(&mut self, _: u32, _: f64) -> Option<String> {
                None
            }
            fn text_to_value(&mut self, _: u32, _: &str) -> Option<f64> {
                None
            }
            fn latency(&mut self) -> u32 {
                0
            }
            fn prepare(&mut self, _: f64, _: u32) -> Result<(), PluginError> {
                Ok(())
            }
            fn is_prepared(&self) -> bool {
                true
            }
            fn process_block(
                &mut self,
                _: &[f32],
                _: &[f32],
                _: &mut [f32],
                _: &mut [f32],
                _: &PluginEvents<'_>,
            ) -> Result<(), PluginError> {
                Ok(())
            }
            fn deactivate(&mut self) {}
            fn notices(&self) -> Option<std::sync::Arc<PluginNotices>> {
                Some(self.0.clone())
            }
        }
        let notices = std::sync::Arc::<PluginNotices>::default();
        let plugins = Plugins::default();
        plugins
            .write()
            .insert("t".into(), Box::new(Telling(notices.clone(), Probe(0))));
        let key = || plugins.read().cell("t").unwrap().state_key();
        let start = key();
        assert_eq!(start.reported, Some(0));
        notices.mark_state_dirty();
        assert!(!start.still(&key()), "marked dirty");
        // Another instance under the same guid is another key.
        let now = key();
        plugins
            .write()
            .insert("t".into(), Box::new(Telling(notices.clone(), Probe(0))));
        assert_ne!(now.instance, key().instance);
    }

    /// The generation moves on whatever may put another plugin under a
    /// guid, and stays put while the map is only read and run.
    #[test]
    fn the_generation_moves_when_an_instance_may_change() {
        let plugins = Plugins::default();
        let at = |p: &Plugins| p.read().generation();
        let start = at(&plugins);
        plugins.write().insert("a".into(), Box::new(Probe(0)));
        let inserted = at(&plugins);
        assert_ne!(inserted, start);
        plugins.with("a", |p| {
            let _ = p.process_block(&[], &[], &mut [], &mut [], &PluginEvents::EMPTY);
        });
        assert_eq!(at(&plugins), inserted, "running a plugin changes nothing");
        assert_eq!(plugins.read().guids().collect::<Vec<_>>(), ["a"]);
        let _ = plugins.write().get_mut("a");
        let reached = at(&plugins);
        assert_ne!(reached, inserted, "a plugin reached mutably may be swapped");
        let _ = plugins.write().remove("a");
        assert_ne!(at(&plugins), reached);
    }

    /// A plugin run through its cell and taken back out is the same one.
    #[test]
    fn a_plugin_comes_back_out_as_it_went_in() {
        let plugins = Plugins::default();
        plugins.write().insert("a".into(), Box::new(Probe(41)));
        plugins.with("a", |p| {
            let _ = p.process_block(&[], &[], &mut [], &mut [], &PluginEvents::EMPTY);
        });
        let mut back = plugins.write().remove("a").expect("the plugin");
        assert_eq!(back.param_value(0), Some(42.0));
        assert!(!plugins.read().contains_key("a"));
    }
}

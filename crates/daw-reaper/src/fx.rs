//! REAPER FX Service Implementation
//!
//! Implements FxService for REAPER by dispatching operations to the main thread
//! using TaskSupport from reaper-high.
//!
//! ## Reference Implementations
//!
//! This implementation is informed by two REAPER scripts:
//!
//! - **Snapshooter** (tilr): Parameter-level snapshots using FX GUIDs + param indices.
//!   Diff-based recall, morphing via linear interpolation.
//!   See: FTS-TRACKS/Scripts/ReaTeam Scripts/Envelopes/tilr_Snapshooter
//!
//! - **Track Snapshot** (Daniel Lumertz): Full binary state chunk capture/restore via
//!   GetTrackStateChunk/SetTrackStateChunk. Selective section swapping (FXCHAIN, envelopes, etc.).
//!   See: FTS-GUITAR/Scripts/Daniel Lumertz Scripts/Tracks/Track Snapshot

use daw_proto::{
    AddFxAtRequest, Fx, FxChainContext, FxEvent, FxLatency, FxParamModulation, FxParameter, FxRef,
    FxService, FxStateChunk, FxTarget, FxType, ProjectContext, SetNamedConfigRequest,
    SetParameterByNameRequest, SetParameterRequest,
};
use reaper_high::{FxChain, Reaper, Track};
use reaper_medium::{FxPresetRef, TrackFxLocation, TransferBehavior};
use roam::{Context, Tx};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::project_context::find_project_by_guid;
use crate::transport::task_support;

// =============================================================================
// FX Event Broadcasting Infrastructure
//
// Follows the same reactive polling pattern as transport.rs:
// poll_and_broadcast_fx() is called from the timer callback on the main thread
// at ~30Hz. It reads current FX chain state, diffs against a cache, and only
// broadcasts FxEvent when something actually changed.
// =============================================================================

/// Cached state for a single FX instance (for change detection)
#[derive(Clone, Debug)]
struct CachedFxState {
    guid: String,
    name: String,
    index: u32,
    enabled: bool,
    /// Cached normalized parameter values for monitored params
    param_values: Vec<f64>,
}

/// Cached state for an entire FX chain (for change detection)
#[derive(Clone, Debug)]
struct CachedChainState {
    /// Ordered list of FX states (by chain index)
    fx_states: Vec<CachedFxState>,
}

/// Key for identifying an FX chain (project + chain context)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ChainKey {
    project_guid: String,
    context: String, // serialized FxChainContext
}

impl ChainKey {
    fn new(project_guid: &str, context: &FxChainContext) -> Self {
        let ctx_str = match context {
            FxChainContext::Track(guid) => format!("track:{}", guid),
            FxChainContext::Input(guid) => format!("input:{}", guid),
            FxChainContext::Monitoring => "monitoring".to_string(),
        };
        Self {
            project_guid: project_guid.to_string(),
            context: ctx_str,
        }
    }
}

/// Global FX event broadcaster
static FX_BROADCASTER: OnceLock<broadcast::Sender<FxEvent>> = OnceLock::new();

/// Cached FX chain states for change detection
static FX_CHAIN_CACHE: OnceLock<Mutex<HashMap<ChainKey, CachedChainState>>> = OnceLock::new();

/// Set of FX chains being monitored (only poll chains with active subscribers)
static FX_MONITORED_CHAINS: OnceLock<Mutex<Vec<(ProjectContext, FxChainContext)>>> =
    OnceLock::new();

/// Initialize the FX event broadcaster.
/// Called during extension initialization alongside init_transport_broadcaster().
pub fn init_fx_broadcaster() {
    let (tx, _rx) = broadcast::channel::<FxEvent>(64);
    let _ = FX_BROADCASTER.set(tx);
    let _ = FX_CHAIN_CACHE.set(Mutex::new(HashMap::new()));
    let _ = FX_MONITORED_CHAINS.set(Mutex::new(Vec::new()));
    info!("FX event broadcaster initialized");
}

/// Get a receiver for FX events.
fn fx_event_receiver() -> Option<broadcast::Receiver<FxEvent>> {
    FX_BROADCASTER.get().map(|tx| tx.subscribe())
}

/// Register an FX chain for monitoring.
fn register_monitored_chain(project: ProjectContext, context: FxChainContext) {
    if let Some(chains) = FX_MONITORED_CHAINS.get() {
        let mut chains = chains.lock().unwrap();
        // Check if already monitoring this chain (compare serialized forms)
        let key = format!("{:?}|{:?}", project, context);
        let already = chains
            .iter()
            .any(|(p, c)| format!("{:?}|{:?}", p, c) == key);
        if !already {
            chains.push((project, context));
        }
    }
}

/// Hash a string for project GUID (same as transport.rs)
fn hash_string(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Get project GUID from a REAPER project (same algorithm as transport.rs)
fn project_guid_string(project: &reaper_high::Project) -> String {
    let path = project.file().map(|p| p.to_string()).unwrap_or_default();
    format!("{:x}", hash_string(&path))
}

/// Parameter value change threshold (avoid flooding with micro-changes from automation)
const PARAM_CHANGE_THRESHOLD: f64 = 0.0001;

/// Maximum number of parameters to monitor per FX (performance budget)
const MAX_MONITORED_PARAMS: u32 = 128;

/// Poll all monitored FX chains and broadcast events for changes.
/// **MUST be called from the main thread** (e.g., from timer callback).
pub fn poll_and_broadcast_fx() {
    let Some(tx) = FX_BROADCASTER.get() else {
        return;
    };

    // Skip if no subscribers
    if tx.receiver_count() == 0 {
        return;
    }

    let Some(monitored) = FX_MONITORED_CHAINS.get() else {
        return;
    };
    let Some(cache) = FX_CHAIN_CACHE.get() else {
        return;
    };

    let chains: Vec<(ProjectContext, FxChainContext)> = {
        let guard = monitored.lock().unwrap();
        guard.clone()
    };

    if chains.is_empty() {
        return;
    }

    let mut cache_guard = cache.lock().unwrap();

    for (project_ctx, chain_ctx) in &chains {
        let Some(project) = resolve_project(project_ctx) else {
            continue;
        };
        let Some((_track, chain)) = resolve_fx_chain(&project, chain_ctx) else {
            continue;
        };

        let proj_guid = project_guid_string(&project);
        let key = ChainKey::new(&proj_guid, chain_ctx);

        // Read current FX chain state
        let current = read_chain_state(&chain);

        // Compare with cached state and emit events
        if let Some(prev) = cache_guard.get(&key) {
            diff_and_broadcast(tx, chain_ctx, prev, &current);
        }

        // Update cache
        cache_guard.insert(key, current);
    }
}

/// Read the current state of an FX chain for caching
fn read_chain_state(chain: &FxChain) -> CachedChainState {
    let mut fx_states = Vec::new();

    for fx in chain.fxs() {
        let guid = fx
            .get_or_query_guid()
            .map(|g| g.to_string_without_braces())
            .unwrap_or_default();
        let name = fx.name().to_str().to_string();
        let index = fx.index();
        let enabled = fx.is_enabled();

        // Read parameter values (up to MAX_MONITORED_PARAMS)
        let param_count = fx.parameter_count().min(MAX_MONITORED_PARAMS);
        let mut param_values = Vec::with_capacity(param_count as usize);
        for i in 0..param_count {
            let param = fx.parameter_by_index(i);
            param_values.push(param.reaper_normalized_value().get());
        }

        fx_states.push(CachedFxState {
            guid,
            name,
            index,
            enabled,
            param_values,
        });
    }

    CachedChainState { fx_states }
}

/// Diff two chain states and broadcast FxEvents for any changes
fn diff_and_broadcast(
    tx: &broadcast::Sender<FxEvent>,
    context: &FxChainContext,
    prev: &CachedChainState,
    curr: &CachedChainState,
) {
    let prev_guids: HashMap<&str, &CachedFxState> = prev
        .fx_states
        .iter()
        .map(|s| (s.guid.as_str(), s))
        .collect();
    let curr_guids: HashMap<&str, &CachedFxState> = curr
        .fx_states
        .iter()
        .map(|s| (s.guid.as_str(), s))
        .collect();

    // Detect removed FX
    for (guid, _prev_fx) in &prev_guids {
        if !curr_guids.contains_key(guid) {
            let _ = tx.send(FxEvent::Removed {
                context: context.clone(),
                fx_guid: guid.to_string(),
            });
        }
    }

    // Detect added FX
    for curr_fx in &curr.fx_states {
        if !prev_guids.contains_key(curr_fx.guid.as_str()) {
            let _ = tx.send(FxEvent::Added {
                context: context.clone(),
                fx: Fx {
                    guid: curr_fx.guid.clone(),
                    index: curr_fx.index,
                    name: curr_fx.name.clone(),
                    plugin_name: curr_fx.name.clone(),
                    plugin_type: FxType::Unknown,
                    enabled: curr_fx.enabled,
                    offline: false,
                    window_open: false,
                    parameter_count: curr_fx.param_values.len() as u32,
                    preset_name: None,
                },
            });
        }
    }

    // Detect changes in existing FX
    for curr_fx in &curr.fx_states {
        if let Some(prev_fx) = prev_guids.get(curr_fx.guid.as_str()) {
            // Enabled/bypass state change
            if prev_fx.enabled != curr_fx.enabled {
                let _ = tx.send(FxEvent::EnabledChanged {
                    context: context.clone(),
                    fx_guid: curr_fx.guid.clone(),
                    enabled: curr_fx.enabled,
                });
            }

            // Reorder detection (index changed)
            if prev_fx.index != curr_fx.index {
                let _ = tx.send(FxEvent::Moved {
                    context: context.clone(),
                    fx_guid: curr_fx.guid.clone(),
                    old_index: prev_fx.index,
                    new_index: curr_fx.index,
                });
            }

            // Parameter value changes
            let min_len = prev_fx.param_values.len().min(curr_fx.param_values.len());
            for i in 0..min_len {
                let delta = (prev_fx.param_values[i] - curr_fx.param_values[i]).abs();
                if delta > PARAM_CHANGE_THRESHOLD {
                    let _ = tx.send(FxEvent::ParameterChanged {
                        context: context.clone(),
                        fx_guid: curr_fx.guid.clone(),
                        param_index: i as u32,
                        value: curr_fx.param_values[i],
                    });
                }
            }
        }
    }
}

/// REAPER FX service implementation that dispatches to the main thread via TaskSupport.
///
/// Follows the same pattern as ReaperTransport and ReaperRouting:
/// zero-field struct, all state lives in REAPER itself, queries via `main_thread_future()`,
/// commands via `do_later_in_main_thread_asap()`.
#[derive(Clone)]
pub struct ReaperFx;

impl ReaperFx {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperFx {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Resolve a ProjectContext to a REAPER Project
fn resolve_project(ctx: &ProjectContext) -> Option<reaper_high::Project> {
    match ctx {
        ProjectContext::Current => Some(Reaper::get().current_project()),
        ProjectContext::Project(guid) => find_project_by_guid(guid),
    }
}

/// Resolve a TrackRef-like string (GUID) to a REAPER Track within a project
fn resolve_track_by_guid(project: &reaper_high::Project, guid: &str) -> Option<Track> {
    for i in 0..project.track_count() {
        if let Some(track) = project.track_by_index(i) {
            if track.guid().to_string_without_braces() == guid {
                return Some(track);
            }
        }
    }
    None
}

/// Get the FxChain for a given FxChainContext
fn resolve_fx_chain(
    project: &reaper_high::Project,
    context: &FxChainContext,
) -> Option<(Track, FxChain)> {
    match context {
        FxChainContext::Track(guid) => {
            let track = resolve_track_by_guid(project, guid)?;
            let chain = track.normal_fx_chain();
            Some((track, chain))
        }
        FxChainContext::Input(guid) => {
            let track = resolve_track_by_guid(project, guid)?;
            let chain = track.input_fx_chain();
            Some((track, chain))
        }
        FxChainContext::Monitoring => {
            let track = project.master_track().ok()?;
            let chain = track.input_fx_chain();
            Some((track, chain))
        }
    }
}

/// Resolve an FxRef to an index within the FX chain
fn resolve_fx_index(chain: &FxChain, fx_ref: &FxRef) -> Option<u32> {
    match fx_ref {
        FxRef::Index(idx) => {
            if *idx < chain.fx_count() {
                Some(*idx)
            } else {
                None
            }
        }
        FxRef::Guid(guid) => {
            // Search by GUID — this is how Snapshooter identifies FX for stability
            for fx in chain.fxs() {
                if let Some(fx_guid) = fx.guid() {
                    if fx_guid.to_string_without_braces() == *guid {
                        return Some(fx.index());
                    }
                }
            }
            None
        }
        FxRef::Name(name) => {
            // Search by name (first match)
            for fx in chain.index_based_fxs() {
                let fx_name = fx.name();
                if fx_name.to_str() == name {
                    return Some(fx.index());
                }
            }
            None
        }
    }
}

/// Get the TrackFxLocation for a given index and chain type
fn fx_location(index: u32, is_input: bool) -> TrackFxLocation {
    if is_input {
        TrackFxLocation::InputFxChain(index)
    } else {
        TrackFxLocation::NormalFxChain(index)
    }
}

/// Convert reaper-high FxInfo sub_type_expression to our FxType
fn parse_fx_type(sub_type: &str) -> FxType {
    match sub_type {
        "VST" | "VSTi" => FxType::Vst2,
        "VST3" | "VST3i" => FxType::Vst3,
        "AU" | "AUi" => FxType::Au,
        "JS" => FxType::Js,
        "CLAP" | "CLAPi" => FxType::Clap,
        _ => FxType::Unknown,
    }
}

/// Build an Fx proto struct from a reaper-high Fx
fn build_fx_info(fx: &reaper_high::Fx) -> Fx {
    let guid = fx
        .get_or_query_guid()
        .map(|g| g.to_string_without_braces())
        .unwrap_or_default();
    let name = fx.name().to_str().to_string();
    let index = fx.index();
    let enabled = fx.is_enabled();
    let offline = !fx.is_online();
    let window_open = fx.window_is_open();
    let parameter_count = fx.parameter_count();

    // Get plugin type and name via info() (REAPER >= 6.37)
    let (plugin_name, plugin_type, preset_name) = match fx.info() {
        Ok(info) => {
            let ptype = parse_fx_type(&info.sub_type_expression);
            (info.effect_name, ptype, None) // TODO: preset name from named config
        }
        Err(_) => (name.clone(), FxType::Unknown, None),
    };

    Fx {
        guid,
        index,
        name,
        plugin_name,
        plugin_type,
        enabled,
        offline,
        window_open,
        parameter_count,
        preset_name,
    }
}

/// Build an FxParameter proto struct from a reaper-high FxParameter
fn build_fx_parameter(param: &reaper_high::FxParameter) -> FxParameter {
    let index = param.index();
    let name = param
        .name()
        .map(|n| n.to_str().to_string())
        .unwrap_or_default();
    let value = param.reaper_normalized_value().get();
    let formatted = param
        .formatted_value()
        .map(|f| f.to_str().to_string())
        .unwrap_or_else(|_| format!("{:.2}", value));
    let is_toggle = matches!(param.character(), reaper_high::FxParameterCharacter::Toggle);

    FxParameter {
        index,
        name,
        value,
        formatted,
        is_toggle,
    }
}

// =============================================================================
// FxService Implementation
// =============================================================================

impl FxService for ReaperFx {
    // =========================================================================
    // Chain Queries
    // =========================================================================

    async fn get_fx_list(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
    ) -> Vec<Fx> {
        debug!("ReaperFx::get_fx_list({:?})", context);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return vec![];
        };

        ts.main_thread_future(move || {
            let Some(proj) = resolve_project(&project) else {
                warn!("Project not found");
                return vec![];
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &context) else {
                warn!("FX chain not found");
                return vec![];
            };

            chain.fxs().map(|fx| build_fx_info(&fx)).collect()
        })
        .await
        .unwrap_or_default()
    }

    async fn get_fx(&self, _cx: &Context, project: ProjectContext, target: FxTarget) -> Option<Fx> {
        debug!("ReaperFx::get_fx({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)?;
            let index = resolve_fx_index(&chain, &target.fx)?;
            let fx = chain.fx_by_index(index)?;
            Some(build_fx_info(&fx))
        })
        .await
        .unwrap_or(None)
    }

    async fn fx_count(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
    ) -> u32 {
        debug!("ReaperFx::fx_count({:?})", context);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return 0;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &context)?;
            Some(chain.fx_count())
        })
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0)
    }

    // =========================================================================
    // FX State
    // =========================================================================

    async fn set_fx_enabled(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        enabled: bool,
    ) {
        debug!("ReaperFx::set_fx_enabled({:?}, {})", target, enabled);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &target.context) else {
                return;
            };
            let Some(index) = resolve_fx_index(&chain, &target.fx) else {
                return;
            };
            if let Some(fx) = chain.fx_by_index(index) {
                if enabled {
                    let _ = fx.enable();
                } else {
                    let _ = fx.disable();
                }
            }
        });
    }

    async fn set_fx_offline(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        offline: bool,
    ) {
        debug!("ReaperFx::set_fx_offline({:?}, {})", target, offline);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &target.context) else {
                return;
            };
            let Some(index) = resolve_fx_index(&chain, &target.fx) else {
                return;
            };
            if let Some(fx) = chain.fx_by_index(index) {
                let _ = fx.set_online(!offline);
            }
        });
    }

    // =========================================================================
    // FX Management
    // =========================================================================

    async fn add_fx(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
        name: String,
    ) -> Option<String> {
        debug!("ReaperFx::add_fx({:?}, {:?})", context, name);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &context)?;
            let fx = chain.add_fx_by_original_name(name.as_str())?;
            let guid = fx.get_or_query_guid().ok()?;
            Some(guid.to_string_without_braces())
        })
        .await
        .unwrap_or(None)
    }

    async fn add_fx_at(
        &self,
        _cx: &Context,
        project: ProjectContext,
        request: AddFxAtRequest,
    ) -> Option<String> {
        debug!("ReaperFx::add_fx_at({:?})", request);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &request.context)?;
            // Add to end first, then move to requested position
            let fx = chain.add_fx_by_original_name(request.name.as_str())?;
            let guid = fx.get_or_query_guid().ok()?;
            // Move to requested index if not already there
            let current_index = fx.index();
            if current_index != request.index {
                let is_input = chain.is_input_fx();
                let track = chain.track()?;
                let raw_track = track.raw().ok()?;
                unsafe {
                    Reaper::get().medium_reaper().track_fx_copy_to_track(
                        (raw_track, fx_location(current_index, is_input)),
                        (raw_track, fx_location(request.index, is_input)),
                        TransferBehavior::Move,
                    );
                }
            }
            Some(guid.to_string_without_braces())
        })
        .await
        .unwrap_or(None)
    }

    async fn remove_fx(&self, _cx: &Context, project: ProjectContext, target: FxTarget) {
        debug!("ReaperFx::remove_fx({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((track, chain)) = resolve_fx_chain(&proj, &target.context) else {
                return;
            };
            let Some(index) = resolve_fx_index(&chain, &target.fx) else {
                return;
            };
            let Ok(raw_track) = track.raw() else { return };
            let location = fx_location(index, chain.is_input_fx());
            unsafe {
                let _ = Reaper::get()
                    .medium_reaper()
                    .track_fx_delete(raw_track, location);
            }
        });
    }

    async fn move_fx(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        new_index: u32,
    ) {
        debug!("ReaperFx::move_fx({:?}, {})", target, new_index);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((track, chain)) = resolve_fx_chain(&proj, &target.context) else {
                return;
            };
            let Some(index) = resolve_fx_index(&chain, &target.fx) else {
                return;
            };
            let Ok(raw_track) = track.raw() else { return };
            let is_input = chain.is_input_fx();
            unsafe {
                Reaper::get().medium_reaper().track_fx_copy_to_track(
                    (raw_track, fx_location(index, is_input)),
                    (raw_track, fx_location(new_index, is_input)),
                    TransferBehavior::Move,
                );
            }
        });
    }

    // =========================================================================
    // Parameters
    //
    // Following Snapshooter's approach: identify FX by GUID, parameters by index,
    // values normalized 0.0-1.0.
    // =========================================================================

    async fn get_parameters(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Vec<FxParameter> {
        debug!("ReaperFx::get_parameters({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return vec![];
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)?;
            let index = resolve_fx_index(&chain, &target.fx)?;
            let fx = chain.fx_by_index(index)?;

            Some(
                fx.parameters()
                    .map(|param| build_fx_parameter(&param))
                    .collect::<Vec<_>>(),
            )
        })
        .await
        .unwrap_or(Some(vec![]))
        .unwrap_or_default()
    }

    async fn get_parameter(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        index: u32,
    ) -> Option<FxParameter> {
        debug!("ReaperFx::get_parameter({:?}, {})", target, index);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)?;
            let fx_idx = resolve_fx_index(&chain, &target.fx)?;
            let fx = chain.fx_by_index(fx_idx)?;

            if index >= fx.parameter_count() {
                return None;
            }
            let param = fx.parameter_by_index(index);
            Some(build_fx_parameter(&param))
        })
        .await
        .unwrap_or(None)
    }

    async fn set_parameter(
        &self,
        _cx: &Context,
        project: ProjectContext,
        request: SetParameterRequest,
    ) {
        debug!(
            "ReaperFx::set_parameter({:?}, idx={}, val={})",
            request.target, request.index, request.value
        );

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &request.target.context) else {
                return;
            };
            let Some(fx_idx) = resolve_fx_index(&chain, &request.target.fx) else {
                return;
            };
            if let Some(fx) = chain.fx_by_index(fx_idx) {
                let param = fx.parameter_by_index(request.index);
                let value = reaper_medium::ReaperNormalizedFxParamValue::new(request.value);
                let _ = param.set_reaper_normalized_value(value);
            }
        });
    }

    async fn get_parameter_by_name(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        name: String,
    ) -> Option<FxParameter> {
        debug!("ReaperFx::get_parameter_by_name({:?}, {:?})", target, name);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)?;
            let fx_idx = resolve_fx_index(&chain, &target.fx)?;
            let fx = chain.fx_by_index(fx_idx)?;

            // Search by name (linear scan — parameter lists are typically small)
            for param in fx.parameters() {
                if let Ok(pname) = param.name() {
                    if pname.to_str() == name {
                        return Some(build_fx_parameter(&param));
                    }
                }
            }
            None
        })
        .await
        .unwrap_or(None)
    }

    async fn set_parameter_by_name(
        &self,
        _cx: &Context,
        project: ProjectContext,
        request: SetParameterByNameRequest,
    ) {
        debug!(
            "ReaperFx::set_parameter_by_name({:?}, {:?}, {})",
            request.target, request.name, request.value
        );

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &request.target.context) else {
                return;
            };
            let Some(fx_idx) = resolve_fx_index(&chain, &request.target.fx) else {
                return;
            };
            if let Some(fx) = chain.fx_by_index(fx_idx) {
                for param in fx.parameters() {
                    if let Ok(pname) = param.name() {
                        if pname.to_str() == request.name {
                            let value =
                                reaper_medium::ReaperNormalizedFxParamValue::new(request.value);
                            let _ = param.set_reaper_normalized_value(value);
                            return;
                        }
                    }
                }
            }
        });
    }

    // =========================================================================
    // Presets
    // =========================================================================

    async fn next_preset(&self, _cx: &Context, project: ProjectContext, target: FxTarget) {
        debug!("ReaperFx::next_preset({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((track, chain)) = resolve_fx_chain(&proj, &target.context) else {
                return;
            };
            let Some(index) = resolve_fx_index(&chain, &target.fx) else {
                return;
            };
            let Ok(raw_track) = track.raw() else { return };
            let location = fx_location(index, chain.is_input_fx());
            unsafe {
                let _ = Reaper::get()
                    .medium_reaper()
                    .track_fx_navigate_presets(raw_track, location, 1);
            }
        });
    }

    async fn prev_preset(&self, _cx: &Context, project: ProjectContext, target: FxTarget) {
        debug!("ReaperFx::prev_preset({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((track, chain)) = resolve_fx_chain(&proj, &target.context) else {
                return;
            };
            let Some(index) = resolve_fx_index(&chain, &target.fx) else {
                return;
            };
            let Ok(raw_track) = track.raw() else { return };
            let location = fx_location(index, chain.is_input_fx());
            unsafe {
                let _ = Reaper::get()
                    .medium_reaper()
                    .track_fx_navigate_presets(raw_track, location, -1);
            }
        });
    }

    async fn set_preset(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        index: u32,
    ) {
        debug!("ReaperFx::set_preset({:?}, {})", target, index);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((track, chain)) = resolve_fx_chain(&proj, &target.context) else {
                return;
            };
            let Some(fx_idx) = resolve_fx_index(&chain, &target.fx) else {
                return;
            };
            let Ok(raw_track) = track.raw() else { return };
            let location = fx_location(fx_idx, chain.is_input_fx());
            unsafe {
                let _ = Reaper::get().medium_reaper().track_fx_set_preset_by_index(
                    raw_track,
                    location,
                    FxPresetRef::Preset(index),
                );
            }
        });
    }

    // =========================================================================
    // UI
    // =========================================================================

    async fn open_fx_ui(&self, _cx: &Context, project: ProjectContext, target: FxTarget) {
        debug!("ReaperFx::open_fx_ui({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &target.context) else {
                return;
            };
            let Some(index) = resolve_fx_index(&chain, &target.fx) else {
                return;
            };
            if let Some(fx) = chain.fx_by_index(index) {
                let _ = fx.show_in_floating_window();
            }
        });
    }

    async fn close_fx_ui(&self, _cx: &Context, project: ProjectContext, target: FxTarget) {
        debug!("ReaperFx::close_fx_ui({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &target.context) else {
                return;
            };
            let Some(index) = resolve_fx_index(&chain, &target.fx) else {
                return;
            };
            if let Some(fx) = chain.fx_by_index(index) {
                let _ = fx.hide_floating_window();
            }
        });
    }

    async fn toggle_fx_ui(&self, _cx: &Context, project: ProjectContext, target: FxTarget) {
        debug!("ReaperFx::toggle_fx_ui({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &target.context) else {
                return;
            };
            let Some(index) = resolve_fx_index(&chain, &target.fx) else {
                return;
            };
            if let Some(fx) = chain.fx_by_index(index) {
                if fx.window_is_open() {
                    let _ = fx.hide_floating_window();
                } else {
                    let _ = fx.show_in_floating_window();
                }
            }
        });
    }

    // =========================================================================
    // Advanced
    // =========================================================================

    async fn get_named_config(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        key: String,
    ) -> Option<String> {
        debug!("ReaperFx::get_named_config({:?}, {:?})", target, key);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)?;
            let index = resolve_fx_index(&chain, &target.fx)?;
            let fx = chain.fx_by_index(index)?;
            let bytes = fx.get_named_config_param(&*key, 4096).ok()?;
            Some(String::from_utf8_lossy(&bytes).to_string())
        })
        .await
        .unwrap_or(None)
    }

    async fn set_named_config(
        &self,
        _cx: &Context,
        project: ProjectContext,
        request: SetNamedConfigRequest,
    ) {
        debug!(
            "ReaperFx::set_named_config({:?}, {:?})",
            request.target, request.key
        );

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &request.target.context) else {
                return;
            };
            let Some(index) = resolve_fx_index(&chain, &request.target.fx) else {
                return;
            };
            if let Some(fx) = chain.fx_by_index(index) {
                if let Ok(c_string) = std::ffi::CString::new(request.value) {
                    unsafe {
                        let _ = fx.set_named_config_param(&*request.key, c_string.as_ptr());
                    }
                }
            }
        });
    }

    async fn get_fx_latency(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> FxLatency {
        debug!("ReaperFx::get_fx_latency({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return FxLatency::default();
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)?;
            let index = resolve_fx_index(&chain, &target.fx)?;

            // Use named config param "pdc" for PDC info
            let fx = chain.fx_by_index(index)?;
            let pdc = fx
                .get_named_config_param("pdc", 64)
                .ok()
                .and_then(|bytes| {
                    let s = String::from_utf8_lossy(&bytes);
                    s.trim().parse::<i32>().ok()
                })
                .unwrap_or(0);

            Some(FxLatency {
                pdc_samples: pdc,
                chain_pdc_actual: 0, // Would need TrackFX_GetNamedConfigParm with other keys
                chain_pdc_reporting: 0,
            })
        })
        .await
        .unwrap_or(Some(FxLatency::default()))
        .unwrap_or_default()
    }

    async fn get_param_modulation(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        target: FxTarget,
        param_index: u32,
    ) -> FxParamModulation {
        debug!(
            "ReaperFx::get_param_modulation({:?}, {})",
            target, param_index
        );

        // Parameter modulation info requires parsing the track chunk or using
        // named config params. For now, return defaults — this can be enhanced
        // when we need LFO/linking info for the rig UI.
        FxParamModulation::default()
    }

    // =========================================================================
    // State Chunks
    //
    // Uses reaper-high's vst_chunk / set_vst_chunk for individual FX binary
    // state, and iterates the chain for full chain capture/restore.
    // =========================================================================

    async fn get_fx_state_chunk(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Option<Vec<u8>> {
        debug!("ReaperFx::get_fx_state_chunk({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)?;
            let index = resolve_fx_index(&chain, &target.fx)?;
            let fx = chain.fx_by_index(index)?;
            fx.vst_chunk().ok()
        })
        .await
        .unwrap_or(None)
    }

    async fn set_fx_state_chunk(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        chunk: Vec<u8>,
    ) {
        debug!("ReaperFx::set_fx_state_chunk({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &target.context) else {
                return;
            };
            let Some(index) = resolve_fx_index(&chain, &target.fx) else {
                return;
            };
            if let Some(fx) = chain.fx_by_index(index) {
                let _ = fx.set_vst_chunk(&chunk);
            }
        });
    }

    async fn get_fx_state_chunk_encoded(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Option<String> {
        debug!("ReaperFx::get_fx_state_chunk_encoded({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)?;
            let index = resolve_fx_index(&chain, &target.fx)?;
            let fx = chain.fx_by_index(index)?;
            fx.vst_chunk_encoded().ok().map(|s| s.to_str().to_string())
        })
        .await
        .unwrap_or(None)
    }

    async fn set_fx_state_chunk_encoded(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        encoded: String,
    ) {
        debug!("ReaperFx::set_fx_state_chunk_encoded({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &target.context) else {
                return;
            };
            let Some(index) = resolve_fx_index(&chain, &target.fx) else {
                return;
            };
            if let Some(fx) = chain.fx_by_index(index) {
                let _ = fx.set_vst_chunk_encoded(encoded);
            }
        });
    }

    async fn get_fx_chain_state(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
    ) -> Vec<FxStateChunk> {
        debug!("ReaperFx::get_fx_chain_state({:?})", context);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return vec![];
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &context)?;

            let mut chunks = Vec::new();
            for fx in chain.fxs() {
                let guid = fx
                    .get_or_query_guid()
                    .map(|g| g.to_string_without_braces())
                    .unwrap_or_default();
                let plugin_name = fx.name().to_str().to_string();
                let index = fx.index();

                // Get the base64-encoded VST chunk
                if let Ok(encoded) = fx.vst_chunk_encoded() {
                    chunks.push(FxStateChunk {
                        fx_guid: guid,
                        fx_index: index,
                        plugin_name,
                        encoded_chunk: encoded.to_str().to_string(),
                    });
                }
            }
            Some(chunks)
        })
        .await
        .unwrap_or(Some(vec![]))
        .unwrap_or_default()
    }

    async fn set_fx_chain_state(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
        chunks: Vec<FxStateChunk>,
    ) {
        debug!(
            "ReaperFx::set_fx_chain_state({:?}, {} chunks)",
            context,
            chunks.len()
        );

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return;
        };

        // Apply all chunks in a single main-thread operation for atomicity
        let _ = ts.do_later_in_main_thread_asap(move || {
            let Some(proj) = resolve_project(&project) else {
                return;
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &context) else {
                return;
            };

            // Build a GUID → FX index lookup (like Snapshooter's hashmap approach)
            let mut guid_to_index = std::collections::HashMap::new();
            for fx in chain.fxs() {
                if let Ok(guid) = fx.get_or_query_guid() {
                    guid_to_index.insert(guid.to_string_without_braces(), fx.index());
                }
            }

            // Apply each chunk by matching GUID
            for chunk in &chunks {
                if let Some(&fx_index) = guid_to_index.get(&chunk.fx_guid) {
                    if let Some(fx) = chain.fx_by_index(fx_index) {
                        let _ = fx.set_vst_chunk_encoded(chunk.encoded_chunk.clone());
                    }
                } else {
                    debug!(
                        "FX GUID {} not found in chain, skipping (plugin: {})",
                        chunk.fx_guid, chunk.plugin_name
                    );
                }
            }
        });
    }

    // =========================================================================
    // Observation / Subscriptions
    // =========================================================================

    async fn subscribe_fx_events(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
        events: Tx<FxEvent>,
    ) {
        info!("ReaperFx: subscribe_fx_events for {:?}", context);

        // Register this chain for monitoring so poll_and_broadcast_fx() polls it
        register_monitored_chain(project, context.clone());

        // Get a receiver from the broadcast channel
        let Some(mut rx) = fx_event_receiver() else {
            info!("FX broadcaster not initialized, subscriber will not receive events");
            return;
        };

        // Spawn a forwarding loop that filters events for this specific chain
        let target_context = context;
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        // Filter: only forward events for the subscribed chain
                        let event_context = match &event {
                            FxEvent::Added { context, .. } => context,
                            FxEvent::Removed { context, .. } => context,
                            FxEvent::EnabledChanged { context, .. } => context,
                            FxEvent::Moved { context, .. } => context,
                            FxEvent::ParameterChanged { context, .. } => context,
                            FxEvent::PresetChanged { context, .. } => context,
                            FxEvent::WindowChanged { context, .. } => context,
                        };

                        if format!("{:?}", event_context) == format!("{:?}", target_context) {
                            if let Err(e) = events.send(&event).await {
                                debug!("FX event subscriber disconnected: {}", e);
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        debug!("FX event subscriber lagged by {} messages", count);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("FX broadcast channel closed");
                        break;
                    }
                }
            }
            info!("FX event subscription ended");
        });
    }
}

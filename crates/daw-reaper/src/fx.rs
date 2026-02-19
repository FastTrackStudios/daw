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
    AddFxAtRequest, CreateContainerRequest, EncloseInContainerRequest, Fx, FxChainContext,
    FxChannelConfig, FxContainerChannelConfig, FxEvent, FxLatency, FxNode, FxNodeId,
    FxParamModulation, FxParameter, FxPinMappings, FxPresetIndex, FxRef, FxRoutingMode, FxService,
    FxStateChunk, FxTarget, FxTree, FxType, MoveFromContainerRequest, MoveToContainerRequest,
    ProjectContext, SetContainerChannelConfigRequest, SetNamedConfigRequest,
    SetParameterByNameRequest, SetParameterRequest,
};
use reaper_high::{FxChain, MAX_TRACK_CHUNK_SIZE, Reaper, Track};
use reaper_medium::{ChunkCacheHint, FxPresetRef, TrackFxLocation, TransferBehavior};
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

/// Cached state for a container in the FX chain (for structure change detection)
#[derive(Clone, Debug, PartialEq)]
struct CachedContainerState {
    /// Synthetic container ID (e.g. "container:2")
    node_id: String,
    /// Display name of the container
    name: String,
    /// Routing mode (serial=0, parallel=1)
    routing_mode: u32,
    /// Number of direct children
    child_count: u32,
    /// GUIDs of direct child plugins (containers are represented by their node_id prefixed with "c:")
    child_ids: Vec<String>,
}

/// Cached state for an entire FX chain (for change detection)
#[derive(Clone, Debug)]
struct CachedChainState {
    /// Ordered list of FX states (by chain index)
    fx_states: Vec<CachedFxState>,
    /// Container tree structure snapshot (for detecting structural changes)
    containers: Vec<CachedContainerState>,
}

/// Key for identifying an FX chain (project + chain context).
/// Uses proper PartialEq/Hash on the domain types — no string allocation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ChainKey {
    project_guid: String,
    context: FxChainContext,
}

impl ChainKey {
    fn new(project_guid: &str, context: &FxChainContext) -> Self {
        Self {
            project_guid: project_guid.to_string(),
            context: context.clone(),
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
        let already = chains.iter().any(|(p, c)| p == &project && c == &context);
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
        let name = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            fx.name().to_str().to_string()
        }))
        .unwrap_or_else(|_| "(unknown)".to_string());
        let index = fx.index();
        let enabled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fx.is_enabled()))
            .unwrap_or(false);

        // Read parameter values (up to MAX_MONITORED_PARAMS)
        let param_count =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fx.parameter_count()))
                .unwrap_or(0)
                .min(MAX_MONITORED_PARAMS);
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

    // Snapshot container structure for tree change detection
    let containers = read_container_states(chain);

    CachedChainState {
        fx_states,
        containers,
    }
}

/// Read container structure from an FX chain for caching.
/// Walks top-level FX looking for containers, then recursively snapshots nested ones.
fn read_container_states(chain: &FxChain) -> Vec<CachedContainerState> {
    let mut containers = Vec::new();
    let top_count = chain.fx_count();

    for i in 0..top_count {
        let fx = chain.fx_by_index_untracked(i);
        if is_container_fx(&fx) {
            snapshot_container(chain, &fx, &format!("{}", i), &mut containers);
        }
    }

    containers
}

/// Recursively snapshot a container and its nested containers.
/// Uses `container_item.X` API to get child FX IDs safely.
fn snapshot_container(
    chain: &FxChain,
    container_fx: &reaper_high::Fx,
    path: &str,
    out: &mut Vec<CachedContainerState>,
) {
    let child_count = read_config_u32(container_fx, "container_count");
    let name = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        container_fx.name().to_str().to_string()
    }))
    .unwrap_or_else(|_| "Container".to_string());
    let routing_mode = read_config_u32(container_fx, "parallel");

    let mut child_ids = Vec::with_capacity(child_count as usize);

    for i in 0..child_count {
        let Some(child_raw) = container_child_fx_id(container_fx, i) else {
            continue;
        };
        let child_fx = chain.fx_by_index_untracked(child_raw);

        if is_container_fx(&child_fx) {
            let child_path = format!("{}:{}", path, i);
            child_ids.push(format!("c:{}", child_path));
            snapshot_container(chain, &child_fx, &child_path, out);
        } else {
            // Plugin child — identify by GUID
            let guid = reaper_high::get_fx_guid(chain, child_raw)
                .map(|g| g.to_string_without_braces())
                .unwrap_or_default();
            child_ids.push(guid);
        }
    }

    out.push(CachedContainerState {
        node_id: format!("container:{}", path),
        name,
        routing_mode,
        child_count,
        child_ids,
    });
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

    // =========================================================================
    // Container structure diffing
    // =========================================================================
    diff_containers(tx, context, &prev.containers, &curr.containers);
}

/// Diff container snapshots and emit container-specific events.
fn diff_containers(
    tx: &broadcast::Sender<FxEvent>,
    context: &FxChainContext,
    prev: &[CachedContainerState],
    curr: &[CachedContainerState],
) {
    let prev_map: HashMap<&str, &CachedContainerState> =
        prev.iter().map(|c| (c.node_id.as_str(), c)).collect();
    let curr_map: HashMap<&str, &CachedContainerState> =
        curr.iter().map(|c| (c.node_id.as_str(), c)).collect();

    // Detect new containers
    for c in curr {
        if !prev_map.contains_key(c.node_id.as_str()) {
            let _ = tx.send(FxEvent::ContainerCreated {
                context: context.clone(),
                container_id: FxNodeId::container(&c.node_id["container:".len()..]),
                name: c.name.clone(),
            });
        }
    }

    // Detect removed containers
    for p in prev {
        if !curr_map.contains_key(p.node_id.as_str()) {
            let _ = tx.send(FxEvent::ContainerRemoved {
                context: context.clone(),
                container_id: FxNodeId::container(&p.node_id["container:".len()..]),
            });
        }
    }

    // Detect changes in existing containers
    let mut tree_changed = false;
    for c in curr {
        if let Some(p) = prev_map.get(c.node_id.as_str()) {
            let container_id = FxNodeId::container(&c.node_id["container:".len()..]);

            // Routing mode change
            if p.routing_mode != c.routing_mode {
                let _ = tx.send(FxEvent::RoutingModeChanged {
                    context: context.clone(),
                    container_id: container_id.clone(),
                    mode: FxRoutingMode::from_reaper_param(&c.routing_mode.to_string()),
                });
            }

            // Name change
            if p.name != c.name {
                let _ = tx.send(FxEvent::ContainerRenamed {
                    context: context.clone(),
                    container_id: container_id.clone(),
                    name: c.name.clone(),
                });
            }

            // Child list changed (additions, removals, reordering)
            if p.child_ids != c.child_ids {
                tree_changed = true;

                // Emit MovedToContainer for plugins that moved INTO this container
                for child_id in &c.child_ids {
                    if !child_id.starts_with("c:") && !p.child_ids.contains(child_id) {
                        // Find where this plugin was before (which container had it)
                        let source = prev.iter().find(|pc| pc.child_ids.contains(child_id));
                        let _ = tx.send(FxEvent::MovedToContainer {
                            context: context.clone(),
                            node_id: FxNodeId::from_guid(child_id),
                            source_container: source
                                .map(|s| FxNodeId::container(&s.node_id["container:".len()..])),
                            dest_container: container_id.clone(),
                        });
                    }
                }
            }
        }
    }

    // Emit catch-all if tree structure changed in ways beyond individual events
    if tree_changed {
        let _ = tx.send(FxEvent::TreeStructureChanged {
            context: context.clone(),
        });
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
            // Container-aware GUID lookup: scans top-level FX first,
            // then recursively walks into containers using container_item.X.
            let node_id = FxNodeId::from_guid(guid.clone());
            resolve_plugin_guid(chain, &node_id)
        }
        FxRef::Name(name) => {
            // Search by name (first match)
            for fx in chain.index_based_fxs() {
                let fx_name = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    fx.name().to_str().to_string()
                }));
                if let Ok(n) = fx_name {
                    if n == *name {
                        return Some(fx.index());
                    }
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

/// Find the byte offset of the closing `>` for an RPP block.
///
/// RPP uses `<TAG` to open blocks and a standalone `>` (possibly indented)
/// to close them. This function tracks nesting depth line-by-line and returns
/// the byte offset of the closing `>` character.
fn find_block_end(block_text: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut offset = 0usize;

    for line in block_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('<') {
            depth += 1;
        }
        if trimmed == ">" {
            depth -= 1;
            if depth == 0 {
                // Return the offset of the `>` character in this line
                let gt_pos = line.rfind('>').unwrap();
                return Some(offset + gt_pos);
            }
        }
        offset += line.len() + 1; // +1 for newline
    }
    None
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

/// Build an Fx proto struct from a reaper-high Fx.
///
/// Uses `catch_unwind` around methods that call `.expect()` internally
/// (like `name()`) to prevent panics from crashing REAPER when an FX
/// reference is stale (e.g. container child with an invalid index).
///
/// The optional `chain` parameter enables a fallback GUID lookup for
/// container children. `get_or_query_guid()` internally calls the
/// bounds-checked `fx_by_index()`, which always fails for container-encoded
/// indices (0x2000000+). When a chain is provided and the primary lookup
/// fails, we fall back to `get_fx_guid(chain, raw_index)` which passes
/// the encoded index directly to REAPER's `TrackFX_GetFXGUID`.
fn build_fx_info(fx: &reaper_high::Fx, chain: Option<&FxChain>) -> Fx {
    let guid = fx
        .get_or_query_guid()
        .map(|g| g.to_string_without_braces())
        .unwrap_or_else(|_| {
            // Fallback: use get_fx_guid which bypasses bounds checking
            chain
                .and_then(|c| reaper_high::get_fx_guid(c, fx.index()))
                .map(|g| g.to_string_without_braces())
                .unwrap_or_default()
        });
    let name = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fx.name().to_str().to_string()
    }))
    .unwrap_or_else(|_| "(unknown)".to_string());
    let index = fx.index();
    let enabled =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fx.is_enabled())).unwrap_or(false);
    let offline =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| !fx.is_online())).unwrap_or(false);
    let window_open =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fx.window_is_open()))
            .unwrap_or(false);
    let parameter_count =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fx.parameter_count()))
            .unwrap_or(0);

    // Get plugin type and name via info() (REAPER >= 6.37)
    let (plugin_name, plugin_type) = match fx.info() {
        Ok(info) => {
            let ptype = parse_fx_type(&info.sub_type_expression);
            (info.effect_name, ptype)
        }
        Err(_) => (name.clone(), FxType::Unknown),
    };

    // Get preset name via dedicated TrackFX_GetPreset API
    let preset_name = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fx.preset_name()
            .map(|rs| rs.to_str().to_string())
            .filter(|s| !s.is_empty())
    }))
    .unwrap_or(None);

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

    let character = param.character();
    let is_toggle = matches!(character, reaper_high::FxParameterCharacter::Toggle);
    let step_sizes_result = param.step_sizes();

    debug!(
        "  param[{}] '{}': character={:?}, step_sizes={:?}, is_toggle={}",
        index, name, character, step_sizes_result, is_toggle
    );

    // Detect discrete parameters via step_sizes() or character().
    // VST plugins may report Discrete character but no step_sizes.
    // CLAP plugins may report step_sizes but not Discrete character.
    // We check both paths.
    let is_discrete_character = matches!(character, reaper_high::FxParameterCharacter::Discrete);

    let step_count_from_sizes = step_sizes_result.and_then(|ss| {
        if let reaper_medium::GetParameterStepSizesResult::Normal { normal_step, .. } = ss {
            if normal_step > 0.0 {
                let n = (1.0 / normal_step).round() as u32;
                if n >= 2 && n <= 256 { Some(n) } else { None }
            } else {
                None
            }
        } else {
            None
        }
    });

    // If character says Discrete but step_sizes didn't give us a count,
    // try to get the count from the formatted values by probing.
    let step_count = step_count_from_sizes.or_else(|| {
        if is_discrete_character {
            // For VST discrete params without step_sizes, probe for step count
            // by checking how many distinct formatted values exist in 0..1 range
            let mut count = 0u32;
            let mut last_label = String::new();
            for i in 0..=256 {
                let norm = (i as f64) / 256.0;
                if let Ok(s) = param.format_reaper_normalized_value(
                    reaper_medium::ReaperNormalizedFxParamValue::new(norm),
                ) {
                    let label = s.to_str().to_string();
                    if label != last_label {
                        count += 1;
                        last_label = label;
                    }
                }
            }
            if count >= 2 && count <= 256 {
                debug!("    → discrete from character probe: {} steps", count);
                Some(count)
            } else {
                None
            }
        } else {
            None
        }
    });

    debug!("    → final step_count={:?}", step_count);

    // Enumerate labels for discrete parameters by formatting each step value
    let step_labels = if let Some(n) = step_count {
        let mut labels = Vec::with_capacity(n as usize);
        for i in 0..n {
            // Normalized value for step i out of (n-1) steps
            let norm = if n <= 1 {
                0.0
            } else {
                (i as f64) / ((n - 1) as f64)
            };
            let label = param
                .format_reaper_normalized_value(reaper_medium::ReaperNormalizedFxParamValue::new(
                    norm,
                ))
                .map(|s| s.to_str().to_string())
                .unwrap_or_else(|_| format!("{}", i));
            labels.push((norm, label));
        }
        debug!(
            "    → step_labels for '{}': {:?}",
            name,
            labels
                .iter()
                .map(|(n, l)| format!("{:.3}={}", n, l))
                .collect::<Vec<_>>()
        );
        labels
    } else {
        Vec::new()
    };

    FxParameter {
        index,
        name,
        value,
        formatted,
        is_toggle,
        step_count,
        step_labels,
    }
}

// =============================================================================
// FX Tree Building — recursive container traversal
//
// REAPER's FX container system uses stride-based addressing:
// - Top-level FX: flat indices 0..count-1
// - Container children: 0x2000000 + container_flat_index + (stride * child_pos)
// - Stride grows at each depth: new_stride = (child_count + 1) * prev_stride
//
// We use the high-level Fx API for most queries (name, GUID, named config params)
// since fx_by_index() doesn't validate indices. The one exception is is_enabled()
// which has an is_available() guard — for that we call medium-level API directly.
// =============================================================================

/// REAPER container addressing base offset.
const CONTAINER_BASE: u32 = 0x2000000;

/// Build the complete FxNode tree from an FxChain.
///
/// Entry point that iterates top-level FX (flat indices 0..count-1) and
/// recurses into containers.
fn build_fx_tree_from_chain(chain: &FxChain, _is_input: bool, top_level_count: u32) -> Vec<FxNode> {
    let mut nodes = Vec::new();

    for i in 0..top_level_count {
        // Use untracked to avoid bounds check (fx_by_index returns None for >= fx_count)
        let fx = chain.fx_by_index_untracked(i);
        let path_prefix = format!("{}", i);

        if is_container_fx(&fx) {
            nodes.push(build_container_node(
                chain,
                &fx,
                i,    // flat index for this container
                None, // no parent (top-level)
                &path_prefix,
            ));
        } else {
            nodes.push(build_plugin_node(chain, &fx, None));
        }
    }

    nodes
}

/// Check if an FX slot is a container.
///
/// Uses multiple detection methods because `get_named_config_param` returns
/// raw bytes that may contain null terminators or binary data that doesn't
/// cleanly compare as a string:
///
/// 1. `fx_type` config param == "Container" (primary, may fail with raw bytes)
/// 2. `container_count` config param > 0 (reliable — only containers have this)
/// 3. `info().sub_type_expression` == "Container" (uses the string-safe API)
fn is_container_fx(fx: &reaper_high::Fx) -> bool {
    // Method 1: check fx_type via raw bytes
    if let Some(ft) = read_config_str(fx, "fx_type") {
        if ft == "Container" {
            return true;
        }
    }

    // Method 2: if container_count exists and is > 0, it's definitely a container
    // (also catches containers with 0 children, since the param still exists)
    if let Some(cc) = read_config_str(fx, "container_count") {
        if cc.parse::<u32>().unwrap_or(0) > 0 {
            return true;
        }
        // Even if container_count is "0", the param existing means it's a container
        // (an empty container still returns "0")
        if cc.parse::<u32>().is_ok() {
            return true;
        }
    }

    // Method 3: use the higher-level info() API which handles string conversion properly
    if let Ok(info) = fx.info() {
        if info.sub_type_expression == "Container" {
            return true;
        }
    }

    false
}

/// Read a named config param as a trimmed string, returning None on failure.
///
/// Note: `get_named_config_param` returns raw `Vec<u8>` which may contain
/// null terminators. We strip them before converting to a string.
fn read_config_str(fx: &reaper_high::Fx, key: &str) -> Option<String> {
    fx.get_named_config_param(key, 256).ok().map(|bytes| {
        // Strip null terminators and trailing whitespace
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).trim().to_string()
    })
}

/// Read a named config param as u32, returning 0 on failure.
fn read_config_u32(fx: &reaper_high::Fx, key: &str) -> u32 {
    read_config_str(fx, key)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

/// Read a named config param as i32, returning 0 on failure.
fn read_config_i32(fx: &reaper_high::Fx, key: &str) -> i32 {
    read_config_str(fx, key)
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0)
}

/// Read a named config param as f64, returning 0.0 on failure.
fn read_config_f64(fx: &reaper_high::Fx, key: &str) -> f64 {
    read_config_str(fx, key)
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
}

/// Get the raw FX index for child `child_index` (0-based) inside a container,
/// using REAPER's v7.06+ `container_item.X` named config param.
///
/// This is much safer than computing stride-based addresses manually, because
/// REAPER validates the index and returns the correct encoded FX ID directly.
fn container_child_fx_id(container_fx: &reaper_high::Fx, child_index: u32) -> Option<u32> {
    let key = format!("container_item.{}", child_index);
    read_config_str(container_fx, &key).and_then(|s| s.parse::<u32>().ok())
}

/// Verify that a container has the expected child count.
///
/// Used after container mutations (move_to, move_from, enclose, explode) to detect
/// silent failures from incorrect stride addressing or stale indices.
fn verify_container_child_count(
    chain: &FxChain,
    container_index: u32,
    expected_count: u32,
) -> Result<(), String> {
    let container_fx = chain.fx_by_index_untracked(container_index);
    let actual = read_config_u32(&container_fx, "container_count");
    if actual != expected_count {
        Err(format!(
            "container at index {} has {} children, expected {}",
            container_index, actual, expected_count
        ))
    } else {
        Ok(())
    }
}

/// Build an FxNode for a plugin (non-container) FX.
fn build_plugin_node(chain: &FxChain, fx: &reaper_high::Fx, parent_id: Option<FxNodeId>) -> FxNode {
    let fx_info = build_fx_info(fx, Some(chain));
    let enabled = fx_info.enabled;
    let guid = fx_info.guid.clone();
    FxNode::plugin(FxNodeId::from_guid(guid), fx_info, enabled, parent_id)
}

/// Build an FxNode for a container, recursively building its children.
///
/// Uses REAPER's v7.06+ `container_item.X` API to get child FX IDs directly,
/// avoiding manual stride-based address computation which was error-prone.
fn build_container_node(
    chain: &FxChain,
    container_fx: &reaper_high::Fx,
    _container_flat_index: u32,
    parent_id: Option<FxNodeId>,
    path: &str,
) -> FxNode {
    let container_id = FxNodeId::container(path);

    // Read container properties
    let child_count = read_config_u32(container_fx, "container_count");
    let routing = read_config_str(container_fx, "parallel")
        .map(|s| FxRoutingMode::from_reaper_param(&s))
        .unwrap_or_default();
    let channel_config = FxContainerChannelConfig {
        nch: read_config_u32(container_fx, "container_nch"),
        nch_in: read_config_u32(container_fx, "container_nch_in"),
        nch_out: read_config_u32(container_fx, "container_nch_out"),
    };

    // Container name: try renamed_name first, fall back to FX name
    let name = read_config_str(container_fx, "renamed_name")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                container_fx.name().to_str().to_string()
            }))
            .unwrap_or_else(|_| "Container".to_string())
        });

    let enabled =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| container_fx.is_enabled()))
            .unwrap_or(true);

    // Build children using container_item.X API
    let mut children = Vec::new();
    for i in 0..child_count {
        let Some(child_raw) = container_child_fx_id(container_fx, i) else {
            warn!(
                "container_item.{} not found in container at path {}",
                i, path
            );
            continue;
        };

        let child_fx = chain.fx_by_index_untracked(child_raw);
        let child_path = format!("{}:{}", path, i);

        if is_container_fx(&child_fx) {
            children.push(build_container_node(
                chain,
                &child_fx,
                child_raw,
                Some(container_id.clone()),
                &child_path,
            ));
        } else {
            children.push(build_plugin_node(
                chain,
                &child_fx,
                Some(container_id.clone()),
            ));
        }
    }

    let mut node = FxNode::container(
        container_id,
        name,
        routing,
        channel_config,
        enabled,
        parent_id,
    );
    if let Some(c) = node.children_mut() {
        *c = children;
    }
    node
}

// =============================================================================
// FxNodeId Resolution Layer
//
// Maps stable FxNodeId values to/from REAPER's raw stride-based FX indices.
// Plugin nodes resolve via GUID scan. Container nodes resolve via path-based
// stride math (the path "container:2:1" encodes the traversal through the tree).
// =============================================================================

/// Resolve an FxNodeId to a raw REAPER FX index within a chain.
///
/// - **Plugin nodes** (GUID-based): scans the chain for a matching GUID, then
///   checks nested containers if not found at the top level.
/// - **Container nodes** (path-based): parses the path segments and walks the
///   stride math to compute the raw index.
///
/// Returns the raw index suitable for `TrackFxLocation::NormalFxChain(raw)` or
/// `TrackFxLocation::InputFxChain(raw)`.
fn resolve_node_to_raw_index(chain: &FxChain, node_id: &FxNodeId) -> Option<u32> {
    if node_id.is_container() {
        resolve_container_path(chain, node_id)
    } else {
        resolve_plugin_guid(chain, node_id)
    }
}

/// Resolve a container FxNodeId by parsing its path and using `container_item.X`
/// to walk through nested containers.
///
/// Path format: "container:top_idx:child_idx:grandchild_idx:..."
/// The first segment is the top-level FX chain index; subsequent segments
/// are child positions within nested containers.
fn resolve_container_path(chain: &FxChain, node_id: &FxNodeId) -> Option<u32> {
    let path_str = node_id.as_str().strip_prefix("container:")?;
    let segments: Vec<u32> = path_str.split(':').filter_map(|s| s.parse().ok()).collect();

    if segments.is_empty() {
        return None;
    }

    // First segment is the top-level FX chain index
    let top_index = segments[0];
    if top_index >= chain.fx_count() {
        return None;
    }

    if segments.len() == 1 {
        // Top-level container — its raw index is just the flat index
        return Some(top_index);
    }

    // Walk into nested containers using container_item.X
    let mut current_addr = top_index;

    for &child_pos in &segments[1..] {
        let container_fx = chain.fx_by_index_untracked(current_addr);
        let child_raw = container_child_fx_id(&container_fx, child_pos)?;
        current_addr = child_raw;
    }

    Some(current_addr)
}

/// Resolve a plugin FxNodeId by scanning for its GUID.
///
/// First checks top-level FX (fast path), then recursively scans containers.
fn resolve_plugin_guid(chain: &FxChain, node_id: &FxNodeId) -> Option<u32> {
    let target_guid = node_id.as_str();

    // Fast path: check top-level FX
    let top_count = chain.fx_count();
    for i in 0..top_count {
        let guid = reaper_high::get_fx_guid(chain, i).map(|g| g.to_string_without_braces());
        if guid.as_deref() == Some(target_guid) {
            return Some(i);
        }
    }

    // Slow path: scan inside containers recursively
    scan_containers_for_guid(chain, target_guid, top_count)
}

/// Recursively scan containers at the top level looking for a plugin with the given GUID.
fn scan_containers_for_guid(chain: &FxChain, target_guid: &str, top_count: u32) -> Option<u32> {
    for i in 0..top_count {
        let fx = chain.fx_by_index_untracked(i);
        if is_container_fx(&fx) {
            if let Some(raw) = scan_children_for_guid(chain, &fx, target_guid) {
                return Some(raw);
            }
        }
    }
    None
}

/// Recursively scan children of a container for a plugin with the given GUID.
/// Uses `container_item.X` to get child FX IDs.
fn scan_children_for_guid(
    chain: &FxChain,
    container_fx: &reaper_high::Fx,
    target_guid: &str,
) -> Option<u32> {
    let child_count = read_config_u32(container_fx, "container_count");

    for i in 0..child_count {
        let Some(child_raw) = container_child_fx_id(container_fx, i) else {
            continue;
        };
        let child_fx = chain.fx_by_index_untracked(child_raw);

        if is_container_fx(&child_fx) {
            // Recurse into nested container
            if let Some(raw) = scan_children_for_guid(chain, &child_fx, target_guid) {
                return Some(raw);
            }
        } else {
            // Check GUID of this plugin
            let guid =
                reaper_high::get_fx_guid(chain, child_raw).map(|g| g.to_string_without_braces());
            if guid.as_deref() == Some(target_guid) {
                return Some(child_raw);
            }
        }
    }
    None
}

/// Build a mapping from raw REAPER index to FxNodeId by walking the tree.
///
/// This is the reverse of `resolve_node_to_raw_index`. Walks the tree using
/// `container_item.X` and matches against the target raw index.
fn raw_index_to_node_id(chain: &FxChain, raw_index: u32) -> Option<FxNodeId> {
    let top_count = chain.fx_count();

    // Check top-level FX first
    for i in 0..top_count {
        if i == raw_index {
            let fx = chain.fx_by_index_untracked(i);
            if is_container_fx(&fx) {
                return Some(FxNodeId::container(format!("{}", i)));
            } else {
                let guid =
                    reaper_high::get_fx_guid(chain, i).map(|g| g.to_string_without_braces())?;
                return Some(FxNodeId::from_guid(guid));
            }
        }

        // If this is a container, search its children
        let fx = chain.fx_by_index_untracked(i);
        if is_container_fx(&fx) {
            if let Some(id) = search_children_for_raw(chain, &fx, raw_index, &format!("{}", i)) {
                return Some(id);
            }
        }
    }

    None
}

/// Recursively search children for a specific raw index, returning its FxNodeId.
/// Uses `container_item.X` to get child FX IDs.
fn search_children_for_raw(
    chain: &FxChain,
    container_fx: &reaper_high::Fx,
    target_raw: u32,
    parent_path: &str,
) -> Option<FxNodeId> {
    let child_count = read_config_u32(container_fx, "container_count");

    for i in 0..child_count {
        let Some(child_raw) = container_child_fx_id(container_fx, i) else {
            continue;
        };
        let child_path = format!("{}:{}", parent_path, i);

        if child_raw == target_raw {
            let child_fx = chain.fx_by_index_untracked(child_raw);
            if is_container_fx(&child_fx) {
                return Some(FxNodeId::container(child_path));
            } else {
                let guid = reaper_high::get_fx_guid(chain, child_raw)
                    .map(|g| g.to_string_without_braces())?;
                return Some(FxNodeId::from_guid(guid));
            }
        }

        // If this child is a container, recurse
        let child_fx = chain.fx_by_index_untracked(child_raw);
        if is_container_fx(&child_fx) {
            if let Some(id) = search_children_for_raw(chain, &child_fx, target_raw, &child_path) {
                return Some(id);
            }
        }
    }
    None
}

/// Convenience: resolve an FxNodeId to a TrackFxLocation.
fn resolve_node_to_location(
    chain: &FxChain,
    node_id: &FxNodeId,
    is_input: bool,
) -> Option<TrackFxLocation> {
    let raw = resolve_node_to_raw_index(chain, node_id)?;
    Some(fx_location(raw, is_input))
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
                warn!("get_fx_list: project not found ({:?})", project);
                return vec![];
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &context) else {
                warn!("get_fx_list: FX chain not found ({:?})", context);
                return vec![];
            };

            let chain_ref = &chain;
            chain
                .fxs()
                .map(|fx| build_fx_info(&fx, Some(chain_ref)))
                .collect()
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
            // Use untracked: container children have encoded indices (0x2000000+)
            let fx = chain.fx_by_index_untracked(index);
            Some(build_fx_info(&fx, Some(&chain)))
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
    ) -> Result<(), String> {
        debug!("ReaperFx::set_fx_enabled({:?}, {})", target, enabled);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| "project not found".to_string())?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)
                .ok_or_else(|| format!("FX chain not found for {:?}", target.context))?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("FX not found for {:?}", target.fx))?;
            let fx = chain.fx_by_index_untracked(index);
            if enabled {
                fx.enable().map_err(|e| format!("enable failed: {}", e))?;
            } else {
                fx.disable().map_err(|e| format!("disable failed: {}", e))?;
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn set_fx_offline(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        offline: bool,
    ) -> Result<(), String> {
        debug!("ReaperFx::set_fx_offline({:?}, {})", target, offline);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| "project not found".to_string())?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)
                .ok_or_else(|| format!("FX chain not found for {:?}", target.context))?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("FX not found for {:?}", target.fx))?;
            let fx = chain.fx_by_index_untracked(index);
            fx.set_online(!offline)
                .map_err(|e| format!("set_online failed: {}", e))?;
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
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

    async fn remove_fx(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Result<(), String> {
        debug!("ReaperFx::remove_fx({:?})", target);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| "project not found".to_string())?;
            let (track, chain) = resolve_fx_chain(&proj, &target.context)
                .ok_or_else(|| format!("FX chain not found for {:?}", target.context))?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("FX not found for {:?}", target.fx))?;
            let raw_track = track
                .raw()
                .map_err(|e| format!("raw track failed: {}", e))?;
            let location = fx_location(index, chain.is_input_fx());
            unsafe {
                Reaper::get()
                    .medium_reaper()
                    .track_fx_delete(raw_track, location)
                    .map_err(|e| format!("track_fx_delete failed: {}", e))?;
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn move_fx(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        new_index: u32,
    ) -> Result<(), String> {
        debug!("ReaperFx::move_fx({:?}, {})", target, new_index);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| "project not found".to_string())?;
            let (track, chain) = resolve_fx_chain(&proj, &target.context)
                .ok_or_else(|| format!("FX chain not found for {:?}", target.context))?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("FX not found for {:?}", target.fx))?;
            let raw_track = track
                .raw()
                .map_err(|e| format!("raw track failed: {}", e))?;
            let is_input = chain.is_input_fx();
            unsafe {
                Reaper::get().medium_reaper().track_fx_copy_to_track(
                    (raw_track, fx_location(index, is_input)),
                    (raw_track, fx_location(new_index, is_input)),
                    TransferBehavior::Move,
                );
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
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
            let fx = chain.fx_by_index_untracked(index);

            let params: Vec<FxParameter> = fx
                .parameters()
                .map(|param| build_fx_parameter(&param))
                .collect();
            Some(params)
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
            let fx = chain.fx_by_index_untracked(fx_idx);

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
    ) -> Result<(), String> {
        debug!(
            "ReaperFx::set_parameter(fx={:?}, idx={}, val={:.4})",
            request.target.fx, request.index, request.value
        );

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        let result = ts
            .main_thread_future(move || {
                let proj = resolve_project(&project).ok_or_else(|| {
                    warn!("set_parameter: project not found");
                    "project not found".to_string()
                })?;
                let (_track, chain) =
                    resolve_fx_chain(&proj, &request.target.context).ok_or_else(|| {
                        warn!(
                            "set_parameter: FX chain not found for {:?}",
                            request.target.context
                        );
                        format!("FX chain not found for {:?}", request.target.context)
                    })?;
                let fx_idx = resolve_fx_index(&chain, &request.target.fx).ok_or_else(|| {
                    warn!("set_parameter: FX not found for {:?}", request.target.fx);
                    format!("FX not found for {:?}", request.target.fx)
                })?;
                // Use reaper-high FxParameter directly — handles track/location
                // resolution internally via track_and_location()
                let fx = chain.fx_by_index(fx_idx).ok_or_else(|| {
                    warn!("set_parameter: fx_by_index({}) returned None", fx_idx);
                    format!("fx_by_index({}) returned None", fx_idx)
                })?;
                let param = fx.parameter_by_index(request.index);
                let norm_val = reaper_medium::ReaperNormalizedFxParamValue::new(request.value);

                param.set_reaper_normalized_value(norm_val).map_err(|e| {
                    warn!("set_parameter: set_reaper_normalized_value failed: {e}");
                    format!("set_reaper_normalized_value failed: {e}")
                })?;
                // Note: CLAP plugins apply parameter changes asynchronously
                // (via the next process() or flush() cycle), so immediate
                // read-back may not reflect the new value yet. Trust the
                // return value from set_reaper_normalized_value().
                Ok(())
            })
            .await
            .unwrap_or_else(|_| Err("main thread future cancelled".into()));

        if let Err(ref e) = result {
            warn!("set_parameter failed: {}", e);
        }
        result
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
            let fx = chain.fx_by_index_untracked(fx_idx);

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
    ) -> Result<(), String> {
        debug!(
            "ReaperFx::set_parameter_by_name({:?}, {:?}, {})",
            request.target, request.name, request.value
        );

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| {
                format!("set_parameter_by_name: project not found ({:?})", project)
            })?;
            let (_track, chain) =
                resolve_fx_chain(&proj, &request.target.context).ok_or_else(|| {
                    format!(
                        "set_parameter_by_name: FX chain not found ({:?})",
                        request.target.context
                    )
                })?;
            let fx_idx = resolve_fx_index(&chain, &request.target.fx).ok_or_else(|| {
                format!(
                    "set_parameter_by_name: FX not found ({:?})",
                    request.target.fx
                )
            })?;
            let fx = chain.fx_by_index_untracked(fx_idx);
            for param in fx.parameters() {
                if let Ok(pname) = param.name() {
                    if pname.to_str() == request.name {
                        let value = reaper_medium::ReaperNormalizedFxParamValue::new(request.value);
                        let _ = param.set_reaper_normalized_value(value);
                        return Ok(());
                    }
                }
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    // =========================================================================
    // Presets
    // =========================================================================

    async fn get_preset_index(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Option<FxPresetIndex> {
        debug!("ReaperFx::get_preset_index({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)?;
            let index = resolve_fx_index(&chain, &target.fx)?;
            let fx = chain.fx_by_index_untracked(index);

            // Get preset index and count via reaper-high
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                fx.preset_index_and_count()
            }))
            .ok()?;

            // Get preset name via reaper-high
            let name = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                fx.preset_name()
                    .map(|rs| rs.to_str().to_string())
                    .filter(|s| !s.is_empty())
            }))
            .unwrap_or(None);

            Some(FxPresetIndex {
                index: result.index,
                count: result.count,
                name,
            })
        })
        .await
        .unwrap_or(None)
    }

    async fn next_preset(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Result<(), String> {
        debug!("ReaperFx::next_preset({:?})", target);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| format!("next_preset: project not found ({:?})", project))?;
            let (track, chain) = resolve_fx_chain(&proj, &target.context)
                .ok_or_else(|| format!("next_preset: FX chain not found ({:?})", target.context))?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("next_preset: FX not found ({:?})", target.fx))?;
            let raw_track = track
                .raw()
                .map_err(|_| "next_preset: raw track not available".to_string())?;
            let location = fx_location(index, chain.is_input_fx());
            unsafe {
                let _ = Reaper::get()
                    .medium_reaper()
                    .track_fx_navigate_presets(raw_track, location, 1);
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn prev_preset(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Result<(), String> {
        debug!("ReaperFx::prev_preset({:?})", target);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| format!("prev_preset: project not found ({:?})", project))?;
            let (track, chain) = resolve_fx_chain(&proj, &target.context)
                .ok_or_else(|| format!("prev_preset: FX chain not found ({:?})", target.context))?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("prev_preset: FX not found ({:?})", target.fx))?;
            let raw_track = track
                .raw()
                .map_err(|_| "prev_preset: raw track not available".to_string())?;
            let location = fx_location(index, chain.is_input_fx());
            unsafe {
                let _ = Reaper::get()
                    .medium_reaper()
                    .track_fx_navigate_presets(raw_track, location, -1);
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn set_preset(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        index: u32,
    ) -> Result<(), String> {
        debug!("ReaperFx::set_preset({:?}, {})", target, index);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| format!("set_preset: project not found ({:?})", project))?;
            let (track, chain) = resolve_fx_chain(&proj, &target.context)
                .ok_or_else(|| format!("set_preset: FX chain not found ({:?})", target.context))?;
            let fx_idx = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("set_preset: FX not found ({:?})", target.fx))?;
            let raw_track = track
                .raw()
                .map_err(|e| format!("set_preset: raw track failed: {}", e))?;
            let location = fx_location(fx_idx, chain.is_input_fx());
            unsafe {
                let _ = Reaper::get().medium_reaper().track_fx_set_preset_by_index(
                    raw_track,
                    location,
                    FxPresetRef::Preset(index),
                );
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    // =========================================================================
    // UI
    // =========================================================================

    async fn open_fx_ui(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Result<(), String> {
        debug!("ReaperFx::open_fx_ui({:?})", target);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| format!("open_fx_ui: project not found ({:?})", project))?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)
                .ok_or_else(|| format!("open_fx_ui: FX chain not found ({:?})", target.context))?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("open_fx_ui: FX not found ({:?})", target.fx))?;
            let fx = chain.fx_by_index_untracked(index);
            let _ = fx.show_in_floating_window();
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn close_fx_ui(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Result<(), String> {
        debug!("ReaperFx::close_fx_ui({:?})", target);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| format!("close_fx_ui: project not found ({:?})", project))?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)
                .ok_or_else(|| format!("close_fx_ui: FX chain not found ({:?})", target.context))?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("close_fx_ui: FX not found ({:?})", target.fx))?;
            let fx = chain.fx_by_index_untracked(index);
            let _ = fx.hide_floating_window();
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn toggle_fx_ui(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Result<(), String> {
        debug!("ReaperFx::toggle_fx_ui({:?})", target);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| format!("toggle_fx_ui: project not found ({:?})", project))?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context).ok_or_else(|| {
                format!("toggle_fx_ui: FX chain not found ({:?})", target.context)
            })?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("toggle_fx_ui: FX not found ({:?})", target.fx))?;
            let fx = chain.fx_by_index_untracked(index);
            if fx.window_is_open() {
                let _ = fx.hide_floating_window();
            } else {
                let _ = fx.show_in_floating_window();
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
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
            let fx = chain.fx_by_index_untracked(index);
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
    ) -> Result<(), String> {
        debug!(
            "ReaperFx::set_named_config({:?}, {:?})",
            request.target, request.key
        );

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| format!("set_named_config: project not found ({:?})", project))?;
            let (_track, chain) =
                resolve_fx_chain(&proj, &request.target.context).ok_or_else(|| {
                    format!(
                        "set_named_config: FX chain not found ({:?})",
                        request.target.context
                    )
                })?;
            let index = resolve_fx_index(&chain, &request.target.fx).ok_or_else(|| {
                format!("set_named_config: FX not found ({:?})", request.target.fx)
            })?;
            let fx = chain.fx_by_index_untracked(index);
            let c_string = std::ffi::CString::new(request.value)
                .map_err(|e| format!("set_named_config: invalid value: {}", e))?;
            unsafe {
                let _ = fx.set_named_config_param(&*request.key, c_string.as_ptr());
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn get_fx_latency(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Option<FxLatency> {
        debug!("ReaperFx::get_fx_latency({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)?;
            let index = resolve_fx_index(&chain, &target.fx)?;

            let fx = chain.fx_by_index_untracked(index);
            let pdc_samples = read_config_i32(&fx, "pdc");
            let chain_pdc_actual = read_config_i32(&fx, "chain_pdc_actual");
            let chain_pdc_reporting = read_config_i32(&fx, "chain_pdc_reporting");

            Some(FxLatency {
                pdc_samples,
                chain_pdc_actual,
                chain_pdc_reporting,
            })
        })
        .await
        .unwrap_or(None)
    }

    async fn get_param_modulation(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        param_index: u32,
    ) -> Option<FxParamModulation> {
        debug!(
            "ReaperFx::get_param_modulation({:?}, {})",
            target, param_index
        );

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)?;
            let index = resolve_fx_index(&chain, &target.fx)?;
            let fx = chain.fx_by_index_untracked(index);

            let p = param_index;
            let lfo_active =
                read_config_str(&fx, &format!("param.{p}.lfo.active")).map_or(false, |s| s == "1");
            let lfo_speed = read_config_f64(&fx, &format!("param.{p}.lfo.speed"));
            let lfo_strength = read_config_f64(&fx, &format!("param.{p}.lfo.strength"));

            let link_active = read_config_str(&fx, &format!("param.{p}.plink.active"))
                .map_or(false, |s| s == "1");
            let link_fx_index = read_config_i32(&fx, &format!("param.{p}.plink.effect"));
            let link_param_index = read_config_i32(&fx, &format!("param.{p}.plink.param"));

            Some(FxParamModulation {
                lfo_active,
                lfo_speed,
                lfo_strength,
                link_active,
                link_fx_index,
                link_param_index,
            })
        })
        .await
        .unwrap_or(None)
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
            let fx = chain.fx_by_index_untracked(index);
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
    ) -> Result<(), String> {
        debug!("ReaperFx::set_fx_state_chunk({:?})", target);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| "project not found".to_string())?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)
                .ok_or_else(|| format!("FX chain not found for {:?}", target.context))?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("FX not found for {:?}", target.fx))?;
            let fx = chain.fx_by_index_untracked(index);
            fx.set_vst_chunk(&chunk)
                .map_err(|e| format!("set_vst_chunk failed: {}", e))?;
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
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
            let fx = chain.fx_by_index_untracked(index);
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
    ) -> Result<(), String> {
        debug!("ReaperFx::set_fx_state_chunk_encoded({:?})", target);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| {
                format!(
                    "set_fx_state_chunk_encoded: project not found ({:?})",
                    project
                )
            })?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context).ok_or_else(|| {
                format!(
                    "set_fx_state_chunk_encoded: FX chain not found ({:?})",
                    target.context
                )
            })?;
            let index = resolve_fx_index(&chain, &target.fx).ok_or_else(|| {
                format!("set_fx_state_chunk_encoded: FX not found ({:?})", target.fx)
            })?;
            let fx = chain.fx_by_index_untracked(index);
            fx.set_vst_chunk_encoded(encoded)
                .map_err(|e| format!("set_vst_chunk_encoded failed: {}", e))?;
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
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

            let fx_count = chain.fx_count();
            debug!("get_fx_chain_state: chain has {} FX", fx_count);

            let mut chunks = Vec::new();
            for fx in chain.fxs() {
                let guid = fx
                    .get_or_query_guid()
                    .map(|g| g.to_string_without_braces())
                    .unwrap_or_default();
                let plugin_name = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    fx.name().to_str().to_string()
                }))
                .unwrap_or_else(|_| "(unknown)".to_string());
                let index = fx.index();

                // Get the base64-encoded VST chunk
                match fx.vst_chunk_encoded() {
                    Ok(encoded) => {
                        let chunk_len = encoded.to_str().len();
                        debug!(
                            "  FX[{}] '{}' (GUID {}) — chunk captured ({} bytes)",
                            index, plugin_name, guid, chunk_len
                        );
                        chunks.push(FxStateChunk {
                            fx_guid: guid,
                            fx_index: index,
                            plugin_name,
                            encoded_chunk: encoded.to_str().to_string(),
                        });
                    }
                    Err(e) => {
                        warn!(
                            "  FX[{}] '{}' (GUID {}) — vst_chunk_encoded FAILED: {}",
                            index, plugin_name, guid, e
                        );
                    }
                }
            }
            debug!(
                "get_fx_chain_state: captured {}/{} FX chunks",
                chunks.len(),
                fx_count
            );
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
    ) -> Result<(), String> {
        debug!(
            "ReaperFx::set_fx_chain_state({:?}, {} chunks)",
            context,
            chunks.len()
        );

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        // Apply all chunks in a single main-thread operation for atomicity
        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| format!("set_fx_chain_state: project not found ({:?})", project))?;
            let (_track, chain) = resolve_fx_chain(&proj, &context)
                .ok_or_else(|| format!("set_fx_chain_state: FX chain not found ({:?})", context))?;

            // Build a GUID → FX index lookup (like Snapshooter's hashmap approach)
            let mut guid_to_index = std::collections::HashMap::new();
            // Also build name → list of indices for cross-track fallback
            let mut name_to_indices: std::collections::HashMap<String, Vec<u32>> =
                std::collections::HashMap::new();
            for fx in chain.fxs() {
                if let Ok(guid) = fx.get_or_query_guid() {
                    guid_to_index.insert(guid.to_string_without_braces(), fx.index());
                }
                let fx_name = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    fx.name().to_str().to_string()
                }))
                .unwrap_or_default();
                name_to_indices.entry(fx_name).or_default().push(fx.index());
            }
            // Track how many instances of each name have been consumed (for duplicate plugins)
            let mut name_consumed: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();

            // Apply each chunk: try GUID match first, then fall back to plugin name
            for chunk in &chunks {
                if let Some(&fx_index) = guid_to_index.get(&chunk.fx_guid) {
                    // GUID match (same track recall)
                    if let Some(fx) = chain.fx_by_index(fx_index) {
                        let _ = fx.set_vst_chunk_encoded(chunk.encoded_chunk.clone());
                    }
                } else {
                    // Cross-track fallback: match by plugin name (positional for duplicates)
                    let consumed = name_consumed.entry(chunk.plugin_name.clone()).or_insert(0);
                    if let Some(indices) = name_to_indices.get(&chunk.plugin_name) {
                        if let Some(&fx_index) = indices.get(*consumed) {
                            info!(
                                "Cross-track match: '{}' GUID {} → index {} (by name, pos {})",
                                chunk.plugin_name, chunk.fx_guid, fx_index, consumed
                            );
                            if let Some(fx) = chain.fx_by_index(fx_index) {
                                let _ = fx.set_vst_chunk_encoded(chunk.encoded_chunk.clone());
                            }
                            *consumed += 1;
                        } else {
                            warn!(
                                "FX '{}' has no more unmatched instances (pos {})",
                                chunk.plugin_name, consumed
                            );
                        }
                    } else {
                        warn!(
                            "FX '{}' (GUID {}) not found by name either",
                            chunk.plugin_name, chunk.fx_guid
                        );
                    }
                }
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    // =========================================================================
    // Container / Tree Operations
    //
    // Stub implementations — real container traversal and mutation will be
    // implemented in US-004 and US-005.
    // =========================================================================

    async fn get_fx_tree(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
    ) -> FxTree {
        debug!("ReaperFx::get_fx_tree({:?})", context);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return FxTree::new();
        };

        ts.main_thread_future(move || {
            let Some(proj) = resolve_project(&project) else {
                warn!("get_fx_tree: project not found ({:?})", project);
                return FxTree::new();
            };
            let Some((_track, chain)) = resolve_fx_chain(&proj, &context) else {
                warn!("get_fx_tree: FX chain not found ({:?})", context);
                return FxTree::new();
            };

            let is_input = matches!(
                context,
                FxChainContext::Input(_) | FxChainContext::Monitoring
            );
            let top_level_count = chain.fx_count();

            if top_level_count == 0 {
                return FxTree::new();
            }

            let nodes = build_fx_tree_from_chain(&chain, is_input, top_level_count);

            FxTree::from_nodes(nodes)
        })
        .await
        .unwrap_or_default()
    }

    async fn create_container(
        &self,
        _cx: &Context,
        project: ProjectContext,
        request: CreateContainerRequest,
    ) -> Option<FxNodeId> {
        debug!("ReaperFx::create_container({:?})", request.name);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &request.context)?;
            let is_input = chain.is_input_fx();
            let track = chain.track()?;
            let raw_track = track.raw().ok()?;

            // Add container at end of chain, then move to requested position
            let container_fx = chain.insert_fx_by_name(chain.fx_count(), "Container")?;
            let container_index = container_fx.index();

            // Move to requested position if not already there
            if container_index != request.index && request.index < chain.fx_count() {
                unsafe {
                    Reaper::get().medium_reaper().track_fx_copy_to_track(
                        (raw_track, fx_location(container_index, is_input)),
                        (raw_track, fx_location(request.index, is_input)),
                        TransferBehavior::Move,
                    );
                }
            }

            // Set the container name
            let final_index =
                if container_index != request.index && request.index < chain.fx_count() {
                    request.index
                } else {
                    container_index
                };
            let container_fx = chain.fx_by_index_untracked(final_index);
            if let Ok(c_name) = std::ffi::CString::new(request.name) {
                unsafe {
                    let _ = container_fx.set_named_config_param("renamed_name", c_name.as_ptr());
                }
            }

            // Return the new container's node ID
            Some(FxNodeId::container(format!("{}", final_index)))
        })
        .await
        .unwrap_or(None)
    }

    async fn move_to_container(
        &self,
        _cx: &Context,
        project: ProjectContext,
        request: MoveToContainerRequest,
    ) -> Result<(), String> {
        debug!(
            "ReaperFx::move_to_container({:?} -> {:?})",
            request.node_id, request.container_id
        );

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| "project not found".to_string())?;
            let (_track, chain) = resolve_fx_chain(&proj, &request.context)
                .ok_or_else(|| format!("FX chain not found for {:?}", request.context))?;
            let is_input = chain.is_input_fx();
            let track = chain.track().ok_or_else(|| "track not found".to_string())?;
            let raw_track = track
                .raw()
                .map_err(|e| format!("raw track failed: {}", e))?;

            // Resolve source FX raw index
            let source_raw = resolve_node_to_raw_index(&chain, &request.node_id)
                .ok_or_else(|| format!("could not resolve source node {:?}", request.node_id))?;

            // Resolve destination container raw index
            let container_raw = resolve_node_to_raw_index(&chain, &request.container_id)
                .ok_or_else(|| format!("could not resolve container {:?}", request.container_id))?;

            // Two-step move pattern (from MPL's Multi-Mono Container):
            //
            // Instead of computing stride-based destination addresses (which are
            // error-prone for non-empty containers), we:
            //
            // 1. Move the FX to the first child slot (container_item.0 or stride
            //    formula for empty containers). This always creates a new child.
            // 2. If the desired position isn't 0, shuffle within the container
            //    using container_item.X addresses (which are now valid because
            //    the child exists).
            let container_fx = chain.fx_by_index_untracked(container_raw);
            let child_count = read_config_u32(&container_fx, "container_count");
            let child_pos = request.child_index.min(child_count);

            // Step 1: Move into container at the first child slot.
            let first_slot = if child_count > 0 {
                // Non-empty container: target the existing first child's address.
                // CopyToTrack with Move inserts before this position.
                container_child_fx_id(&container_fx, 0)
                    .ok_or_else(|| "container_item.0 not found".to_string())?
            } else {
                // Empty container: use stride formula (only case where we need it).
                let top_count = chain.fx_count();
                let stride = top_count + 1;
                CONTAINER_BASE + container_raw + stride
            };

            unsafe {
                Reaper::get().medium_reaper().track_fx_copy_to_track(
                    (raw_track, fx_location(source_raw, is_input)),
                    (raw_track, fx_location(first_slot, is_input)),
                    TransferBehavior::Move,
                );
            }

            // Verify the move succeeded (child count should have increased).
            verify_container_child_count(&chain, container_raw, child_count + 1)?;

            // Step 2: If the desired position isn't 0, shuffle the newly-inserted
            // child (which is at position 0) to the target position by moving it
            // to just after the element currently at (child_pos - 1).
            if child_pos > 0 {
                // Re-read container state after the move.
                let container_fx = chain.fx_by_index_untracked(container_raw);
                let new_count = read_config_u32(&container_fx, "container_count");

                // The FX we just moved in is now at position 0.
                let src = container_child_fx_id(&container_fx, 0)
                    .ok_or_else(|| "container_item.0 not found after move".to_string())?;

                // Target: the address of the element at child_pos (which was shifted
                // up by 1 due to our insertion at 0). After moving from 0 to this
                // position, REAPER places it at that slot.
                let target_pos = child_pos.min(new_count - 1);
                let dest = container_child_fx_id(&container_fx, target_pos)
                    .ok_or_else(|| format!("container_item.{} not found", target_pos))?;

                unsafe {
                    Reaper::get().medium_reaper().track_fx_copy_to_track(
                        (raw_track, fx_location(src, is_input)),
                        (raw_track, fx_location(dest, is_input)),
                        TransferBehavior::Move,
                    );
                }

                // Verify child count is unchanged after the shuffle.
                verify_container_child_count(&chain, container_raw, child_count + 1)?;
            }

            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn move_from_container(
        &self,
        _cx: &Context,
        project: ProjectContext,
        request: MoveFromContainerRequest,
    ) -> Result<(), String> {
        debug!(
            "ReaperFx::move_from_container({:?} -> index {})",
            request.node_id, request.target_index
        );

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| "project not found".to_string())?;
            let (_track, chain) = resolve_fx_chain(&proj, &request.context)
                .ok_or_else(|| format!("FX chain not found for {:?}", request.context))?;
            let is_input = chain.is_input_fx();
            let track = chain.track().ok_or_else(|| "track not found".to_string())?;
            let raw_track = track
                .raw()
                .map_err(|e| format!("raw track failed: {}", e))?;

            // Resolve source FX (inside container)
            let source_raw = resolve_node_to_raw_index(&chain, &request.node_id)
                .ok_or_else(|| format!("could not resolve node {:?}", request.node_id))?;

            // Capture top-level FX count before the move for verification
            let top_count_before = chain.fx_count();

            // Move to top-level position
            unsafe {
                Reaper::get().medium_reaper().track_fx_copy_to_track(
                    (raw_track, fx_location(source_raw, is_input)),
                    (raw_track, fx_location(request.target_index, is_input)),
                    TransferBehavior::Move,
                );
            }

            // Verify: top-level count should have increased by 1 (child moved out)
            let top_count_after = chain.fx_count();
            if top_count_after != top_count_before + 1 {
                return Err(format!(
                    "move_from_container: top-level FX count {} -> {} (expected {})",
                    top_count_before,
                    top_count_after,
                    top_count_before + 1
                ));
            }

            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn set_routing_mode(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
        node_id: FxNodeId,
        mode: FxRoutingMode,
    ) -> Result<(), String> {
        debug!("ReaperFx::set_routing_mode({:?}, {:?})", node_id, mode);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| format!("set_routing_mode: project not found ({:?})", project))?;
            let (_track, chain) = resolve_fx_chain(&proj, &context)
                .ok_or_else(|| format!("set_routing_mode: FX chain not found ({:?})", context))?;
            let raw_index = resolve_node_to_raw_index(&chain, &node_id).ok_or_else(|| {
                format!(
                    "set_routing_mode: could not resolve container ({:?})",
                    node_id
                )
            })?;
            let container_fx = chain.fx_by_index_untracked(raw_index);
            let value = mode.to_reaper_param();
            let c_value = std::ffi::CString::new(value)
                .map_err(|e| format!("set_routing_mode: invalid value: {}", e))?;
            unsafe {
                let _ = container_fx.set_named_config_param("parallel", c_value.as_ptr());
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn get_container_channel_config(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
        container_id: FxNodeId,
    ) -> Option<FxContainerChannelConfig> {
        debug!("ReaperFx::get_container_channel_config({:?})", container_id);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &context)?;

            let raw_index = resolve_node_to_raw_index(&chain, &container_id)?;
            let container_fx = chain.fx_by_index_untracked(raw_index);

            Some(FxContainerChannelConfig {
                nch: read_config_u32(&container_fx, "container_nch"),
                nch_in: read_config_u32(&container_fx, "container_nch_in"),
                nch_out: read_config_u32(&container_fx, "container_nch_out"),
            })
        })
        .await
        .unwrap_or(None)
    }

    async fn set_container_channel_config(
        &self,
        _cx: &Context,
        project: ProjectContext,
        request: SetContainerChannelConfigRequest,
    ) -> Result<(), String> {
        debug!(
            "ReaperFx::set_container_channel_config({:?})",
            request.container_id
        );

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| {
                format!(
                    "set_container_channel_config: project not found ({:?})",
                    project
                )
            })?;
            let (_track, chain) = resolve_fx_chain(&proj, &request.context).ok_or_else(|| {
                format!(
                    "set_container_channel_config: FX chain not found ({:?})",
                    request.context
                )
            })?;
            let raw_index =
                resolve_node_to_raw_index(&chain, &request.container_id).ok_or_else(|| {
                    format!(
                        "set_container_channel_config: could not resolve container ({:?})",
                        request.container_id
                    )
                })?;
            let container_fx = chain.fx_by_index_untracked(raw_index);

            // Set each channel config parameter
            let params = [
                ("container_nch", request.config.nch),
                ("container_nch_in", request.config.nch_in),
                ("container_nch_out", request.config.nch_out),
            ];

            for (key, value) in &params {
                let c_value = std::ffi::CString::new(value.to_string())
                    .map_err(|e| format!("set_container_channel_config: invalid value: {}", e))?;
                unsafe {
                    let _ = container_fx.set_named_config_param(*key, c_value.as_ptr());
                }
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    // =========================================================================
    // FX Channel Configuration (per-FX, non-container)
    // =========================================================================

    async fn get_fx_channel_config(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Option<FxChannelConfig> {
        debug!("ReaperFx::get_fx_channel_config({:?})", target);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context)?;
            let index = resolve_fx_index(&chain, &target.fx)?;
            let fx = chain.fx_by_index_untracked(index);

            // channel_config returns "count mode flags" as space-separated values.
            // If the param isn't available (e.g., container FX), return default zeros.
            let config = match read_config_str(&fx, "channel_config") {
                Some(raw) if !raw.is_empty() => {
                    let parts: Vec<&str> = raw.split_whitespace().collect();
                    FxChannelConfig {
                        channel_count: parts.first().and_then(|s| s.parse().ok()).unwrap_or(0),
                        channel_mode: parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
                        supported_flags: parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
                    }
                }
                _ => FxChannelConfig::default(),
            };

            Some(config)
        })
        .await
        .unwrap_or(None)
    }

    async fn set_fx_channel_config(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        config: FxChannelConfig,
    ) -> Result<(), String> {
        debug!(
            "ReaperFx::set_fx_channel_config({:?}, count={}, mode={})",
            target, config.channel_count, config.channel_mode
        );

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| {
                format!("set_fx_channel_config: project not found ({:?})", project)
            })?;
            let (_track, chain) = resolve_fx_chain(&proj, &target.context).ok_or_else(|| {
                format!(
                    "set_fx_channel_config: FX chain not found ({:?})",
                    target.context
                )
            })?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("set_fx_channel_config: FX not found ({:?})", target.fx))?;
            let fx = chain.fx_by_index_untracked(index);

            // Write "count mode" — REAPER accepts 1 or 2 values
            let value = format!("{} {}", config.channel_count, config.channel_mode);
            let c_value = std::ffi::CString::new(value)
                .map_err(|e| format!("set_fx_channel_config: invalid value: {}", e))?;
            unsafe {
                let _ = fx.set_named_config_param("channel_config", c_value.as_ptr());
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn silence_fx_output(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
    ) -> Result<FxPinMappings, String> {
        debug!("ReaperFx::silence_fx_output({:?})", target);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| format!("silence_fx_output: project not found ({:?})", project))?;
            let (track, chain) = resolve_fx_chain(&proj, &target.context).ok_or_else(|| {
                format!(
                    "silence_fx_output: FX chain not found ({:?})",
                    target.context
                )
            })?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("silence_fx_output: FX not found ({:?})", target.fx))?;

            let raw_track = track
                .raw()
                .map_err(|e| format!("silence_fx_output: raw track failed: {}", e))?;
            let is_input = chain.is_input_fx();
            let raw_fx = fx_location(index, is_input).to_raw();
            let low = Reaper::get().medium_reaper().low();

            // Read and save current output pin mappings, then zero them.
            // We scan pins 0..MAX_PINS for both the first and second 64-bit banks.
            const MAX_PINS: i32 = 64;
            const SECOND_BANK: i32 = 0x1000000;
            let mut saved = Vec::new();

            unsafe {
                // Scan first bank
                for pin in 0..MAX_PINS {
                    let mut high32: i32 = 0;
                    let low32 = low.TrackFX_GetPinMappings(
                        raw_track.as_ptr(),
                        raw_fx,
                        1, // isoutput
                        pin,
                        &mut high32,
                    );
                    if low32 != 0 || high32 != 0 {
                        saved.push((pin, low32, high32));
                        // Zero this pin
                        low.TrackFX_SetPinMappings(
                            raw_track.as_ptr(),
                            raw_fx,
                            1, // isoutput
                            pin,
                            0,
                            0,
                        );
                    }
                }
                // Scan second bank
                for pin in 0..MAX_PINS {
                    let mut high32: i32 = 0;
                    let low32 = low.TrackFX_GetPinMappings(
                        raw_track.as_ptr(),
                        raw_fx,
                        1, // isoutput
                        pin + SECOND_BANK,
                        &mut high32,
                    );
                    if low32 != 0 || high32 != 0 {
                        saved.push((pin + SECOND_BANK, low32, high32));
                        low.TrackFX_SetPinMappings(
                            raw_track.as_ptr(),
                            raw_fx,
                            1, // isoutput
                            pin + SECOND_BANK,
                            0,
                            0,
                        );
                    }
                }
            }

            Ok(FxPinMappings { output_pins: saved })
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn restore_fx_output(
        &self,
        _cx: &Context,
        project: ProjectContext,
        target: FxTarget,
        saved: FxPinMappings,
    ) -> Result<(), String> {
        debug!("ReaperFx::restore_fx_output({:?})", target);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| format!("restore_fx_output: project not found ({:?})", project))?;
            let (track, chain) = resolve_fx_chain(&proj, &target.context).ok_or_else(|| {
                format!(
                    "restore_fx_output: FX chain not found ({:?})",
                    target.context
                )
            })?;
            let index = resolve_fx_index(&chain, &target.fx)
                .ok_or_else(|| format!("restore_fx_output: FX not found ({:?})", target.fx))?;

            let raw_track = track
                .raw()
                .map_err(|e| format!("restore_fx_output: raw track failed: {}", e))?;
            let is_input = chain.is_input_fx();
            let raw_fx = fx_location(index, is_input).to_raw();
            let low = Reaper::get().medium_reaper().low();

            unsafe {
                if saved.output_pins.is_empty() {
                    // No saved mappings — restore default stereo pass-through.
                    // Pin 0 → channel 0 (bit 0 = 0x1), Pin 1 → channel 1 (bit 1 = 0x2).
                    low.TrackFX_SetPinMappings(
                        raw_track.as_ptr(),
                        raw_fx,
                        1, // isoutput
                        0, // pin 0
                        1, // low32: bit 0
                        0,
                    );
                    low.TrackFX_SetPinMappings(
                        raw_track.as_ptr(),
                        raw_fx,
                        1, // isoutput
                        1, // pin 1
                        2, // low32: bit 1
                        0,
                    );
                } else {
                    for &(pin, low32, high32) in &saved.output_pins {
                        low.TrackFX_SetPinMappings(
                            raw_track.as_ptr(),
                            raw_fx,
                            1, // isoutput
                            pin,
                            low32,
                            high32,
                        );
                    }
                }
            }

            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn enclose_in_container(
        &self,
        _cx: &Context,
        project: ProjectContext,
        request: EncloseInContainerRequest,
    ) -> Option<FxNodeId> {
        debug!(
            "ReaperFx::enclose_in_container({:?}, {:?})",
            request.node_ids, request.name
        );

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (_track, chain) = resolve_fx_chain(&proj, &request.context)?;
            let track = chain.track()?;

            if request.node_ids.is_empty() {
                return None;
            }

            // ── Chunk-based approach ────────────────────────────────────
            // Instead of using REAPER's live API with fragile stride-based
            // addressing, we:
            //   1. Get the full track chunk as RPP text
            //   2. Parse the FXCHAIN block with dawfile-reaper
            //   3. Rearrange nodes in memory (remove selected, wrap in container)
            //   4. Serialize back and set the chunk atomically

            // Step 1: Get track chunk and extract FXCHAIN text
            let chunk = track
                .chunk(MAX_TRACK_CHUNK_SIZE, ChunkCacheHint::NormalMode)
                .ok()?;
            let chunk_str = chunk.to_string();

            // Find the FXCHAIN block boundaries in the raw text
            let fxchain_start = chunk_str.find("<FXCHAIN")?;
            let fxchain_region = &chunk_str[fxchain_start..];
            let fxchain_end = find_block_end(fxchain_region)? + fxchain_start;
            let fxchain_text = &chunk_str[fxchain_start..=fxchain_end];

            // Step 2: Parse FX chain into structured types
            let mut parsed_chain = dawfile_reaper::types::FxChain::parse(fxchain_text)
                .map_err(|e| warn!("Failed to parse FXCHAIN chunk: {}", e))
                .ok()?;

            info!(
                "enclose_in_container: parsed {} top-level nodes",
                parsed_chain.nodes.len()
            );

            // Step 3: Collect the GUIDs we want to enclose
            let target_guids: std::collections::HashSet<String> = request
                .node_ids
                .iter()
                .filter(|id| !id.is_container())
                .map(|id| id.as_str().to_string())
                .collect();

            // Find matching node indices (by FXID GUID)
            let mut matched_indices: Vec<usize> = Vec::new();
            for (i, node) in parsed_chain.nodes.iter().enumerate() {
                let fxid = match node {
                    dawfile_reaper::types::FxChainNode::Plugin(p) => p.fxid.as_deref(),
                    dawfile_reaper::types::FxChainNode::Container(c) => c.fxid.as_deref(),
                };
                if let Some(guid) = fxid {
                    // dawfile-reaper stores GUIDs with braces: {GUID}
                    // our FxNodeId stores them without braces
                    let guid_clean = guid
                        .trim_start_matches('{')
                        .trim_end_matches('}')
                        .to_lowercase();
                    if target_guids.iter().any(|t| t.to_lowercase() == guid_clean) {
                        matched_indices.push(i);
                    }
                }
            }

            if matched_indices.is_empty() {
                warn!(
                    "enclose_in_container: no nodes matched target GUIDs: {:?}",
                    target_guids
                );
                return None;
            }

            info!(
                "enclose_in_container: matched {} nodes at indices {:?}",
                matched_indices.len(),
                matched_indices
            );

            // Remove matched nodes from the chain (in reverse order to preserve indices)
            let insert_pos = matched_indices[0]; // container goes where first matched node was
            let mut removed_nodes: Vec<dawfile_reaper::types::FxChainNode> = Vec::new();
            for &idx in matched_indices.iter().rev() {
                removed_nodes.push(parsed_chain.nodes.remove(idx));
            }
            removed_nodes.reverse(); // restore original order

            // Build a new container wrapping the removed nodes
            let container = dawfile_reaper::types::FxContainer {
                name: request.name.clone(),
                bypassed: false,
                offline: false,
                fxid: None,
                float_pos: None,
                parallel: false,
                container_cfg: Some([2, 2, 2, 0]), // stereo default
                show: 0,
                last_sel: 0,
                docked: false,
                children: removed_nodes,
                raw_block: String::new(),
            };

            // Insert the container at the position of the first removed node
            let insert_at = insert_pos.min(parsed_chain.nodes.len());
            parsed_chain
                .nodes
                .insert(insert_at, dawfile_reaper::types::FxChainNode::Container(container));

            info!(
                "enclose_in_container: new chain has {} top-level nodes, container '{}' at index {}",
                parsed_chain.nodes.len(),
                request.name,
                insert_at
            );

            // Step 4: Serialize back to RPP text
            let new_fxchain_text = parsed_chain.to_rpp_string();

            // Replace the FXCHAIN block in the full track chunk
            let mut new_chunk_str = String::with_capacity(chunk_str.len());
            new_chunk_str.push_str(&chunk_str[..fxchain_start]);
            new_chunk_str.push_str(&new_fxchain_text);
            // Skip past the old FXCHAIN block (fxchain_end points to the closing >)
            if fxchain_end + 1 < chunk_str.len() {
                new_chunk_str.push_str(&chunk_str[fxchain_end + 1..]);
            }

            // Set the modified chunk back on the track
            let new_chunk = reaper_high::Chunk::new(new_chunk_str);
            track.set_chunk(new_chunk).ok()?;

            info!("enclose_in_container: chunk set successfully");

            Some(FxNodeId::container(format!("{}", insert_at)))
        })
        .await
        .unwrap_or(None)
    }

    async fn explode_container(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
        container_id: FxNodeId,
    ) -> Result<(), String> {
        debug!("ReaperFx::explode_container({:?})", container_id);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project).ok_or_else(|| "project not found".to_string())?;
            let (_track, chain) = resolve_fx_chain(&proj, &context)
                .ok_or_else(|| format!("FX chain not found for {:?}", context))?;
            let track = chain.track().ok_or_else(|| "track not found".to_string())?;

            // ── Chunk-based approach ────────────────────────────────────
            // Parse the FXCHAIN, find the container by GUID, replace it
            // with its children in-place, serialize back atomically.

            let chunk = track
                .chunk(MAX_TRACK_CHUNK_SIZE, ChunkCacheHint::NormalMode)
                .map_err(|e| format!("failed to get track chunk: {:?}", e))?;
            let chunk_str = chunk.to_string();

            let fxchain_start = chunk_str
                .find("<FXCHAIN")
                .ok_or_else(|| "no FXCHAIN block in track chunk".to_string())?;
            let fxchain_region = &chunk_str[fxchain_start..];
            let fxchain_end = find_block_end(fxchain_region)
                .ok_or_else(|| "could not find FXCHAIN closing tag".to_string())?
                + fxchain_start;
            let fxchain_text = &chunk_str[fxchain_start..=fxchain_end];

            let mut parsed_chain = dawfile_reaper::types::FxChain::parse(fxchain_text)
                .map_err(|e| format!("failed to parse FXCHAIN: {}", e))?;

            // Find the container to explode — match by FXID GUID
            let target_guid = if container_id.is_container() {
                // For container IDs like "container:2", resolve by index
                let path_str = container_id
                    .as_str()
                    .strip_prefix("container:")
                    .ok_or_else(|| "invalid container id".to_string())?;
                let idx: usize = path_str
                    .split(':')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| format!("invalid container path: {}", path_str))?;
                // Get the FXID from the node at that index
                parsed_chain.nodes.get(idx).and_then(|n| match n {
                    dawfile_reaper::types::FxChainNode::Container(c) => c.fxid.clone(),
                    _ => None,
                })
            } else {
                Some(container_id.as_str().to_string())
            };

            // Find the container index in the parsed chain
            let container_idx = parsed_chain
                .nodes
                .iter()
                .position(|node| {
                    if let dawfile_reaper::types::FxChainNode::Container(c) = node {
                        if let Some(ref target) = target_guid {
                            if let Some(ref fxid) = c.fxid {
                                let fxid_clean = fxid
                                    .trim_start_matches('{')
                                    .trim_end_matches('}')
                                    .to_lowercase();
                                let target_clean = target
                                    .trim_start_matches('{')
                                    .trim_end_matches('}')
                                    .to_lowercase();
                                return fxid_clean == target_clean;
                            }
                        }
                        // Fallback: match by index position if no GUID match
                        false
                    } else {
                        false
                    }
                })
                .or_else(|| {
                    // Fallback: if container_id is "container:N", use N as index
                    if container_id.is_container() {
                        container_id
                            .as_str()
                            .strip_prefix("container:")
                            .and_then(|s| s.split(':').next())
                            .and_then(|s| s.parse::<usize>().ok())
                            .filter(|&idx| {
                                idx < parsed_chain.nodes.len()
                                    && matches!(
                                        parsed_chain.nodes[idx],
                                        dawfile_reaper::types::FxChainNode::Container(_)
                                    )
                            })
                    } else {
                        None
                    }
                })
                .ok_or_else(|| {
                    format!(
                        "could not find container {:?} in parsed FX chain",
                        container_id
                    )
                })?;

            // Remove the container and splice its children into the chain
            let container_node = parsed_chain.nodes.remove(container_idx);
            if let dawfile_reaper::types::FxChainNode::Container(c) = container_node {
                info!(
                    "explode_container: exploding '{}' with {} children at index {}",
                    c.name,
                    c.children.len(),
                    container_idx
                );
                // Insert children at the container's former position
                for (i, child) in c.children.into_iter().enumerate() {
                    parsed_chain.nodes.insert(container_idx + i, child);
                }
            }

            // Serialize and set chunk back
            let new_fxchain_text = parsed_chain.to_rpp_string();
            let mut new_chunk_str = String::with_capacity(chunk_str.len());
            new_chunk_str.push_str(&chunk_str[..fxchain_start]);
            new_chunk_str.push_str(&new_fxchain_text);
            if fxchain_end + 1 < chunk_str.len() {
                new_chunk_str.push_str(&chunk_str[fxchain_end + 1..]);
            }

            let new_chunk = reaper_high::Chunk::new(new_chunk_str);
            track
                .set_chunk(new_chunk)
                .map_err(|e| format!("failed to set track chunk: {:?}", e))?;

            info!("explode_container: chunk set successfully");
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    async fn rename_container(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
        container_id: FxNodeId,
        name: String,
    ) -> Result<(), String> {
        debug!("ReaperFx::rename_container({:?}, {:?})", container_id, name);

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| format!("rename_container: project not found ({:?})", project))?;
            let (_track, chain) = resolve_fx_chain(&proj, &context)
                .ok_or_else(|| format!("rename_container: FX chain not found ({:?})", context))?;
            let raw_index = resolve_node_to_raw_index(&chain, &container_id).ok_or_else(|| {
                format!(
                    "rename_container: could not resolve container ({:?})",
                    container_id
                )
            })?;
            let container_fx = chain.fx_by_index_untracked(raw_index);
            let c_name = std::ffi::CString::new(name)
                .map_err(|e| format!("rename_container: invalid name: {}", e))?;
            unsafe {
                let _ = container_fx.set_named_config_param("renamed_name", c_name.as_ptr());
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
    }

    // =========================================================================
    // Raw FX Chain Chunk Operations
    //
    // Operates on the track's raw RPP state text. Used for atomic module
    // preset save/load — capture a full <CONTAINER> block and splice it
    // into any track's FXCHAIN section in a single operation.
    // =========================================================================

    async fn get_fx_chain_chunk_text(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
    ) -> Option<String> {
        debug!("ReaperFx::get_fx_chain_chunk_text({:?})", context);

        let Some(ts) = task_support() else {
            warn!("TaskSupport not set");
            return None;
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)?;
            let (track, _chain) = resolve_fx_chain(&proj, &context)?;

            // Get full track state as RPP text.
            // NormalMode is required here — UndoMode returns cached data that
            // may be stale after structural changes like enclose_in_container.
            let chunk = track
                .chunk(MAX_TRACK_CHUNK_SIZE, ChunkCacheHint::NormalMode)
                .ok()?;
            let region = chunk.region();

            // Find the <FXCHAIN ...>...</FXCHAIN> block
            let fxchain_region = region.find_first_tag_named(0, "FXCHAIN")?;
            let content = fxchain_region.content().to_string();
            Some(content)
        })
        .await
        .unwrap_or(None)
    }

    async fn insert_fx_chain_chunk(
        &self,
        _cx: &Context,
        project: ProjectContext,
        context: FxChainContext,
        chunk_text: String,
    ) -> Result<(), String> {
        info!(
            "ReaperFx::insert_fx_chain_chunk({:?}, {} bytes)",
            context,
            chunk_text.len()
        );

        let Some(ts) = task_support() else {
            return Err("TaskSupport not set".into());
        };

        ts.main_thread_future(move || {
            let proj = resolve_project(&project)
                .ok_or_else(|| "insert_fx_chain_chunk: project not found".to_string())?;
            let (track, _chain) = resolve_fx_chain(&proj, &context)
                .ok_or_else(|| "insert_fx_chain_chunk: FX chain not found".to_string())?;

            // Get full track state as RPP text.
            // NormalMode ensures we see the current state (not undo cache).
            let mut chunk = track
                .chunk(MAX_TRACK_CHUNK_SIZE, ChunkCacheHint::NormalMode)
                .map_err(|e| {
                    format!("insert_fx_chain_chunk: failed to get track chunk: {:?}", e)
                })?;
            let region = chunk.region();

            // Find the <FXCHAIN> block
            if let Some(fxchain_region) = region.find_first_tag_named(0, "FXCHAIN") {
                // FXCHAIN exists — insert the new block before its closing `>` line
                let closing_line = fxchain_region.last_line();
                chunk.insert_before_region_as_block(&closing_line, &chunk_text);
            } else {
                // No FXCHAIN section (empty track). Create one wrapping the chunk text,
                // and insert it before the track's closing `>` line.
                let fxchain_block =
                    format!("<FXCHAIN\nSHOW 0\nLASTSEL 0\nDOCKED 0\n{}\n>", chunk_text);
                let track_closing = chunk.region().last_line();
                chunk.insert_before_region_as_block(&track_closing, &fxchain_block);
            }

            // Set the modified chunk back on the track
            track
                .set_chunk(chunk)
                .map_err(|e| format!("Failed to set track chunk: {:?}", e))?;
            info!("Successfully inserted FX chain chunk");
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("main thread future cancelled".into()))
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
        peeps::spawn_tracked!("reaper-fx-subscribe", async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        // Filter: only forward events for the subscribed chain
                        let event_context = match &event {
                            FxEvent::Added { context, .. }
                            | FxEvent::Removed { context, .. }
                            | FxEvent::EnabledChanged { context, .. }
                            | FxEvent::Moved { context, .. }
                            | FxEvent::ParameterChanged { context, .. }
                            | FxEvent::PresetChanged { context, .. }
                            | FxEvent::WindowChanged { context, .. }
                            | FxEvent::ContainerCreated { context, .. }
                            | FxEvent::ContainerRemoved { context, .. }
                            | FxEvent::RoutingModeChanged { context, .. }
                            | FxEvent::MovedToContainer { context, .. }
                            | FxEvent::ContainerRenamed { context, .. }
                            | FxEvent::TreeStructureChanged { context, .. } => context,
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

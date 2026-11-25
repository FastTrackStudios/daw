//! `impl Effects for Standalone` — backed by `ProjectState.fx_chains`.
//!
//! Standalone hosts real CLAP/VST3 plugins when the matching feature
//! is enabled and the FX name is a plugin bundle path. Non-path names
//! fall back to synthetic FX with 8 generic continuous parameters
//! (0..=1.0), which keeps tests and UI prototypes lightweight.
//!
//! Implemented surface:
//! - `list_installed` — returns a small canned list of "built-in" FX
//!   names; future audio-graph FX integration replaces this.
//! - Chain queries: `list` / `get` / `count`
//! - State: `set_enabled` / `set_offline`
//! - Management: `add` / `add_at` / `remove` / `move_to`
//! - Parameters: `parameters` / `parameter` / `parameter_by_name` /
//!   `set_parameter` / `set_parameter_by_name`
//! - UI: `open_ui` / `close_ui` / `toggle_ui` (mutates `window_open`)
//! - State chunks: `state_chunk` / `set_state_chunk` /
//!   `state_chunk_encoded` / `set_state_chunk_encoded` /
//!   `chain_state` / `set_chain_state`
//!
//! Still `todo!()`: containers / `tree` / latency / param modulation /
//! channel config / preset cycle / named-config / chain chunk text.
//! These need either real plugin metadata or a deeper modeling pass.

use daw_proto::fx::{
    AddFxAtRequest, CreateContainerRequest, EncloseInContainerRequest, Fx, FxChainContext,
    FxChannelConfig, FxContainerChannelConfig, FxLatency, FxNodeId, FxParamModulation, FxParameter,
    FxPinMappings, FxPresetIndex, FxRef, FxRoutingMode, FxStateChunk, FxTarget, FxTree, FxType,
    InstalledFx, LastTouchedFx, MoveFromContainerRequest, MoveToContainerRequest,
    SetContainerChannelConfigRequest, SetNamedConfigRequest, SetParameterByNameRequest,
    SetParameterRequest,
};
use daw_proto::project::ProjectContext;
use daw_proto::{DawError, DawResult, fx::Effects};
use uuid::Uuid;

use crate::sync::{FxChainKey, FxEntry, Standalone};

fn resolve_project(daw: &Standalone, ctx: &ProjectContext) -> Option<String> {
    match ctx {
        ProjectContext::Project(guid) => Some(guid.clone()),
        ProjectContext::Current => daw.current_project_guid(),
    }
}

fn not_found_proj() -> DawError {
    DawError::not_found("Project", "context")
}

/// Shared next/prev preset implementation. Looks up the current
/// stored index, applies `delta`, clamps non-negative.
fn bump_preset(
    daw: &Standalone,
    project: &ProjectContext,
    target: &FxTarget,
    delta: i32,
) -> DawResult<()> {
    let guid =
        resolve_project(daw, project).ok_or_else(|| DawError::not_found("project", "current"))?;
    let chain_key: FxChainKey = (&target.context).into();
    daw.with_project_mut(&guid, |p| {
        let chain = p.fx_chains.get(&chain_key).ok_or_else(not_found_fx)?;
        let i = find_fx_index(chain, &target.fx).ok_or_else(not_found_fx)?;
        let fx_guid = chain[i].fx.guid.clone();
        let current = p.fx_preset_index.get(&fx_guid).copied().unwrap_or(0) as i32;
        let next = (current + delta).max(0) as u32;
        p.fx_preset_index.insert(fx_guid, next);
        Ok::<(), DawError>(())
    })?
}

fn not_found_fx() -> DawError {
    DawError::not_found("Fx", "")
}

fn find_fx_index(chain: &[FxEntry], fx_ref: &FxRef) -> Option<usize> {
    match fx_ref {
        FxRef::Guid(g) => chain.iter().position(|e| e.fx.guid == *g),
        FxRef::Index(i) => {
            let u = *i as usize;
            (u < chain.len()).then_some(u)
        }
        FxRef::Name(name) => chain.iter().position(|e| e.fx.plugin_name == *name),
    }
}

fn resolve_fx_guid(
    daw: &Standalone,
    project: &ProjectContext,
    target: &FxTarget,
) -> Option<String> {
    let guid = resolve_project(daw, project)?;
    let key: FxChainKey = (&target.context).into();
    daw.with_project(&guid, |p| {
        let chain = p.fx_chains.get(&key)?;
        let i = find_fx_index(chain, &target.fx)?;
        Some(chain[i].fx.guid.clone())
    })
    .ok()
    .flatten()
}

/// Synthesize 8 generic parameters for a new FX. Provides enough surface
/// for tests + UI prototypes without needing real plugin metadata.
fn default_params() -> std::collections::HashMap<u32, f64> {
    (0..8u32).map(|i| (i, 0.5)).collect()
}

fn param_name(idx: u32) -> String {
    format!("Param {}", idx + 1)
}

/// Built-in FX list — returned by `list_installed`. These are aspirational
/// — the audio-graph integration replaces this with real installed FX
/// (FastTrackStudio DSP, CLAP plugins) once available.
fn built_in_installed() -> Vec<InstalledFx> {
    [
        ("ReaComp", "JS: ReaComp"),
        ("ReaEQ", "JS: ReaEQ"),
        ("ReaGate", "JS: ReaGate"),
        ("ReaDelay", "JS: ReaDelay"),
        ("ReaVerbate", "JS: ReaVerbate"),
    ]
    .into_iter()
    .map(|(name, ident)| InstalledFx {
        name: name.to_string(),
        ident: ident.to_string(),
    })
    .collect()
}

/// Instantiate `name` as a live DSP instance: plugin-file backends
/// first (CLAP/VST3 by path), then the injected built-in FX factory
/// ([`crate::plugin::FxFactory`]). `None` ⇒ synthetic fallback.
pub(crate) fn instantiate(
    daw: &Standalone,
    name: &str,
) -> Option<Box<dyn crate::plugin::PluginInstance>> {
    if let Some(p) = crate::plugin::load_plugin_with(name, &daw.plugin_load_options())
        .ok()
        .flatten()
    {
        return Some(p);
    }
    // Instances are re-`prepare`d at the stream's real rate by the
    // renderer; 48 kHz is only the construction-time default.
    daw.fx_factory()?.create(name, 48_000.0)
}

/// Whether the FX `fx_guid` on chain `key` is a plugin loaded from a
/// bundle (CLAP / VST3) with a live instance: the plugin, not the stored
/// chunk, is what holds its state. A built-in of the injected factory is
/// left to its own path (`session_file`'s built-in states).
fn hosts_bundle_state(
    daw: &Standalone,
    project_guid: &str,
    key: &FxChainKey,
    fx_guid: &str,
) -> bool {
    if !daw.plugin_instances.read().contains_key(fx_guid) {
        return false;
    }
    let factory = daw.fx_factory();
    daw.with_project(project_guid, |p| {
        p.fx_chains
            .get(key)
            .and_then(|c| c.iter().find(|e| e.fx.guid == fx_guid))
            .is_some_and(|e| {
                matches!(
                    e.fx.plugin_type,
                    daw_proto::fx::FxType::Clap | daw_proto::fx::FxType::Vst3
                ) && !factory.as_ref().is_some_and(|f| f.provides(&e.fx.name))
            })
    })
    .unwrap_or(false)
}

/// A bundle plugin's state, from the plugin itself (`save_state`): what a
/// save writes and a reopen loads. `None` when the FX is not one, or the
/// plugin would not say.
fn live_state(
    daw: &Standalone,
    project_guid: &str,
    key: &FxChainKey,
    fx_guid: &str,
) -> Option<Vec<u8>> {
    if !hosts_bundle_state(daw, project_guid, key, fx_guid) {
        return None;
    }
    daw.save_plugin_state(fx_guid).ok()
}

/// Load `state` into a bundle plugin (`load_state`). `None` when the FX
/// is not one (the caller stores the chunk as text); an empty state is
/// nothing to load.
fn load_live_state(
    daw: &Standalone,
    project_guid: &str,
    key: &FxChainKey,
    target: &FxTarget,
    state: &[u8],
) -> Option<DawResult<()>> {
    let fx_guid = daw
        .with_project(project_guid, |p| {
            let chain = p.fx_chains.get(key)?;
            let i = find_fx_index(chain, &target.fx)?;
            Some(chain[i].fx.guid.clone())
        })
        .ok()
        .flatten()?;
    if !hosts_bundle_state(daw, project_guid, key, &fx_guid) {
        return None;
    }
    if state.is_empty() {
        return Some(Ok(()));
    }
    Some(
        daw.apply_plugin_state(&fx_guid, state)
            .map_err(|e| DawError::operation_failed(e.to_string())),
    )
}

impl Standalone {
    /// [`Effects::set_state_chunk`] for many FX at once, on
    /// `FTS_STATE_LOAD_THREADS` threads (one unless it says more), then
    /// each hosted plugin
    /// activated there at the project's rate for the largest block
    /// ([`PREPARE_FRAMES`](crate::plugin::PREPARE_FRAMES)) —
    /// so the first block played activates nothing. Each FX is its own
    /// instance behind its own lock, so the engine makes no load wait on
    /// another's. The answers come back in `loads`' order; the
    /// `fx.load_states` span sums each load's and activation's time.
    pub fn set_state_chunks(
        &self,
        project: &ProjectContext,
        loads: Vec<(FxTarget, Vec<u8>)>,
    ) -> Vec<DawResult<()>> {
        use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
        let span = tracing::info_span!(
            "fx.load_states",
            fx.count = loads.len(),
            fx.threads = tracing::field::Empty,
            fx.failed = tracing::field::Empty,
            fx.load_ms = tracing::field::Empty,
            fx.slowest_load_ms = tracing::field::Empty,
            fx.prepare_ms = tracing::field::Empty,
            fx.slowest_prepare_ms = tracing::field::Empty,
        );
        // Each load's and each activation's own time, summed and the
        // slowest: what the plugins cost, beside the wall time it took.
        let (load_us, slowest_load_us) = (AtomicU64::new(0), AtomicU64::new(0));
        let (prepare_us, slowest_prepare_us) = (AtomicU64::new(0), AtomicU64::new(0));
        let timed = |sum: &AtomicU64, slowest: &AtomicU64, began: std::time::Instant| {
            let us = u64::try_from(began.elapsed().as_micros()).unwrap_or(u64::MAX);
            sum.fetch_add(us, Ordering::Relaxed);
            slowest.fetch_max(us, Ordering::Relaxed);
        };
        let _in = span.enter();
        // One at a time unless `FTS_STATE_LOAD_THREADS` says more: Signal's
        // sampler loads (most of the time) wait on each other inside the
        // plugin, so side by side they took longer (13.1 s against 12.3 s
        // for Unbreakable's 246, with 6 threads or 32).
        let threads = std::env::var("FTS_STATE_LOAD_THREADS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(1)
            .min(loads.len())
            .max(1);
        span.record("fx.threads", threads);
        let rate = resolve_project(self, project)
            .map(|guid| f64::from(self.transport_engine_for(&guid).shared.sample_rate()));
        let next = AtomicUsize::new(0);
        let answers: Vec<std::sync::Mutex<Option<DawResult<()>>>> =
            loads.iter().map(|_| std::sync::Mutex::new(None)).collect();
        let parent = tracing::Span::current();
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    let _in = parent.enter();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some((target, chunk)) = loads.get(i) else {
                            break;
                        };
                        let began = std::time::Instant::now();
                        let loaded =
                            self.set_state_chunk(project.clone(), target.clone(), chunk.clone());
                        timed(&load_us, &slowest_load_us, began);
                        if loaded.is_ok()
                            && let Some(rate) = rate
                        {
                            let began = std::time::Instant::now();
                            self.prepare_ahead(project, target, rate);
                            timed(&prepare_us, &slowest_prepare_us, began);
                        }
                        if let Ok(mut slot) = answers[i].lock() {
                            *slot = Some(loaded);
                        }
                    }
                });
            }
        });
        let answers: Vec<DawResult<()>> = answers
            .into_iter()
            .map(|a| {
                a.into_inner()
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| Err(DawError::operation_failed("state load did not run")))
            })
            .collect();
        span.record("fx.failed", answers.iter().filter(|a| a.is_err()).count());
        let ms = |us: &AtomicU64| us.load(Ordering::Relaxed) as f64 / 1e3;
        span.record("fx.load_ms", ms(&load_us));
        span.record("fx.slowest_load_ms", ms(&slowest_load_us));
        span.record("fx.prepare_ms", ms(&prepare_us));
        span.record("fx.slowest_prepare_ms", ms(&slowest_prepare_us));
        answers
    }

    /// Activate the plugin behind `target` at `rate` for the largest block,
    /// off the audio thread, unless it already is. A failure is left to
    /// the first block, which tries again and bypasses it.
    fn prepare_ahead(&self, project: &ProjectContext, target: &FxTarget, rate: f64) {
        let Some(guid) = resolve_project(self, project) else {
            return;
        };
        let key: FxChainKey = (&target.context).into();
        let Some(fx_guid) = self
            .with_project(&guid, |p| {
                let chain = p.fx_chains.get(&key)?;
                let i = find_fx_index(chain, &target.fx)?;
                Some(chain[i].fx.guid.clone())
            })
            .ok()
            .flatten()
        else {
            return;
        };
        self.plugin_instances.with(&fx_guid, |p| {
            if !p.is_prepared() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    p.prepare(rate, crate::plugin::PREPARE_FRAMES)
                }));
            }
        });
    }
}

impl Effects for Standalone {
    fn list_installed(&self) -> Vec<InstalledFx> {
        let mut list = built_in_installed();
        if let Some(factory) = self.fx_factory() {
            list.extend(factory.installed());
        }
        list
    }

    fn last_touched(&self) -> Option<LastTouchedFx> {
        self.engine_state().last_touched_fx.clone()
    }

    fn list(&self, project: ProjectContext, chain: FxChainContext) -> Vec<Fx> {
        let Some(guid) = resolve_project(self, &project) else {
            return Vec::new();
        };
        let key: FxChainKey = (&chain).into();
        self.with_project(&guid, |p| {
            p.fx_chains
                .get(&key)
                .map(|c| c.iter().map(|e| e.fx.clone()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default()
    }

    fn get(&self, project: ProjectContext, target: FxTarget) -> Option<Fx> {
        let guid = resolve_project(self, &project)?;
        let key: FxChainKey = (&target.context).into();
        self.with_project(&guid, |p| {
            let chain = p.fx_chains.get(&key)?;
            let i = find_fx_index(chain, &target.fx)?;
            Some(chain[i].fx.clone())
        })
        .ok()
        .flatten()
    }

    fn count(&self, project: ProjectContext, chain: FxChainContext) -> u32 {
        let Some(guid) = resolve_project(self, &project) else {
            return 0;
        };
        let key: FxChainKey = (&chain).into();
        self.with_project(&guid, |p| {
            p.fx_chains.get(&key).map(|c| c.len() as u32).unwrap_or(0)
        })
        .unwrap_or(0)
    }

    fn set_enabled(
        &self,
        project: ProjectContext,
        target: FxTarget,
        enabled: bool,
    ) -> DawResult<()> {
        let guid = resolve_project(self, &project).ok_or_else(not_found_proj)?;
        let key: FxChainKey = (&target.context).into();
        let fx_guid = self.with_project_mut(&guid, |p| {
            let chain = p.fx_chains.get_mut(&key).ok_or_else(not_found_fx)?;
            let i = find_fx_index(chain, &target.fx).ok_or_else(not_found_fx)?;
            chain[i].fx.enabled = enabled;
            Ok::<String, DawError>(chain[i].fx.guid.clone())
        })??;
        self.publish_fx_event(
            &guid,
            daw_proto::FxEvent::EnabledChanged {
                context: target.context,
                fx_guid,
                enabled,
            },
        );
        Ok(())
    }

    fn set_offline(
        &self,
        project: ProjectContext,
        target: FxTarget,
        offline: bool,
    ) -> DawResult<()> {
        let guid = resolve_project(self, &project).ok_or_else(not_found_proj)?;
        let key: FxChainKey = (&target.context).into();
        self.with_project_mut(&guid, |p| {
            let chain = p.fx_chains.get_mut(&key).ok_or_else(not_found_fx)?;
            let i = find_fx_index(chain, &target.fx).ok_or_else(not_found_fx)?;
            chain[i].fx.offline = offline;
            Ok::<(), DawError>(())
        })?
    }

    fn add(&self, project: ProjectContext, chain: FxChainContext, name: &str) -> Option<String> {
        let guid = resolve_project(self, &project)?;
        let key: FxChainKey = (&chain).into();
        // Try to instantiate real DSP first — plugin-file backends
        // (CLAP today; VST3/LV2 land when their backends ship), then
        // the injected built-in FX factory. On failure or for unknown
        // names fall through to the existing synthetic 8-param entry.
        let mut real_plugin = instantiate(self, name);
        let real_param_count = real_plugin
            .as_mut()
            .map(|p| p.params().len() as u32)
            .unwrap_or(8);
        let ranges = param_ranges(real_plugin.as_mut());
        let real_descriptor = real_plugin.as_ref().map(|p| p.descriptor());
        let plugin_format = real_descriptor
            .as_ref()
            .map(|d| d.format)
            .unwrap_or(crate::plugin::PluginFormat::Synthetic);
        let has_instance = real_plugin.is_some();

        let new_guid = self
            .with_project_mut(&guid, |p| {
                let chain_vec = p.fx_chains.entry(key).or_default();
                let new_index = chain_vec.len() as u32;
                let new_guid = Uuid::new_v4().to_string();
                p.next_fx_counter += 1;

                let mut fx = Fx::new(new_guid.clone(), new_index, name.to_string());
                fx.plugin_name = real_descriptor
                    .as_ref()
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| name.to_string());
                fx.plugin_type = match plugin_format {
                    crate::plugin::PluginFormat::Clap => daw_proto::fx::FxType::Clap,
                    crate::plugin::PluginFormat::Vst3 => daw_proto::fx::FxType::Vst3,
                    _ => guess_fx_type(name),
                };
                fx.parameter_count = real_param_count;

                chain_vec.push(FxEntry {
                    fx,
                    state_chunk: String::new(),
                    // Live instances resolve params against the plugin
                    // (stored values are PLAIN overrides under the real
                    // param id — see `fx_params.rs`), so start with no
                    // overrides and let the DSP keep its defaults.
                    // Synthetic FX keep the 8 generic slot params.
                    params: if has_instance {
                        Default::default()
                    } else {
                        default_params()
                    },
                    ranges,
                });
                new_guid
            })
            .ok()?;
        // Stash the loaded plugin instance for the renderer to find.
        if let Some(plugin) = real_plugin {
            self.plugin_instances
                .write()
                .insert(new_guid.clone(), plugin);
        }
        let added = self
            .with_project(&guid, |p| {
                p.fx_chains
                    .get(&FxChainKey::from(&chain))
                    .and_then(|c| c.iter().find(|e| e.fx.guid == new_guid))
                    .map(|e| e.fx.clone())
            })
            .ok()
            .flatten();
        if let Some(fx) = added {
            self.republish_fx_counts(&guid, &chain);
            self.publish_fx_event(&guid, daw_proto::FxEvent::Added { context: chain, fx });
        }
        Some(new_guid)
    }

    fn add_at(&self, project: ProjectContext, request: AddFxAtRequest) -> Option<String> {
        let guid = resolve_project(self, &project)?;
        let key: FxChainKey = (&request.context).into();
        // Same real-DSP-first path as `add` (plugin files, then the
        // built-in factory), falling back to a synthetic entry.
        let mut real_plugin = instantiate(self, &request.name);
        let real_param_count = real_plugin
            .as_mut()
            .map(|p| p.params().len() as u32)
            .unwrap_or(8);
        let ranges = param_ranges(real_plugin.as_mut());
        let real_descriptor = real_plugin.as_ref().map(|p| p.descriptor());
        let has_instance = real_plugin.is_some();
        let new_guid = self
            .with_project_mut(&guid, |p| {
                let chain_vec = p.fx_chains.entry(key).or_default();
                let pos = (request.index as usize).min(chain_vec.len());
                let new_guid = Uuid::new_v4().to_string();
                p.next_fx_counter += 1;

                let mut fx = Fx::new(new_guid.clone(), pos as u32, request.name.clone());
                fx.plugin_name = real_descriptor
                    .as_ref()
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| request.name.clone());
                fx.plugin_type = match real_descriptor.as_ref().map(|d| d.format) {
                    Some(crate::plugin::PluginFormat::Clap) => daw_proto::fx::FxType::Clap,
                    Some(crate::plugin::PluginFormat::Vst3) => daw_proto::fx::FxType::Vst3,
                    _ => guess_fx_type(&request.name),
                };
                fx.parameter_count = real_param_count;

                chain_vec.insert(
                    pos,
                    FxEntry {
                        fx,
                        state_chunk: String::new(),
                        params: if has_instance {
                            Default::default()
                        } else {
                            default_params()
                        },
                        ranges,
                    },
                );
                // Renumber.
                for (i, e) in chain_vec.iter_mut().enumerate() {
                    e.fx.index = i as u32;
                }
                Some(new_guid)
            })
            .ok()
            .flatten()?;
        // Stash the loaded plugin instance for the renderer to find.
        if let Some(plugin) = real_plugin {
            self.insert_plugin_instance(new_guid.clone(), plugin);
        }
        self.republish_fx_counts(&guid, &request.context);
        Some(new_guid)
    }

    fn remove(&self, project: ProjectContext, target: FxTarget) -> DawResult<()> {
        let guid = resolve_project(self, &project).ok_or_else(not_found_proj)?;
        let key: FxChainKey = (&target.context).into();
        let removed_guid = self.with_project_mut(&guid, |p| {
            let chain = p.fx_chains.get_mut(&key).ok_or_else(not_found_fx)?;
            let i = find_fx_index(chain, &target.fx).ok_or_else(not_found_fx)?;
            let removed = chain.remove(i);
            for (i, e) in chain.iter_mut().enumerate() {
                e.fx.index = i as u32;
            }
            Ok::<String, DawError>(removed.fx.guid)
        })??;
        // Drop any loaded plugin instance backing this FX entry.
        let removed = self.plugin_instances.write().remove(&removed_guid);
        drop(removed);
        self.republish_fx_counts(&guid, &target.context);
        self.publish_fx_event(
            &guid,
            daw_proto::FxEvent::Removed {
                context: target.context,
                fx_guid: removed_guid,
            },
        );
        Ok(())
    }

    fn move_to(&self, project: ProjectContext, target: FxTarget, new_index: u32) -> DawResult<()> {
        let guid = resolve_project(self, &project).ok_or_else(not_found_proj)?;
        let key: FxChainKey = (&target.context).into();
        self.with_project_mut(&guid, |p| {
            let chain = p.fx_chains.get_mut(&key).ok_or_else(not_found_fx)?;
            let i = find_fx_index(chain, &target.fx).ok_or_else(not_found_fx)?;
            let entry = chain.remove(i);
            let dest = (new_index as usize).min(chain.len());
            chain.insert(dest, entry);
            for (i, e) in chain.iter_mut().enumerate() {
                e.fx.index = i as u32;
            }
            Ok::<(), DawError>(())
        })?
    }

    fn parameters(&self, project: ProjectContext, target: FxTarget) -> Vec<FxParameter> {
        if let Some(fx_guid) = resolve_fx_guid(self, &project, &target) {
            // Live plugin: slot-indexed, normalized, engine overrides
            // preferred — same view FxParams exposes.
            let stored = resolve_project(self, &project).and_then(|guid| {
                self.with_project(&guid, |p| {
                    p.fx_chains
                        .get(&FxChainKey::from(&target.context))
                        .and_then(|c| c.iter().find(|e| e.fx.guid == fx_guid))
                        .map(|e| e.params.clone())
                })
                .ok()
                .flatten()
            });
            if let Some(params) = self.slot_parameters(&fx_guid, &stored.unwrap_or_default()) {
                return params;
            }
        }

        let Some(guid) = resolve_project(self, &project) else {
            return Vec::new();
        };
        let key: FxChainKey = (&target.context).into();
        self.with_project(&guid, |p| {
            let chain = match p.fx_chains.get(&key) {
                Some(c) => c,
                None => return Vec::new(),
            };
            let Some(i) = find_fx_index(chain, &target.fx) else {
                return Vec::new();
            };
            let entry = &chain[i];
            (0..entry.fx.parameter_count)
                .map(|pidx| FxParameter {
                    index: pidx,
                    name: param_name(pidx),
                    value: entry.params.get(&pidx).copied().unwrap_or(0.0),
                    formatted: format!("{:.2}", entry.params.get(&pidx).copied().unwrap_or(0.0)),
                    is_toggle: false,
                    step_count: None,
                    step_labels: Vec::new(),
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn parameter(
        &self,
        project: ProjectContext,
        target: FxTarget,
        index: u32,
    ) -> Option<FxParameter> {
        Effects::parameters(self, project, target)
            .into_iter()
            .find(|p| p.index == index)
    }

    fn set_parameter(
        &self,
        project: ProjectContext,
        request: SetParameterRequest,
    ) -> DawResult<()> {
        let guid = resolve_project(self, &project).ok_or_else(not_found_proj)?;
        let key: FxChainKey = (&request.target.context).into();
        // Resolve the chain index, then delegate to FxParams::set —
        // one slot→param-id translation + ParameterChanged publish
        // path for every API surface.
        let fx_idx = self.with_project(&guid, |p| {
            p.fx_chains
                .get(&key)
                .and_then(|chain| find_fx_index(chain, &request.target.fx))
        })?;
        let fx_idx = fx_idx.ok_or_else(not_found_fx)? as u32;
        daw_proto::FxParams::set(
            self,
            request.target.context.clone(),
            fx_idx,
            request.index,
            request.value.clamp(0.0, 1.0),
        )?;
        // Update last-touched FX.
        let (track_guid, is_input_fx) = match &request.target.context {
            FxChainContext::Track(g) => (g.clone(), false),
            FxChainContext::Input(g) => (g.clone(), true),
            FxChainContext::Monitoring => (String::new(), false),
        };
        // Look up the FX's chain index for the touched record.
        let fx_index = Effects::get(self, project, request.target.clone())
            .map(|fx| fx.index)
            .unwrap_or(0);
        self.engine_state().last_touched_fx = Some(LastTouchedFx {
            track_guid,
            fx_index,
            param_index: request.index,
            is_input_fx,
        });
        let _ = guid;
        Ok(())
    }

    fn parameter_by_name(
        &self,
        project: ProjectContext,
        target: FxTarget,
        name: &str,
    ) -> Option<FxParameter> {
        Effects::parameters(self, project, target)
            .into_iter()
            .find(|p| p.name == name)
    }

    fn set_parameter_by_name(
        &self,
        project: ProjectContext,
        request: SetParameterByNameRequest,
    ) -> DawResult<()> {
        let pname = request.name.clone();
        let param =
            Effects::parameter_by_name(self, project.clone(), request.target.clone(), &pname)
                .ok_or_else(|| DawError::not_found("FxParameter", &pname))?;
        Effects::set_parameter(
            self,
            project,
            SetParameterRequest {
                target: request.target,
                index: param.index,
                value: request.value,
            },
        )
    }

    fn open_ui(&self, project: ProjectContext, target: FxTarget) -> DawResult<()> {
        if let Some(fx_guid) = resolve_fx_guid(self, &project, &target) {
            let plugins = self.plugin_instances.read();
            if let Some(mut plugin) = plugins.cell(&fx_guid).map(crate::plugin::PluginCell::lock) {
                plugin
                    .open_gui()
                    .map_err(|e| DawError::operation_failed(e.to_string()))?;
            }
        }
        set_window_open(self, project, target, true)
    }
    fn close_ui(&self, project: ProjectContext, target: FxTarget) -> DawResult<()> {
        if let Some(fx_guid) = resolve_fx_guid(self, &project, &target) {
            let plugins = self.plugin_instances.read();
            if let Some(mut plugin) = plugins.cell(&fx_guid).map(crate::plugin::PluginCell::lock) {
                plugin
                    .close_gui()
                    .map_err(|e| DawError::operation_failed(e.to_string()))?;
            }
        }
        set_window_open(self, project, target, false)
    }
    fn toggle_ui(&self, project: ProjectContext, target: FxTarget) -> DawResult<()> {
        let cur = Effects::get(self, project.clone(), target.clone())
            .map(|fx| fx.window_open)
            .unwrap_or(false);
        set_window_open(self, project, target, !cur)
    }

    fn state_chunk(&self, project: ProjectContext, target: FxTarget) -> Option<Vec<u8>> {
        let guid = resolve_project(self, &project)?;
        let key: FxChainKey = (&target.context).into();
        let (fx_guid, stored) = self
            .with_project(&guid, |p| {
                let chain = p.fx_chains.get(&key)?;
                let i = find_fx_index(chain, &target.fx)?;
                Some((
                    chain[i].fx.guid.clone(),
                    chain[i].state_chunk.clone().into_bytes(),
                ))
            })
            .ok()
            .flatten()?;
        Some(live_state(self, &guid, &key, &fx_guid).unwrap_or(stored))
    }

    fn set_state_chunk(
        &self,
        project: ProjectContext,
        target: FxTarget,
        chunk: Vec<u8>,
    ) -> DawResult<()> {
        let guid = resolve_project(self, &project).ok_or_else(not_found_proj)?;
        let key: FxChainKey = (&target.context).into();
        if let Some(loaded) = load_live_state(self, &guid, &key, &target, &chunk) {
            return loaded;
        }
        let text = String::from_utf8(chunk)
            .map_err(|_| DawError::operation_failed("non-UTF8 state chunk"))?;
        self.with_project_mut(&guid, |p| {
            let chain = p.fx_chains.get_mut(&key).ok_or_else(not_found_fx)?;
            let i = find_fx_index(chain, &target.fx).ok_or_else(not_found_fx)?;
            chain[i].state_chunk = text;
            Ok::<(), DawError>(())
        })?
    }

    fn state_chunk_encoded(&self, project: ProjectContext, target: FxTarget) -> Option<String> {
        use base64::Engine;
        let bytes = Effects::state_chunk(self, project, target)?;
        Some(base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    fn set_state_chunk_encoded(
        &self,
        project: ProjectContext,
        target: FxTarget,
        encoded: &str,
    ) -> DawResult<()> {
        use base64::Engine;
        let guid = resolve_project(self, &project)
            .ok_or_else(|| DawError::not_found("project", "current"))?;
        let key: FxChainKey = (&target.context).into();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .map_err(|e| DawError::operation_failed(format!("base64 decode: {e}")))?;
        if let Some(loaded) = load_live_state(self, &guid, &key, &target, &bytes) {
            return loaded;
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| DawError::operation_failed("decoded state chunk is not UTF-8"))?;
        self.with_project_mut(&guid, |p| {
            let chain = p.fx_chains.get_mut(&key).ok_or_else(not_found_fx)?;
            let i = find_fx_index(chain, &target.fx).ok_or_else(not_found_fx)?;
            chain[i].state_chunk = text;
            Ok::<(), DawError>(())
        })?
    }

    fn chain_state(&self, project: ProjectContext, chain: FxChainContext) -> Vec<FxStateChunk> {
        let Some(guid) = resolve_project(self, &project) else {
            return Vec::new();
        };
        let key: FxChainKey = (&chain).into();
        self.with_project(&guid, |p| {
            p.fx_chains
                .get(&key)
                .map(|c| {
                    c.iter()
                        .map(|e| FxStateChunk {
                            fx_guid: e.fx.guid.clone(),
                            fx_index: e.fx.index,
                            plugin_name: e.fx.plugin_name.clone(),
                            encoded_chunk: e.state_chunk.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default()
    }

    fn set_chain_state(
        &self,
        project: ProjectContext,
        chain: FxChainContext,
        chunks: Vec<FxStateChunk>,
    ) -> DawResult<()> {
        let guid = resolve_project(self, &project).ok_or_else(not_found_proj)?;
        let key: FxChainKey = (&chain).into();
        self.with_project_mut(&guid, |p| {
            let chain_vec = p.fx_chains.get_mut(&key).ok_or_else(not_found_fx)?;
            for chunk in chunks {
                if let Some(e) = chain_vec.iter_mut().find(|e| e.fx.guid == chunk.fx_guid) {
                    e.state_chunk = chunk.encoded_chunk;
                }
            }
            Ok::<(), DawError>(())
        })?
    }

    // ── Deferred — todo!() ────────────────────────────────────────────

    // Preset enumeration — we don't introspect plugin presets (would
    // need real-plugin metadata). Store an arbitrary "active index"
    // per FX in a side map so set/get round-trips, but don't enumerate
    // names. Tests that depend on exact REAPER preset behaviour will
    // need to mock; tests that just care about the trait contract
    // see a stable surface.
    fn preset_index(&self, project: ProjectContext, target: FxTarget) -> Option<FxPresetIndex> {
        let guid = resolve_project(self, &project)?;
        let key: FxChainKey = (&target.context).into();
        self.with_project(&guid, |p| {
            let chain = p.fx_chains.get(&key)?;
            let i = find_fx_index(chain, &target.fx)?;
            let fx_guid = &chain[i].fx.guid;
            p.fx_preset_index
                .get(fx_guid)
                .copied()
                .map(|index| FxPresetIndex {
                    index: Some(index),
                    count: 0,
                    name: None,
                })
        })
        .ok()
        .flatten()
    }
    fn next_preset(&self, project: ProjectContext, target: FxTarget) -> DawResult<()> {
        bump_preset(self, &project, &target, 1)
    }
    fn prev_preset(&self, project: ProjectContext, target: FxTarget) -> DawResult<()> {
        bump_preset(self, &project, &target, -1)
    }
    fn set_preset(&self, project: ProjectContext, target: FxTarget, index: u32) -> DawResult<()> {
        let guid = resolve_project(self, &project)
            .ok_or_else(|| DawError::not_found("project", "current"))?;
        let key: FxChainKey = (&target.context).into();
        self.with_project_mut(&guid, |p| {
            let chain = p.fx_chains.get_mut(&key).ok_or_else(not_found_fx)?;
            let i = find_fx_index(chain, &target.fx).ok_or_else(not_found_fx)?;
            let fx_guid = chain[i].fx.guid.clone();
            p.fx_preset_index.insert(fx_guid, index);
            Ok::<(), DawError>(())
        })?
    }
    fn named_config(&self, project: ProjectContext, target: FxTarget, key: &str) -> Option<String> {
        let guid = resolve_project(self, &project)?;
        let chain_key: FxChainKey = (&target.context).into();
        self.with_project(&guid, |p| {
            let chain = p.fx_chains.get(&chain_key)?;
            let i = find_fx_index(chain, &target.fx)?;
            let fx_guid = chain[i].fx.guid.clone();
            p.fx_named_config.get(&(fx_guid, key.to_string())).cloned()
        })
        .ok()
        .flatten()
    }
    fn set_named_config(
        &self,
        project: ProjectContext,
        request: SetNamedConfigRequest,
    ) -> DawResult<()> {
        let guid = resolve_project(self, &project)
            .ok_or_else(|| DawError::not_found("project", "current"))?;
        let chain_key: FxChainKey = (&request.target.context).into();
        self.with_project_mut(&guid, |p| {
            let chain = p.fx_chains.get_mut(&chain_key).ok_or_else(not_found_fx)?;
            let i = find_fx_index(chain, &request.target.fx).ok_or_else(not_found_fx)?;
            let fx_guid = chain[i].fx.guid.clone();
            p.fx_named_config
                .insert((fx_guid, request.key.clone()), request.value.clone());
            Ok::<(), DawError>(())
        })?
    }
    fn latency(&self, project: ProjectContext, target: FxTarget) -> Option<FxLatency> {
        if let Some(fx_guid) = resolve_fx_guid(self, &project, &target) {
            let plugins = self.plugin_instances.read();
            if let Some(mut plugin) = plugins.cell(&fx_guid).map(crate::plugin::PluginCell::lock) {
                let samples = plugin.latency() as i32;
                return Some(FxLatency {
                    pdc_samples: samples,
                    chain_pdc_actual: samples,
                    chain_pdc_reporting: samples,
                });
            }
        }
        Some(FxLatency::default())
    }
    fn param_modulation(
        &self,
        _project: ProjectContext,
        _target: FxTarget,
        _param_index: u32,
    ) -> Option<FxParamModulation> {
        Some(FxParamModulation::default())
    }
    fn channel_config(
        &self,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Option<FxChannelConfig> {
        // Pin routing isn't modelled; return the all-defaults channel
        // config so callers can read a sensible value without panicking.
        Some(FxChannelConfig::default())
    }
    fn set_channel_config(
        &self,
        _project: ProjectContext,
        _target: FxTarget,
        _config: FxChannelConfig,
    ) -> DawResult<()> {
        // Accept + discard. We don't have a pin map to update yet.
        Ok(())
    }
    fn silence_output(
        &self,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> DawResult<FxPinMappings> {
        // "Silence output" is a REAPER trick: zero out the FX's pin
        // map so its output bus produces silence, return the old
        // mapping so the caller can restore. We don't have pin maps,
        // so just hand back the defaults and accept the no-op.
        Ok(FxPinMappings::default())
    }
    fn restore_output(
        &self,
        _project: ProjectContext,
        _target: FxTarget,
        _saved: FxPinMappings,
    ) -> DawResult<()> {
        Ok(())
    }
    fn tree(&self, project: ProjectContext, chain: FxChainContext) -> FxTree {
        // Flatten the existing chain into a single-level tree (no
        // containers). Once container ops land we'll model nested
        // FxNodes; for now every plugin is a top-level Plugin node.
        let Some(guid) = resolve_project(self, &project) else {
            return FxTree::default();
        };
        let key: FxChainKey = (&chain).into();
        self.with_project(&guid, |p| {
            let Some(entries) = p.fx_chains.get(&key) else {
                return FxTree::default();
            };
            FxTree {
                nodes: entries
                    .iter()
                    .map(|e| daw_proto::fx::FxNode {
                        id: daw_proto::fx::FxNodeId::from_guid(e.fx.guid.clone()),
                        kind: daw_proto::fx::FxNodeKind::Plugin(e.fx.clone()),
                        enabled: e.fx.enabled,
                        parent_id: None,
                    })
                    .collect(),
            }
        })
        .unwrap_or_default()
    }
    fn create_container(
        &self,
        _project: ProjectContext,
        _request: CreateContainerRequest,
    ) -> Option<FxNodeId> {
        // REAPER 7 FX containers aren't modelled. Return None so the
        // caller knows the op didn't take.
        None
    }
    fn move_to_container(
        &self,
        _project: ProjectContext,
        _request: MoveToContainerRequest,
    ) -> DawResult<()> {
        Err(DawError::operation_failed(
            "standalone: FX containers not modelled",
        ))
    }
    fn move_from_container(
        &self,
        _project: ProjectContext,
        _request: MoveFromContainerRequest,
    ) -> DawResult<()> {
        Err(DawError::operation_failed(
            "standalone: FX containers not modelled",
        ))
    }
    fn set_routing_mode(
        &self,
        _project: ProjectContext,
        _chain: FxChainContext,
        _node_id: FxNodeId,
        _mode: FxRoutingMode,
    ) -> DawResult<()> {
        Err(DawError::operation_failed(
            "standalone: FX containers not modelled",
        ))
    }
    fn container_channel_config(
        &self,
        _project: ProjectContext,
        _chain: FxChainContext,
        _container_id: FxNodeId,
    ) -> Option<FxContainerChannelConfig> {
        None
    }
    fn set_container_channel_config(
        &self,
        _project: ProjectContext,
        _request: SetContainerChannelConfigRequest,
    ) -> DawResult<()> {
        Err(DawError::operation_failed(
            "standalone: FX containers not modelled",
        ))
    }
    fn enclose_in_container(
        &self,
        _project: ProjectContext,
        _request: EncloseInContainerRequest,
    ) -> Option<FxNodeId> {
        None
    }
    fn explode_container(
        &self,
        _project: ProjectContext,
        _chain: FxChainContext,
        _container_id: FxNodeId,
    ) -> DawResult<()> {
        Err(DawError::operation_failed(
            "standalone: FX containers not modelled",
        ))
    }
    fn rename_container(
        &self,
        _project: ProjectContext,
        _chain: FxChainContext,
        _container_id: FxNodeId,
        _name: &str,
    ) -> DawResult<()> {
        Err(DawError::operation_failed(
            "standalone: FX containers not modelled",
        ))
    }
    fn chain_chunk_text(&self, project: ProjectContext, chain: FxChainContext) -> Option<String> {
        // Concatenate the raw state chunks per-FX as a best-effort
        // approximation. Full RPP chunk text serialization (with
        // BYPASS/FXID/WAK/PRESETNAME framing) isn't reconstructed —
        // that needs dawfile-reaper's writer.
        let guid = resolve_project(self, &project)?;
        let key: FxChainKey = (&chain).into();
        self.with_project(&guid, |p| {
            let chain = p.fx_chains.get(&key)?;
            let mut text = String::new();
            for e in chain {
                text.push_str(&e.state_chunk);
                text.push('\n');
            }
            Some(text)
        })
        .ok()
        .flatten()
    }
    fn insert_chain_chunk(
        &self,
        _project: ProjectContext,
        _chain: FxChainContext,
        _chunk_text: &str,
    ) -> DawResult<()> {
        todo!("standalone: Effects::insert_chain_chunk")
    }
}

fn set_window_open(
    daw: &Standalone,
    project: ProjectContext,
    target: FxTarget,
    open: bool,
) -> DawResult<()> {
    let guid = resolve_project(daw, &project).ok_or_else(not_found_proj)?;
    let key: FxChainKey = (&target.context).into();
    daw.with_project_mut(&guid, |p| {
        let chain = p.fx_chains.get_mut(&key).ok_or_else(not_found_fx)?;
        let i = find_fx_index(chain, &target.fx).ok_or_else(not_found_fx)?;
        chain[i].fx.window_open = open;
        Ok::<(), DawError>(())
    })?
}

fn guess_fx_type(name: &str) -> FxType {
    let lower = name.to_lowercase();
    if lower.starts_with("vst3:") {
        FxType::Vst3
    } else if lower.starts_with("vst:") || lower.starts_with("vst2:") {
        FxType::Vst2
    } else if lower.starts_with("clap:") {
        FxType::Clap
    } else if lower.starts_with("au:") {
        FxType::Au
    } else if lower.starts_with("js:") {
        FxType::Js
    } else {
        FxType::Unknown
    }
}

/// A live instance's parameters by slot, as an [`FxEntry`] keeps them:
/// each one's id and plain range. Empty with no instance.
fn param_ranges(
    plugin: Option<&mut Box<dyn crate::plugin::PluginInstance>>,
) -> Vec<(u32, f64, f64)> {
    plugin.map_or_else(Vec::new, |p| {
        p.params()
            .iter()
            .map(|info| (info.id, info.min, info.max))
            .collect()
    })
}

#[cfg(all(test, feature = "clap-host"))]
mod clap_state_tests {
    use daw_proto::fx::{Effects, FxChainContext, FxRef, FxTarget, SetParameterRequest};
    use daw_proto::{ProjectContext, Tracks};

    /// A CLAP in the user's folder, when there is one to test with.
    fn a_clap() -> Option<String> {
        let home = std::env::var_os("HOME")?;
        ["FTS EQ.clap", "DragonflyRoomReverb.clap", "ZamComp.clap"]
            .iter()
            .map(|name| std::path::Path::new(&home).join(".clap").join(name))
            .find(|p| p.exists())
            .and_then(|p| p.to_str().map(str::to_owned))
    }

    #[test]
    fn a_clap_s_state_chunk_is_the_plugin_s_own() {
        let Some(clap) = a_clap() else {
            return;
        };
        let daw = crate::sync::Standalone::new();
        let song = daw.seed_project(daw_proto::ProjectInfo {
            guid: "clap-state".into(),
            name: "Clap".into(),
            path: String::new(),
        });
        let ctx = ProjectContext::Project(song);
        let track = Tracks::add(&daw, ctx.clone(), "Kick", None).expect("a track");
        let chain = FxChainContext::Track(track);
        let fx = Effects::add(&daw, ctx.clone(), chain.clone(), &clap).expect("the CLAP");
        let target = FxTarget {
            context: chain,
            fx: FxRef::Guid(fx.clone()),
        };
        let before = daw
            .state_chunk(ctx.clone(), target.clone())
            .expect("a state");
        assert!(
            !before.is_empty(),
            "the plugin's own state, not the stored text"
        );
        // A parameter moved in the plugin, then the old state loaded back.
        let first = daw.parameters(ctx.clone(), target.clone())[0].clone();
        daw.set_parameter(
            ctx.clone(),
            SetParameterRequest {
                target: target.clone(),
                index: first.index,
                value: if first.value > 0.5 { 0.1 } else { 0.9 },
            },
        )
        .expect("set");
        daw.set_state_chunk(ctx.clone(), target.clone(), before.clone())
            .expect("loaded");
        assert_eq!(daw.state_chunk(ctx, target), Some(before));
    }
}

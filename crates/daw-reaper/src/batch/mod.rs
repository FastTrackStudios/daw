//! Batch executor — executes a batch program of instructions sequentially,
//! resolving cross-step dependencies and optionally grouping mutations in
//! a single REAPER undo block.
//!
//! Two execution paths:
//! - **Sync** (default): Entire batch runs in a single `main_thread::query()` call.
//!   Each op executes directly on REAPER's main thread at native speed (~µs per op).
//! - **Async** (fallback): Used when any op lacks sync support. Each op goes through
//!   the async service layer with per-op `main_thread::query()` (~33ms per op).

mod dispatch;
mod dispatch_sync;
mod resolve;

use std::sync::Arc;

use crate::main_thread;
use daw_proto::batch::*;

/// Inner state for the batch executor (not Clone due to ReaperAudioAccessor's Mutex).
struct BatchExecutorInner {
    project_svc: crate::ReaperProject,
    transport_svc: crate::ReaperTransport,
    track_svc: crate::ReaperTrack,
    fx_svc: crate::ReaperFx,
    routing_svc: crate::ReaperRouting,
    item_svc: crate::ReaperItem,
    take_svc: crate::ReaperTake,
    marker_svc: crate::ReaperMarker,
    region_svc: crate::ReaperRegion,
    tempo_map_svc: crate::ReaperTempoMap,
    midi_svc: crate::ReaperMidi,
    live_midi_svc: crate::ReaperLiveMidi,
    ext_state_svc: crate::ReaperExtState,
    audio_engine_svc: crate::ReaperAudioEngine,
    position_svc: crate::ReaperPositionConversion,
    health_svc: crate::ReaperHealth,
    action_registry_svc: crate::ReaperActionRegistry,
    toolbar_svc: crate::ReaperToolbar,
    plugin_loader_svc: crate::ReaperPluginLoader,
    peak_svc: crate::ReaperPeak,
    resource_svc: crate::resource::ReaperResource,
    audio_accessor_svc: crate::ReaperAudioAccessor,
    midi_analysis_svc: crate::ReaperMidiAnalysis,
}

/// Batch executor that holds all service implementations behind an Arc for Clone.
#[derive(Clone)]
pub struct BatchExecutor {
    inner: Arc<BatchExecutorInner>,
}

impl BatchExecutor {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(BatchExecutorInner {
                project_svc: crate::ReaperProject::new(),
                transport_svc: crate::ReaperTransport::new(),
                track_svc: crate::ReaperTrack::new(),
                fx_svc: crate::ReaperFx::new(),
                routing_svc: crate::ReaperRouting::new(),
                item_svc: crate::ReaperItem::new(),
                take_svc: crate::ReaperTake::new(),
                marker_svc: crate::ReaperMarker::new(),
                region_svc: crate::ReaperRegion::new(),
                tempo_map_svc: crate::ReaperTempoMap::new(),
                midi_svc: crate::ReaperMidi::new(),
                live_midi_svc: crate::ReaperLiveMidi::new(),
                ext_state_svc: crate::ReaperExtState::new(),
                audio_engine_svc: crate::ReaperAudioEngine::new(),
                position_svc: crate::ReaperPositionConversion::new(),
                health_svc: crate::ReaperHealth::new(),
                action_registry_svc: crate::ReaperActionRegistry::new(),
                toolbar_svc: crate::ReaperToolbar::new(),
                plugin_loader_svc: crate::ReaperPluginLoader::new(),
                peak_svc: crate::ReaperPeak::new(),
                resource_svc: crate::resource::ReaperResource::new(),
                audio_accessor_svc: crate::ReaperAudioAccessor::new(),
                midi_analysis_svc: crate::ReaperMidiAnalysis::new(),
            }),
        }
    }

    /// Check if all ops in the request are supported by the sync path.
    fn all_ops_sync_supported(request: &BatchRequest) -> bool {
        request
            .instructions
            .iter()
            .all(|i| dispatch_sync::is_sync_supported(&i.op))
    }

    /// Execute the entire batch in a single main_thread::query() call.
    ///
    /// All operations run synchronously on REAPER's main thread, eliminating
    /// the ~33ms timer callback latency per operation.
    async fn execute_sync(&self, request: BatchRequest) -> BatchResponse {
        let inner = Arc::clone(&self.inner);
        main_thread::query(move || {
            use crate::project::UNDO_LABEL;
            use crate::project_context::{project_guid, resolve_project_context};
            use crate::track::{clear_project_cache, set_project_cache};
            use daw_proto::ProjectContext;
            use reaper_high::Reaper;
            use reaper_medium::UndoScope;

            let services = dispatch_sync::SyncServices {
                audio_accessor_svc: &inner.audio_accessor_svc,
            };

            // Cache the current project GUID so resolve_project() skips FFI per op
            let current_project = Reaper::get().current_project();
            let current_guid = project_guid(&current_project);
            set_project_cache(current_guid, current_project);

            let n = request.instructions.len();
            let mut outputs: Vec<Option<StepOutput>> = vec![None; n];
            let mut results: Vec<StepResult> = Vec::with_capacity(n);
            let mut failed: Vec<bool> = vec![false; n];

            // Begin undo block if requested
            if let Some(ref label) = request.options.undo_label {
                let rctx = resolve_project_context(&ProjectContext::Current);
                Reaper::get().medium_reaper().undo_begin_block_2(rctx);
                UNDO_LABEL.with(|cell| cell.replace(Some(label.clone())));
            }

            for instruction in &request.instructions {
                let step = instruction.step as usize;

                // Check dependencies — skip if any dependency failed
                let deps = instruction.op.step_dependencies();
                let failed_dep = deps.iter().find(|&&d| {
                    let d = d as usize;
                    d < failed.len() && failed[d]
                });

                if let Some(&dep) = failed_dep {
                    if step < n {
                        failed[step] = true;
                    }
                    results.push(StepResult {
                        step: instruction.step,
                        outcome: StepOutcome::Skipped(dep),
                    });
                    continue;
                }

                let result = dispatch_sync::dispatch_op_sync(&instruction.op, &outputs, &services);

                match result {
                    Ok(output) => {
                        if step < n {
                            outputs[step] = Some(output.clone());
                        }
                        results.push(StepResult {
                            step: instruction.step,
                            outcome: StepOutcome::Ok(output),
                        });
                    }
                    Err(msg) => {
                        if step < n {
                            failed[step] = true;
                        }
                        results.push(StepResult {
                            step: instruction.step,
                            outcome: StepOutcome::Error(msg),
                        });

                        if request.options.fail_fast {
                            for remaining in request.instructions.iter().skip(results.len()) {
                                results.push(StepResult {
                                    step: remaining.step,
                                    outcome: StepOutcome::Skipped(instruction.step),
                                });
                            }
                            break;
                        }
                    }
                }
            }

            // End undo block if we started one
            if let Some(ref label) = request.options.undo_label {
                let rctx = resolve_project_context(&ProjectContext::Current);
                Reaper::get().medium_reaper().undo_end_block_2(
                    rctx,
                    label.as_str(),
                    UndoScope::All,
                );
            }

            clear_project_cache();
            BatchResponse { results }
        })
        .await
        .unwrap_or_else(|| BatchResponse { results: vec![] })
    }

    /// Execute using the async path (per-op main_thread::query, ~33ms each).
    async fn execute_async(&self, request: BatchRequest) -> BatchResponse {
        use daw_proto::ProjectService;

        let s = &self.inner;
        let n = request.instructions.len();
        let mut outputs: Vec<Option<StepOutput>> = vec![None; n];
        let mut results: Vec<StepResult> = Vec::with_capacity(n);
        let mut failed: Vec<bool> = vec![false; n];

        if let Some(ref label) = request.options.undo_label {
            s.project_svc
                .begin_undo_block(daw_proto::ProjectContext::Current, label.clone())
                .await;
        }

        for instruction in &request.instructions {
            let step = instruction.step as usize;

            let deps = instruction.op.step_dependencies();
            let failed_dep = deps.iter().find(|&&d| {
                let d = d as usize;
                d < failed.len() && failed[d]
            });

            if let Some(&dep) = failed_dep {
                if step < n {
                    failed[step] = true;
                }
                results.push(StepResult {
                    step: instruction.step,
                    outcome: StepOutcome::Skipped(dep),
                });
                continue;
            }

            let result = dispatch::dispatch_op(
                &instruction.op,
                &outputs,
                &s.project_svc,
                &s.transport_svc,
                &s.track_svc,
                &s.fx_svc,
                &s.routing_svc,
                &s.item_svc,
                &s.take_svc,
                &s.marker_svc,
                &s.region_svc,
                &s.tempo_map_svc,
                &s.midi_svc,
                &s.live_midi_svc,
                &s.ext_state_svc,
                &s.audio_engine_svc,
                &s.position_svc,
                &s.health_svc,
                &s.action_registry_svc,
                &s.toolbar_svc,
                &s.plugin_loader_svc,
                &s.peak_svc,
                &s.resource_svc,
                &s.audio_accessor_svc,
                &s.midi_analysis_svc,
            )
            .await;

            match result {
                Ok(output) => {
                    if step < n {
                        outputs[step] = Some(output.clone());
                    }
                    results.push(StepResult {
                        step: instruction.step,
                        outcome: StepOutcome::Ok(output),
                    });
                }
                Err(msg) => {
                    if step < n {
                        failed[step] = true;
                    }
                    results.push(StepResult {
                        step: instruction.step,
                        outcome: StepOutcome::Error(msg),
                    });

                    if request.options.fail_fast {
                        for remaining in request.instructions.iter().skip(results.len()) {
                            results.push(StepResult {
                                step: remaining.step,
                                outcome: StepOutcome::Skipped(instruction.step),
                            });
                        }
                        break;
                    }
                }
            }
        }

        if let Some(ref label) = request.options.undo_label {
            s.project_svc
                .end_undo_block(daw_proto::ProjectContext::Current, label.clone(), None)
                .await;
        }

        BatchResponse { results }
    }
}

impl daw_proto::batch::BatchService for BatchExecutor {
    async fn execute(&self, request: BatchRequest) -> BatchResponse {
        let n = request.instructions.len();
        let use_sync = Self::all_ops_sync_supported(&request);

        tracing::info!(
            "BatchExecutor::execute — {} instructions, path={}",
            n,
            if use_sync { "sync" } else { "async" }
        );

        if use_sync {
            self.execute_sync(request).await
        } else {
            self.execute_async(request).await
        }
    }
}

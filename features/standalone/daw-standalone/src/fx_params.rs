//! `impl FxParams for Standalone` — post-architect::rpc port.
//!
//! Backed by `ProjectState::fx_chains` — each `FxEntry` carries a
//! `params: HashMap<u32, f64>` of normalized parameter values.

use daw_proto::FxParams;
use daw_proto::{DawError, DawResult, FxChainContext, FxParameter};

use crate::sync::{FxChainKey, ProjectState, Standalone};

fn resolve_project(daw: &Standalone) -> Option<String> {
    let state = daw.state.lock().ok()?;
    state.current_project_guid.clone()
}

fn no_project() -> DawError {
    DawError::not_found("Project", "current")
}

fn ctx_label(ctx: &FxChainContext) -> String {
    match ctx {
        FxChainContext::Track(g) => format!("Track({g})"),
        FxChainContext::Input(g) => format!("Input({g})"),
        FxChainContext::Monitoring => "Monitoring".to_string(),
    }
}

fn entry<'p>(
    p: &'p ProjectState,
    ctx: &FxChainContext,
    fx_idx: u32,
) -> Option<&'p crate::sync::FxEntry> {
    p.fx_chains
        .get(&FxChainKey::from(ctx))
        .and_then(|chain| chain.get(fx_idx as usize))
}

impl FxParams for Standalone {
    fn count(&self, ctx: FxChainContext, fx_idx: u32) -> u32 {
        let Some(guid) = resolve_project(self) else {
            return 0;
        };
        self.with_project(&guid, |p| {
            entry(p, &ctx, fx_idx)
                .map(|e| e.fx.parameter_count)
                .unwrap_or(0)
        })
        .unwrap_or(0)
    }

    fn get(&self, ctx: FxChainContext, fx_idx: u32, param_idx: u32) -> Option<f64> {
        let guid = resolve_project(self)?;
        self.with_project(&guid, |p| {
            entry(p, &ctx, fx_idx).and_then(|e| e.params.get(&param_idx).copied())
        })
        .ok()
        .flatten()
    }

    fn set(&self, ctx: FxChainContext, fx_idx: u32, param_idx: u32, value: f64) -> DawResult<()> {
        let guid = resolve_project(self).ok_or_else(no_project)?;
        self.with_project_mut(&guid, |p| {
            let chain = p
                .fx_chains
                .get_mut(&FxChainKey::from(&ctx))
                .ok_or_else(|| DawError::not_found("FxChain", &ctx_label(&ctx)))?;
            let e = chain
                .get_mut(fx_idx as usize)
                .ok_or_else(|| DawError::not_found("Fx", &fx_idx.to_string()))?;
            e.params.insert(param_idx, value);
            if param_idx + 1 > e.fx.parameter_count {
                e.fx.parameter_count = param_idx + 1;
            }
            Ok::<(), DawError>(())
        })?
    }

    fn name(&self, ctx: FxChainContext, fx_idx: u32, param_idx: u32) -> Option<String> {
        let guid = resolve_project(self)?;
        self.with_project(&guid, |p| {
            entry(p, &ctx, fx_idx).map(|_| format!("Param {param_idx}"))
        })
        .ok()
        .flatten()
    }

    fn info(&self, ctx: FxChainContext, fx_idx: u32, param_idx: u32) -> Option<FxParameter> {
        let guid = resolve_project(self)?;
        self.with_project(&guid, |p| {
            entry(p, &ctx, fx_idx).map(|e| {
                let value = e.params.get(&param_idx).copied().unwrap_or(0.0);
                FxParameter::new(param_idx, format!("Param {param_idx}"), value)
            })
        })
        .ok()
        .flatten()
    }
}

//! Standalone FX implementation

use daw_proto::{
    AddFxAtRequest, Fx, FxChainContext, FxLatency, FxParamModulation, FxParameter, FxService,
    FxTarget, ProjectContext, SetNamedConfigRequest, SetParameterByNameRequest,
    SetParameterRequest,
};
use roam::Context;
use uuid::Uuid;

/// Standalone FX service implementation.
///
/// Provides stub implementations for FX operations (no actual FX processing).
#[derive(Clone, Default)]
pub struct StandaloneFx;

impl StandaloneFx {
    pub fn new() -> Self {
        Self
    }
}

impl FxService for StandaloneFx {
    async fn get_fx_list(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
    ) -> Vec<Fx> {
        vec![]
    }

    async fn get_fx(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Option<Fx> {
        None
    }

    async fn fx_count(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
    ) -> u32 {
        0
    }

    async fn set_fx_enabled(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _enabled: bool,
    ) {
    }

    async fn set_fx_offline(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _offline: bool,
    ) {
    }

    async fn add_fx(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
        _name: String,
    ) -> Option<String> {
        // Return a fake GUID
        Some(Uuid::new_v4().to_string())
    }

    async fn add_fx_at(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _request: AddFxAtRequest,
    ) -> Option<String> {
        Some(Uuid::new_v4().to_string())
    }

    async fn remove_fx(&self, _cx: &Context, _project: ProjectContext, _target: FxTarget) {}

    async fn move_fx(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _new_index: u32,
    ) {
    }

    async fn get_parameters(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Vec<FxParameter> {
        vec![]
    }

    async fn get_parameter(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _index: u32,
    ) -> Option<FxParameter> {
        None
    }

    async fn set_parameter(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _request: SetParameterRequest,
    ) {
    }

    async fn get_parameter_by_name(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _name: String,
    ) -> Option<FxParameter> {
        None
    }

    async fn set_parameter_by_name(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _request: SetParameterByNameRequest,
    ) {
    }

    async fn next_preset(&self, _cx: &Context, _project: ProjectContext, _target: FxTarget) {}

    async fn prev_preset(&self, _cx: &Context, _project: ProjectContext, _target: FxTarget) {}

    async fn set_preset(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _index: u32,
    ) {
    }

    async fn open_fx_ui(&self, _cx: &Context, _project: ProjectContext, _target: FxTarget) {}

    async fn close_fx_ui(&self, _cx: &Context, _project: ProjectContext, _target: FxTarget) {}

    async fn toggle_fx_ui(&self, _cx: &Context, _project: ProjectContext, _target: FxTarget) {}

    async fn get_named_config(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _key: String,
    ) -> Option<String> {
        None
    }

    async fn set_named_config(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _request: SetNamedConfigRequest,
    ) {
    }

    async fn get_fx_latency(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> FxLatency {
        FxLatency::default()
    }

    async fn get_param_modulation(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _param_index: u32,
    ) -> FxParamModulation {
        FxParamModulation::default()
    }
}

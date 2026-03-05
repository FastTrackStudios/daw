//! Standalone FX implementation

use daw_proto::{
    AddFxAtRequest, CreateContainerRequest, EncloseInContainerRequest, Fx, FxChainContext,
    FxChannelConfig, FxContainerChannelConfig, FxEvent, FxLatency, FxNodeId, FxParamModulation,
    FxParameter, FxPinMappings, FxPresetIndex, FxRoutingMode, FxService, FxStateChunk, FxTarget,
    FxTree, InstalledFx, MoveFromContainerRequest, MoveToContainerRequest, ProjectContext,
    SetContainerChannelConfigRequest, SetNamedConfigRequest, SetParameterByNameRequest,
    SetParameterRequest,
};
use roam::{Context, Tx};
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
    async fn list_installed_fx(&self, _cx: &Context) -> Vec<InstalledFx> {
        vec![]
    }

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
    ) -> Result<(), String> {
        Ok(())
    }

    async fn set_fx_offline(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _offline: bool,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn add_fx(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
        _name: String,
    ) -> Option<String> {
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

    async fn remove_fx(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn move_fx(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _new_index: u32,
    ) -> Result<(), String> {
        Ok(())
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
    ) -> Result<(), String> {
        Ok(())
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
    ) -> Result<(), String> {
        Ok(())
    }

    async fn get_preset_index(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Option<FxPresetIndex> {
        None
    }

    async fn next_preset(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn prev_preset(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn set_preset(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _index: u32,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn open_fx_ui(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn close_fx_ui(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn toggle_fx_ui(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Result<(), String> {
        Ok(())
    }

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
    ) -> Result<(), String> {
        Ok(())
    }

    async fn get_fx_latency(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Option<FxLatency> {
        None
    }

    async fn get_param_modulation(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _param_index: u32,
    ) -> Option<FxParamModulation> {
        None
    }

    async fn get_fx_state_chunk(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Option<Vec<u8>> {
        None
    }

    async fn set_fx_state_chunk(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _chunk: Vec<u8>,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn get_fx_state_chunk_encoded(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Option<String> {
        None
    }

    async fn set_fx_state_chunk_encoded(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _encoded: String,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn get_fx_chain_state(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
    ) -> Vec<FxStateChunk> {
        vec![]
    }

    async fn set_fx_chain_state(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
        _chunks: Vec<FxStateChunk>,
    ) -> Result<(), String> {
        Ok(())
    }

    // =========================================================================
    // Container / Tree Operations (stubs)
    // =========================================================================

    async fn get_fx_tree(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
    ) -> FxTree {
        FxTree::new()
    }

    async fn create_container(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _request: CreateContainerRequest,
    ) -> Option<FxNodeId> {
        None
    }

    async fn move_to_container(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _request: MoveToContainerRequest,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn move_from_container(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _request: MoveFromContainerRequest,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn set_routing_mode(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
        _node_id: FxNodeId,
        _mode: FxRoutingMode,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn get_container_channel_config(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
        _container_id: FxNodeId,
    ) -> Option<FxContainerChannelConfig> {
        None
    }

    async fn set_container_channel_config(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _request: SetContainerChannelConfigRequest,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn get_fx_channel_config(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Option<FxChannelConfig> {
        None
    }

    async fn set_fx_channel_config(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _config: FxChannelConfig,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn silence_fx_output(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
    ) -> Result<FxPinMappings, String> {
        Ok(FxPinMappings::default())
    }

    async fn restore_fx_output(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _target: FxTarget,
        _saved: FxPinMappings,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn enclose_in_container(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _request: EncloseInContainerRequest,
    ) -> Option<FxNodeId> {
        None
    }

    async fn explode_container(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
        _container_id: FxNodeId,
    ) -> Result<(), String> {
        Ok(())
    }

    async fn rename_container(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
        _container_id: FxNodeId,
        _name: String,
    ) -> Result<(), String> {
        Ok(())
    }

    // =========================================================================
    // Raw Chunk Text Operations
    // =========================================================================

    async fn get_fx_chain_chunk_text(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
    ) -> Option<String> {
        None
    }

    async fn insert_fx_chain_chunk(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
        _chunk_text: String,
    ) -> Result<(), String> {
        Ok(())
    }

    // =========================================================================
    // Observation / Subscriptions
    // =========================================================================

    async fn subscribe_fx_events(
        &self,
        _cx: &Context,
        _project: ProjectContext,
        _context: FxChainContext,
        _events: Tx<FxEvent>,
    ) {
        // Standalone: no FX events to observe
    }
}

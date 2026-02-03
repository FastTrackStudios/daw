//! FX service trait
//!
//! Defines the RPC interface for FX (audio plugin) operations.

use super::{
    AddFxAtRequest, Fx, FxChainContext, FxLatency, FxParamModulation, FxParameter, FxTarget,
    SetNamedConfigRequest, SetParameterByNameRequest, SetParameterRequest,
};
use crate::ProjectContext;
use roam::service;

/// Service for managing FX (audio plugins) in a DAW project
///
/// FX are audio processing plugins (VST, AU, JS, CLAP) that can be
/// inserted into track FX chains for processing audio.
#[service]
pub trait FxService {
    // =========================================================================
    // Chain Queries
    // =========================================================================

    /// Get all FX in a chain
    async fn get_fx_list(&self, project: ProjectContext, context: FxChainContext) -> Vec<Fx>;

    /// Get a specific FX by reference
    async fn get_fx(&self, project: ProjectContext, target: FxTarget) -> Option<Fx>;

    /// Get the number of FX in a chain
    async fn fx_count(&self, project: ProjectContext, context: FxChainContext) -> u32;

    // =========================================================================
    // FX State
    // =========================================================================

    /// Enable or bypass an FX
    async fn set_fx_enabled(&self, project: ProjectContext, target: FxTarget, enabled: bool);

    /// Set FX offline state (completely disable processing)
    async fn set_fx_offline(&self, project: ProjectContext, target: FxTarget, offline: bool);

    // =========================================================================
    // FX Management
    // =========================================================================

    /// Add an FX to the end of a chain
    ///
    /// Returns the GUID of the newly added FX, or None if the plugin wasn't found.
    ///
    /// # Examples
    /// - `"ReaComp"` - Add by short name
    /// - `"VST3: FabFilter Pro-C 2"` - Add with type prefix
    /// - `"JS: 1175 Compressor"` - Add JS effect
    async fn add_fx(
        &self,
        project: ProjectContext,
        context: FxChainContext,
        name: String,
    ) -> Option<String>;

    /// Add an FX at a specific position in the chain
    async fn add_fx_at(&self, project: ProjectContext, request: AddFxAtRequest) -> Option<String>;

    /// Remove an FX from the chain
    async fn remove_fx(&self, project: ProjectContext, target: FxTarget);

    /// Move an FX to a new position in the chain
    async fn move_fx(&self, project: ProjectContext, target: FxTarget, new_index: u32);

    // =========================================================================
    // Parameters
    // =========================================================================

    /// Get all parameters for an FX
    async fn get_parameters(&self, project: ProjectContext, target: FxTarget) -> Vec<FxParameter>;

    /// Get a specific parameter by index
    async fn get_parameter(
        &self,
        project: ProjectContext,
        target: FxTarget,
        index: u32,
    ) -> Option<FxParameter>;

    /// Set a parameter value by index (normalized 0.0-1.0)
    async fn set_parameter(&self, project: ProjectContext, request: SetParameterRequest);

    /// Get a parameter by name (first match)
    async fn get_parameter_by_name(
        &self,
        project: ProjectContext,
        target: FxTarget,
        name: String,
    ) -> Option<FxParameter>;

    /// Set a parameter value by name (normalized 0.0-1.0)
    async fn set_parameter_by_name(
        &self,
        project: ProjectContext,
        request: SetParameterByNameRequest,
    );

    // =========================================================================
    // Presets
    // =========================================================================

    /// Navigate to the next preset
    async fn next_preset(&self, project: ProjectContext, target: FxTarget);

    /// Navigate to the previous preset
    async fn prev_preset(&self, project: ProjectContext, target: FxTarget);

    /// Set preset by index
    async fn set_preset(&self, project: ProjectContext, target: FxTarget, index: u32);

    // =========================================================================
    // UI
    // =========================================================================

    /// Open the FX UI window
    async fn open_fx_ui(&self, project: ProjectContext, target: FxTarget);

    /// Close the FX UI window
    async fn close_fx_ui(&self, project: ProjectContext, target: FxTarget);

    /// Toggle the FX UI window
    async fn toggle_fx_ui(&self, project: ProjectContext, target: FxTarget);

    // =========================================================================
    // Advanced (Named Config Parameters)
    // =========================================================================

    /// Get a named configuration parameter (TrackFX_GetNamedConfigParm)
    ///
    /// Named config params provide access to plugin-specific settings
    /// that aren't exposed as regular parameters.
    async fn get_named_config(
        &self,
        project: ProjectContext,
        target: FxTarget,
        key: String,
    ) -> Option<String>;

    /// Set a named configuration parameter (TrackFX_SetNamedConfigParm)
    async fn set_named_config(&self, project: ProjectContext, request: SetNamedConfigRequest);

    /// Get FX latency information (PDC)
    async fn get_fx_latency(&self, project: ProjectContext, target: FxTarget) -> FxLatency;

    /// Get parameter modulation state
    async fn get_param_modulation(
        &self,
        project: ProjectContext,
        target: FxTarget,
        param_index: u32,
    ) -> FxParamModulation;
}

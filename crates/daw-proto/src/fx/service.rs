//! FX service trait
//!
//! Defines the RPC interface for FX (audio plugin) operations.
//!
//! ## Return Type Conventions
//!
//! - **Query methods** (`get_*`, `fx_count`): Return `Option<T>` when the target
//!   may not exist (single item lookups), or `Vec<T>` / plain `T` for
//!   collection queries and counts where "not found" is just "empty".
//! - **Mutation methods** (`set_*`, `move_*`, `remove_*`, `create_*`, etc.):
//!   Return `Result<(), String>` (or `Result<T, String>` when producing a value)
//!   so callers can detect and handle failures.
//! - **Subscription methods**: Return `Result<(), String>` for setup failures.

use super::{
    AddFxAtRequest, CreateContainerRequest, EncloseInContainerRequest, Fx, FxChainContext,
    FxContainerChannelConfig, FxEvent, FxLatency, FxNodeId, FxParamModulation, FxParameter,
    FxRoutingMode, FxStateChunk, FxTarget, FxTree, MoveFromContainerRequest,
    MoveToContainerRequest, SetContainerChannelConfigRequest, SetNamedConfigRequest,
    SetParameterByNameRequest, SetParameterRequest,
};
use crate::ProjectContext;
use roam::Tx;
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
    async fn set_fx_enabled(
        &self,
        project: ProjectContext,
        target: FxTarget,
        enabled: bool,
    ) -> Result<(), String>;

    /// Set FX offline state (completely disable processing)
    async fn set_fx_offline(
        &self,
        project: ProjectContext,
        target: FxTarget,
        offline: bool,
    ) -> Result<(), String>;

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
    async fn remove_fx(&self, project: ProjectContext, target: FxTarget) -> Result<(), String>;

    /// Move an FX to a new position in the chain
    async fn move_fx(
        &self,
        project: ProjectContext,
        target: FxTarget,
        new_index: u32,
    ) -> Result<(), String>;

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
    async fn set_parameter(
        &self,
        project: ProjectContext,
        request: SetParameterRequest,
    ) -> Result<(), String>;

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
    ) -> Result<(), String>;

    // =========================================================================
    // Presets
    // =========================================================================

    /// Navigate to the next preset
    async fn next_preset(&self, project: ProjectContext, target: FxTarget) -> Result<(), String>;

    /// Navigate to the previous preset
    async fn prev_preset(&self, project: ProjectContext, target: FxTarget) -> Result<(), String>;

    /// Set preset by index
    async fn set_preset(
        &self,
        project: ProjectContext,
        target: FxTarget,
        index: u32,
    ) -> Result<(), String>;

    // =========================================================================
    // UI
    // =========================================================================

    /// Open the FX UI window
    async fn open_fx_ui(&self, project: ProjectContext, target: FxTarget) -> Result<(), String>;

    /// Close the FX UI window
    async fn close_fx_ui(&self, project: ProjectContext, target: FxTarget) -> Result<(), String>;

    /// Toggle the FX UI window
    async fn toggle_fx_ui(&self, project: ProjectContext, target: FxTarget) -> Result<(), String>;

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
    async fn set_named_config(
        &self,
        project: ProjectContext,
        request: SetNamedConfigRequest,
    ) -> Result<(), String>;

    /// Get FX latency information (PDC)
    async fn get_fx_latency(&self, project: ProjectContext, target: FxTarget) -> Option<FxLatency>;

    /// Get parameter modulation state
    async fn get_param_modulation(
        &self,
        project: ProjectContext,
        target: FxTarget,
        param_index: u32,
    ) -> Option<FxParamModulation>;

    // =========================================================================
    // State Chunks
    //
    // Binary plugin state capture and restore.
    //
    // Following Track Snapshot's approach: full binary state round-trip via
    // REAPER's native chunk format. Individual FX chunks use vst_chunk (base64).
    // Full chain chunks capture all FX state in one operation.
    //
    // Reference: FTS-GUITAR/Scripts/Daniel Lumertz Scripts/Tracks/Track Snapshot
    // =========================================================================

    /// Get the binary state chunk for a single FX (decoded bytes).
    ///
    /// This captures the complete plugin state including internal state
    /// not exposed as parameters. Uses REAPER's `vst_chunk` named config.
    async fn get_fx_state_chunk(
        &self,
        project: ProjectContext,
        target: FxTarget,
    ) -> Option<Vec<u8>>;

    /// Set the binary state chunk for a single FX (decoded bytes).
    ///
    /// Restores complete plugin state. The FX must already exist in the chain.
    async fn set_fx_state_chunk(
        &self,
        project: ProjectContext,
        target: FxTarget,
        chunk: Vec<u8>,
    ) -> Result<(), String>;

    /// Get the base64-encoded state chunk for a single FX.
    ///
    /// More efficient than `get_fx_state_chunk` when the data will be
    /// serialized (avoids decode + re-encode round-trip).
    async fn get_fx_state_chunk_encoded(
        &self,
        project: ProjectContext,
        target: FxTarget,
    ) -> Option<String>;

    /// Set the base64-encoded state chunk for a single FX.
    async fn set_fx_state_chunk_encoded(
        &self,
        project: ProjectContext,
        target: FxTarget,
        encoded: String,
    ) -> Result<(), String>;

    /// Capture state chunks for all FX in a chain.
    ///
    /// Returns a list of (fx_guid, base64_encoded_chunk) pairs in chain order.
    /// Like Track Snapshot, this captures the full binary state of every plugin.
    async fn get_fx_chain_state(
        &self,
        project: ProjectContext,
        context: FxChainContext,
    ) -> Vec<FxStateChunk>;

    /// Restore state chunks for all FX in a chain.
    ///
    /// Matches FX by GUID and applies state chunks. FX not found in the
    /// chain are skipped (graceful handling of missing FX, like Track Snapshot).
    async fn set_fx_chain_state(
        &self,
        project: ProjectContext,
        context: FxChainContext,
        chunks: Vec<FxStateChunk>,
    ) -> Result<(), String>;

    // =========================================================================
    // Container / Tree Operations
    //
    // Tree-aware methods for reading and manipulating FX containers.
    // All container methods use FxNodeId for addressing — never raw
    // REAPER indices. The tree model abstracts the DAW's internal
    // encoding completely.
    // =========================================================================

    /// Get the full FX chain as a recursive tree.
    ///
    /// Returns the complete hierarchy including containers, their children,
    /// routing modes, and channel configurations. This is the primary method
    /// for reading container-aware FX chain state.
    async fn get_fx_tree(&self, project: ProjectContext, context: FxChainContext) -> FxTree;

    /// Create a new empty container at a position in the chain.
    ///
    /// Returns the `FxNodeId` of the newly created container.
    async fn create_container(
        &self,
        project: ProjectContext,
        request: CreateContainerRequest,
    ) -> Option<FxNodeId>;

    /// Move an FX node into a container at a specified child position.
    async fn move_to_container(
        &self,
        project: ProjectContext,
        request: MoveToContainerRequest,
    ) -> Result<(), String>;

    /// Move an FX node out of its container to a parent-level position.
    async fn move_from_container(
        &self,
        project: ProjectContext,
        request: MoveFromContainerRequest,
    ) -> Result<(), String>;

    /// Set the routing mode (serial/parallel) for FX within a container.
    async fn set_routing_mode(
        &self,
        project: ProjectContext,
        context: FxChainContext,
        node_id: FxNodeId,
        mode: FxRoutingMode,
    ) -> Result<(), String>;

    /// Get the channel configuration for a container.
    async fn get_container_channel_config(
        &self,
        project: ProjectContext,
        context: FxChainContext,
        container_id: FxNodeId,
    ) -> Option<FxContainerChannelConfig>;

    /// Set the channel configuration for a container.
    async fn set_container_channel_config(
        &self,
        project: ProjectContext,
        request: SetContainerChannelConfigRequest,
    ) -> Result<(), String>;

    /// Enclose one or more existing FX nodes in a new container.
    ///
    /// Creates a container at the position of the first specified node,
    /// then moves all specified nodes into it. Returns the new container's
    /// `FxNodeId`.
    async fn enclose_in_container(
        &self,
        project: ProjectContext,
        request: EncloseInContainerRequest,
    ) -> Option<FxNodeId>;

    /// Explode a container — moves all children out to the parent level,
    /// then removes the empty container.
    async fn explode_container(
        &self,
        project: ProjectContext,
        context: FxChainContext,
        container_id: FxNodeId,
    ) -> Result<(), String>;

    /// Rename a container.
    async fn rename_container(
        &self,
        project: ProjectContext,
        context: FxChainContext,
        container_id: FxNodeId,
        name: String,
    ) -> Result<(), String>;

    // =========================================================================
    // Raw Chunk Text Operations
    //
    // Track Snapshot-style operations that work with raw RPP chunk text.
    // Used for atomic module preset save/load — captures the full container
    // block including nested FX, their binary state, routing, and config.
    // =========================================================================

    /// Get the raw RPP chunk text for the entire FX chain section.
    ///
    /// Returns the `<FXCHAIN ...>...</FXCHAIN>` block as a string from the
    /// track's state chunk. Returns `None` if the track has no FX chain.
    async fn get_fx_chain_chunk_text(
        &self,
        project: ProjectContext,
        context: FxChainContext,
    ) -> Option<String>;

    /// Insert a raw RPP chunk block into the FX chain.
    ///
    /// The `chunk_text` should be a complete RPP block (e.g., a `<CONTAINER>`
    /// block with nested FX). It is appended at the end of the existing chain.
    /// REAPER handles all plugin instantiation and state restoration atomically.
    async fn insert_fx_chain_chunk(
        &self,
        project: ProjectContext,
        context: FxChainContext,
        chunk_text: String,
    ) -> Result<(), String>;

    // =========================================================================
    // Observation / Subscriptions
    //
    // Reactive push-based FX state observation.
    // Follows the same pattern as TransportService::subscribe_state.
    // =========================================================================

    /// Subscribe to FX chain events for a specific chain.
    ///
    /// Events include parameter changes, FX add/remove/reorder,
    /// enable/bypass changes, and preset changes. The subscription
    /// delivers events through a unified `FxEvent` stream.
    ///
    /// The subscriber receives events until the sender is dropped
    /// or the subscriber disconnects.
    async fn subscribe_fx_events(
        &self,
        project: ProjectContext,
        context: FxChainContext,
        events: Tx<FxEvent>,
    );
}

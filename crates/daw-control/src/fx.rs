//! FX chain, FX handle, and parameter handle

use std::sync::Arc;

use crate::DawClients;
use daw_proto::{
    AddFxAtRequest, Fx, FxChainContext, FxLatency, FxParamModulation, FxParameter, FxRef, FxTarget,
    ProjectContext, SetNamedConfigRequest, SetParameterByNameRequest, SetParameterRequest,
};
use eyre::Result;

/// Handle to an FX chain
///
/// Represents a chain of audio effects that can be on a track (output or input)
/// or in the monitoring path.
///
/// # Example
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
///
/// // Get a track's FX chain
/// let track = project.tracks().by_name("Vocals").await?.unwrap();
/// let chain = track.fx_chain();
///
/// // Enumerate FX
/// for fx in chain.all().await? {
///     println!("FX: {} ({:?})", fx.name, fx.plugin_type);
/// }
///
/// // Add and configure FX
/// let compressor = chain.add("ReaComp").await?;
/// compressor.param_by_name("Threshold").set(0.3).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct FxChain {
    context: FxChainContext,
    project_id: String,
    clients: Arc<DawClients>,
}

impl FxChain {
    /// Create a new FX chain handle
    pub(crate) fn new(
        context: FxChainContext,
        project_id: String,
        clients: Arc<DawClients>,
    ) -> Self {
        Self {
            context,
            project_id,
            clients,
        }
    }

    /// Helper to create project context
    fn project_context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Get all FX in the chain
    pub async fn all(&self) -> Result<Vec<Fx>> {
        let fx_list = self
            .clients
            .fx
            .get_fx_list(self.project_context(), self.context.clone())
            .await?;
        Ok(fx_list)
    }

    /// Get FX by index
    pub async fn by_index(&self, index: u32) -> Result<Option<FxHandle>> {
        let target = FxTarget::new(self.context.clone(), FxRef::Index(index));
        let fx = self
            .clients
            .fx
            .get_fx(self.project_context(), target)
            .await?;

        Ok(fx.map(|f| {
            FxHandle::new(
                self.context.clone(),
                f.guid,
                self.project_id.clone(),
                self.clients.clone(),
            )
        }))
    }

    /// Get FX by GUID
    pub async fn by_guid(&self, guid: &str) -> Result<Option<FxHandle>> {
        let target = FxTarget::new(self.context.clone(), FxRef::Guid(guid.to_string()));
        let fx = self
            .clients
            .fx
            .get_fx(self.project_context(), target)
            .await?;

        Ok(fx.map(|f| {
            FxHandle::new(
                self.context.clone(),
                f.guid,
                self.project_id.clone(),
                self.clients.clone(),
            )
        }))
    }

    /// Get FX by name (first match)
    pub async fn by_name(&self, name: &str) -> Result<Option<FxHandle>> {
        let target = FxTarget::new(self.context.clone(), FxRef::Name(name.to_string()));
        let fx = self
            .clients
            .fx
            .get_fx(self.project_context(), target)
            .await?;

        Ok(fx.map(|f| {
            FxHandle::new(
                self.context.clone(),
                f.guid,
                self.project_id.clone(),
                self.clients.clone(),
            )
        }))
    }

    /// Get FX count
    pub async fn count(&self) -> Result<u32> {
        let count = self
            .clients
            .fx
            .fx_count(self.project_context(), self.context.clone())
            .await?;
        Ok(count)
    }

    // =========================================================================
    // FX Management
    // =========================================================================

    /// Add an FX to the end of the chain
    ///
    /// Returns a handle to the newly added FX.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use daw_control::FxChain;
    /// # async fn example(chain: FxChain) -> eyre::Result<()> {
    /// chain.add("ReaComp").await?;
    /// chain.add("VST3: FabFilter Pro-C 2").await?;
    /// chain.add("JS: 1175 Compressor").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn add(&self, fx_name: &str) -> Result<FxHandle> {
        let guid = self
            .clients
            .fx
            .add_fx(
                self.project_context(),
                self.context.clone(),
                fx_name.to_string(),
            )
            .await?
            .ok_or_else(|| eyre::eyre!("Failed to add FX: {}", fx_name))?;

        Ok(FxHandle::new(
            self.context.clone(),
            guid,
            self.project_id.clone(),
            self.clients.clone(),
        ))
    }

    /// Add an FX at a specific position in the chain
    pub async fn add_at(&self, fx_name: &str, index: u32) -> Result<FxHandle> {
        let request = AddFxAtRequest {
            context: self.context.clone(),
            name: fx_name.to_string(),
            index,
        };

        let guid = self
            .clients
            .fx
            .add_fx_at(self.project_context(), request)
            .await?
            .ok_or_else(|| eyre::eyre!("Failed to add FX at index {}: {}", index, fx_name))?;

        Ok(FxHandle::new(
            self.context.clone(),
            guid,
            self.project_id.clone(),
            self.clients.clone(),
        ))
    }
}

impl std::fmt::Debug for FxChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FxChain")
            .field("context", &self.context)
            .field("project_id", &self.project_id)
            .finish()
    }
}

// =============================================================================
// FxHandle
// =============================================================================

/// Handle to a single FX instance
///
/// This handle represents a specific audio effect plugin. It's lightweight
/// and cheap to clone.
///
/// # Example
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
/// let track = project.tracks().by_name("Vocals").await?.unwrap();
///
/// // Get an FX
/// let compressor = track.fx_chain().by_name("ReaComp").await?.unwrap();
///
/// // Enable/disable
/// compressor.enable().await?;
/// compressor.toggle().await?;
///
/// // Parameters
/// compressor.param(0).set(0.5).await?;
/// compressor.param_by_name("Threshold").set(0.3).await?;
///
/// // Presets
/// compressor.next_preset().await?;
///
/// // UI
/// compressor.open_ui().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct FxHandle {
    context: FxChainContext,
    fx_guid: String,
    project_id: String,
    clients: Arc<DawClients>,
}

impl FxHandle {
    /// Create a new FX handle
    pub(crate) fn new(
        context: FxChainContext,
        fx_guid: String,
        project_id: String,
        clients: Arc<DawClients>,
    ) -> Self {
        Self {
            context,
            fx_guid,
            project_id,
            clients,
        }
    }

    /// Get the FX GUID
    pub fn guid(&self) -> &str {
        &self.fx_guid
    }

    /// Helper to create project context
    fn project_context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    /// Helper to create FX target
    fn target(&self) -> FxTarget {
        FxTarget::new(self.context.clone(), FxRef::Guid(self.fx_guid.clone()))
    }

    // =========================================================================
    // Info
    // =========================================================================

    /// Get full FX state
    pub async fn info(&self) -> Result<Fx> {
        self.clients
            .fx
            .get_fx(self.project_context(), self.target())
            .await?
            .ok_or_else(|| eyre::eyre!("FX not found: {}", self.fx_guid))
    }

    // =========================================================================
    // Enable/Bypass
    // =========================================================================

    /// Enable the FX (un-bypass)
    pub async fn enable(&self) -> Result<()> {
        self.clients
            .fx
            .set_fx_enabled(self.project_context(), self.target(), true)
            .await?;
        Ok(())
    }

    /// Disable/bypass the FX
    pub async fn disable(&self) -> Result<()> {
        self.clients
            .fx
            .set_fx_enabled(self.project_context(), self.target(), false)
            .await?;
        Ok(())
    }

    /// Toggle enabled state
    pub async fn toggle(&self) -> Result<()> {
        let info = self.info().await?;
        self.clients
            .fx
            .set_fx_enabled(self.project_context(), self.target(), !info.enabled)
            .await?;
        Ok(())
    }

    /// Check if FX is enabled
    pub async fn is_enabled(&self) -> Result<bool> {
        Ok(self.info().await?.enabled)
    }

    // =========================================================================
    // Parameters
    // =========================================================================

    /// Get all parameters
    pub async fn parameters(&self) -> Result<Vec<FxParameter>> {
        let params = self
            .clients
            .fx
            .get_parameters(self.project_context(), self.target())
            .await?;
        Ok(params)
    }

    /// Get a parameter handle by index
    pub fn param(&self, index: u32) -> FxParamHandle {
        FxParamHandle::new_by_index(
            self.context.clone(),
            self.fx_guid.clone(),
            index,
            self.project_id.clone(),
            self.clients.clone(),
        )
    }

    /// Get a parameter handle by name
    pub fn param_by_name(&self, name: &str) -> FxParamHandle {
        FxParamHandle::new_by_name(
            self.context.clone(),
            self.fx_guid.clone(),
            name.to_string(),
            self.project_id.clone(),
            self.clients.clone(),
        )
    }

    // =========================================================================
    // Presets
    // =========================================================================

    /// Navigate to the next preset
    pub async fn next_preset(&self) -> Result<()> {
        self.clients
            .fx
            .next_preset(self.project_context(), self.target())
            .await?;
        Ok(())
    }

    /// Navigate to the previous preset
    pub async fn prev_preset(&self) -> Result<()> {
        self.clients
            .fx
            .prev_preset(self.project_context(), self.target())
            .await?;
        Ok(())
    }

    /// Set preset by index
    pub async fn set_preset(&self, index: u32) -> Result<()> {
        self.clients
            .fx
            .set_preset(self.project_context(), self.target(), index)
            .await?;
        Ok(())
    }

    // =========================================================================
    // UI
    // =========================================================================

    /// Open the FX UI window
    pub async fn open_ui(&self) -> Result<()> {
        self.clients
            .fx
            .open_fx_ui(self.project_context(), self.target())
            .await?;
        Ok(())
    }

    /// Close the FX UI window
    pub async fn close_ui(&self) -> Result<()> {
        self.clients
            .fx
            .close_fx_ui(self.project_context(), self.target())
            .await?;
        Ok(())
    }

    /// Toggle the FX UI window
    pub async fn toggle_ui(&self) -> Result<()> {
        self.clients
            .fx
            .toggle_fx_ui(self.project_context(), self.target())
            .await?;
        Ok(())
    }

    // =========================================================================
    // Chain Operations
    // =========================================================================

    /// Remove this FX from the chain
    pub async fn remove(&self) -> Result<()> {
        self.clients
            .fx
            .remove_fx(self.project_context(), self.target())
            .await?;
        Ok(())
    }

    /// Move this FX to a new position in the chain
    pub async fn move_to(&self, index: u32) -> Result<()> {
        self.clients
            .fx
            .move_fx(self.project_context(), self.target(), index)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Advanced
    // =========================================================================

    /// Get FX latency information
    pub async fn latency(&self) -> Result<FxLatency> {
        let latency = self
            .clients
            .fx
            .get_fx_latency(self.project_context(), self.target())
            .await?;
        Ok(latency)
    }

    /// Get a named configuration parameter
    pub async fn get_config(&self, key: &str) -> Result<Option<String>> {
        let value = self
            .clients
            .fx
            .get_named_config(self.project_context(), self.target(), key.to_string())
            .await?;
        Ok(value)
    }

    /// Set a named configuration parameter
    pub async fn set_config(&self, key: &str, value: &str) -> Result<()> {
        let request = SetNamedConfigRequest {
            target: self.target(),
            key: key.to_string(),
            value: value.to_string(),
        };
        self.clients
            .fx
            .set_named_config(self.project_context(), request)
            .await?;
        Ok(())
    }
}

impl std::fmt::Debug for FxHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FxHandle")
            .field("fx_guid", &self.fx_guid)
            .field("context", &self.context)
            .field("project_id", &self.project_id)
            .finish()
    }
}

// =============================================================================
// FxParamHandle
// =============================================================================

/// Handle to an FX parameter
///
/// This handle represents a specific parameter on an FX. Parameters can be
/// referenced by index or by name.
///
/// # Example
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
/// let track = project.tracks().by_name("Vocals").await?.unwrap();
/// let compressor = track.fx_chain().by_name("ReaComp").await?.unwrap();
///
/// // By index
/// let threshold = compressor.param(0);
/// threshold.set(0.5).await?;
/// let value = threshold.get().await?;
///
/// // By name
/// let ratio = compressor.param_by_name("Ratio");
/// ratio.set(0.7).await?;
/// let formatted = ratio.formatted().await?;
///
/// // Modulation info
/// let mod_state = ratio.modulation().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct FxParamHandle {
    context: FxChainContext,
    fx_guid: String,
    param_ref: ParamRef,
    project_id: String,
    clients: Arc<DawClients>,
}

/// Internal reference to a parameter (by index or name)
#[derive(Clone, Debug)]
enum ParamRef {
    Index(u32),
    Name(String),
}

impl FxParamHandle {
    /// Create a parameter handle by index
    pub(crate) fn new_by_index(
        context: FxChainContext,
        fx_guid: String,
        index: u32,
        project_id: String,
        clients: Arc<DawClients>,
    ) -> Self {
        Self {
            context,
            fx_guid,
            param_ref: ParamRef::Index(index),
            project_id,
            clients,
        }
    }

    /// Create a parameter handle by name
    pub(crate) fn new_by_name(
        context: FxChainContext,
        fx_guid: String,
        name: String,
        project_id: String,
        clients: Arc<DawClients>,
    ) -> Self {
        Self {
            context,
            fx_guid,
            param_ref: ParamRef::Name(name),
            project_id,
            clients,
        }
    }

    /// Helper to create project context
    fn project_context(&self) -> ProjectContext {
        ProjectContext::Project(self.project_id.clone())
    }

    /// Helper to create FX target
    fn target(&self) -> FxTarget {
        FxTarget::new(self.context.clone(), FxRef::Guid(self.fx_guid.clone()))
    }

    // =========================================================================
    // Get/Set
    // =========================================================================

    /// Get the parameter state
    pub async fn info(&self) -> Result<FxParameter> {
        match &self.param_ref {
            ParamRef::Index(index) => self
                .clients
                .fx
                .get_parameter(self.project_context(), self.target(), *index)
                .await?
                .ok_or_else(|| eyre::eyre!("Parameter not found at index {}", index)),
            ParamRef::Name(name) => self
                .clients
                .fx
                .get_parameter_by_name(self.project_context(), self.target(), name.clone())
                .await?
                .ok_or_else(|| eyre::eyre!("Parameter not found: {}", name)),
        }
    }

    /// Get current value (normalized 0.0-1.0)
    pub async fn get(&self) -> Result<f64> {
        Ok(self.info().await?.value)
    }

    /// Set value (normalized 0.0-1.0)
    pub async fn set(&self, value: f64) -> Result<()> {
        match &self.param_ref {
            ParamRef::Index(index) => {
                let request = SetParameterRequest {
                    target: self.target(),
                    index: *index,
                    value,
                };
                self.clients
                    .fx
                    .set_parameter(self.project_context(), request)
                    .await?;
            }
            ParamRef::Name(name) => {
                let request = SetParameterByNameRequest {
                    target: self.target(),
                    name: name.clone(),
                    value,
                };
                self.clients
                    .fx
                    .set_parameter_by_name(self.project_context(), request)
                    .await?;
            }
        }
        Ok(())
    }

    /// Get formatted display value (e.g., "-12.5 dB")
    pub async fn formatted(&self) -> Result<String> {
        Ok(self.info().await?.formatted)
    }

    // =========================================================================
    // Advanced
    // =========================================================================

    /// Get modulation state (LFO, parameter linking)
    pub async fn modulation(&self) -> Result<FxParamModulation> {
        // Need to resolve index for modulation query
        let index = match &self.param_ref {
            ParamRef::Index(i) => *i,
            ParamRef::Name(_) => self.info().await?.index,
        };

        let mod_state = self
            .clients
            .fx
            .get_param_modulation(self.project_context(), self.target(), index)
            .await?;
        Ok(mod_state)
    }
}

impl std::fmt::Debug for FxParamHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FxParamHandle")
            .field("fx_guid", &self.fx_guid)
            .field("param_ref", &self.param_ref)
            .field("context", &self.context)
            .field("project_id", &self.project_id)
            .finish()
    }
}

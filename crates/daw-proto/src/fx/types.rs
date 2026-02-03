//! FX data types
//!
//! Types for representing audio effects (plugins) and their parameters.

use facet::Facet;

/// Context specifying which FX chain to operate on
///
/// FX can exist in different locations within a DAW:
/// - Track output chain (normal FX processing)
/// - Track input chain (recording FX)
/// - Monitoring chain (global monitoring FX)
#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum FxChainContext {
    /// Normal track FX (output/playback chain)
    Track(String), // track GUID
    /// Input/recording FX chain
    Input(String), // track GUID
    /// Monitoring FX (global, not per-track)
    Monitoring,
}

impl FxChainContext {
    /// Create a track FX chain context
    pub fn track(guid: impl Into<String>) -> Self {
        Self::Track(guid.into())
    }

    /// Create an input FX chain context
    pub fn input(guid: impl Into<String>) -> Self {
        Self::Input(guid.into())
    }

    /// Create a monitoring FX chain context
    pub fn monitoring() -> Self {
        Self::Monitoring
    }
}

/// Reference to an FX - how to identify an FX for operations
///
/// FX can be identified by GUID (stable), index (position-based),
/// or name (first match).
#[repr(u8)]
#[derive(Clone, Debug, Facet)]
pub enum FxRef {
    /// FX GUID - stable across sessions
    Guid(String),
    /// FX index (0-based position in chain)
    Index(u32),
    /// FX name - matches first FX with this name
    Name(String),
}

impl FxRef {
    /// Create a reference by GUID
    pub fn guid(guid: impl Into<String>) -> Self {
        Self::Guid(guid.into())
    }

    /// Create a reference by index
    pub fn index(index: u32) -> Self {
        Self::Index(index)
    }

    /// Create a reference by name
    pub fn name(name: impl Into<String>) -> Self {
        Self::Name(name.into())
    }
}

impl From<u32> for FxRef {
    fn from(index: u32) -> Self {
        Self::Index(index)
    }
}

impl From<&str> for FxRef {
    fn from(name: &str) -> Self {
        Self::Name(name.to_string())
    }
}

impl From<String> for FxRef {
    fn from(name: String) -> Self {
        Self::Name(name)
    }
}

/// FX plugin type
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Facet)]
pub enum FxType {
    /// VST2 plugin
    Vst2,
    /// VST3 plugin
    Vst3,
    /// Audio Unit (macOS)
    Au,
    /// REAPER JS effect
    Js,
    /// CLAP plugin
    Clap,
    /// Unknown or other type
    #[default]
    Unknown,
}

/// Complete FX state returned from queries
#[derive(Clone, Debug, Facet)]
pub struct Fx {
    /// Unique GUID for stable identification
    pub guid: String,
    /// Index in the FX chain (0-based)
    pub index: u32,
    /// Display name (may include preset name)
    pub name: String,
    /// Plugin name (e.g., "ReaComp", "Pro-C 2")
    pub plugin_name: String,
    /// Plugin type
    pub plugin_type: FxType,
    /// Whether the FX is enabled (not bypassed)
    pub enabled: bool,
    /// Whether the FX is offline (not processing)
    pub offline: bool,
    /// Whether the FX UI window is open
    pub window_open: bool,
    /// Number of parameters
    pub parameter_count: u32,
    /// Current preset name, if any
    pub preset_name: Option<String>,
}

impl Fx {
    /// Create a new FX with minimal info
    pub fn new(guid: String, index: u32, name: String) -> Self {
        Self {
            guid,
            index,
            name: name.clone(),
            plugin_name: name,
            plugin_type: FxType::Unknown,
            enabled: true,
            offline: false,
            window_open: false,
            parameter_count: 0,
            preset_name: None,
        }
    }

    /// Get an FxRef for this FX by GUID
    pub fn as_ref(&self) -> FxRef {
        FxRef::Guid(self.guid.clone())
    }

    /// Get an FxRef for this FX by index
    pub fn as_index_ref(&self) -> FxRef {
        FxRef::Index(self.index)
    }
}

impl Default for Fx {
    fn default() -> Self {
        Self::new(String::new(), 0, String::new())
    }
}

/// FX Parameter state
#[derive(Clone, Debug, Facet)]
pub struct FxParameter {
    /// Parameter index (0-based)
    pub index: u32,
    /// Parameter name
    pub name: String,
    /// Current value (normalized 0.0-1.0)
    pub value: f64,
    /// Formatted display value (e.g., "-12.5 dB", "50%")
    pub formatted: String,
    /// Whether this is a toggle/boolean parameter
    pub is_toggle: bool,
}

impl FxParameter {
    /// Create a new parameter
    pub fn new(index: u32, name: String, value: f64) -> Self {
        Self {
            index,
            name,
            value,
            formatted: format!("{:.2}", value),
            is_toggle: false,
        }
    }
}

impl Default for FxParameter {
    fn default() -> Self {
        Self::new(0, String::new(), 0.0)
    }
}

/// FX latency/PDC information
#[derive(Clone, Debug, Default, Facet)]
pub struct FxLatency {
    /// Plugin-reported PDC in samples
    pub pdc_samples: i32,
    /// Actual PDC being applied in the chain
    pub chain_pdc_actual: i32,
    /// PDC being reported to the host
    pub chain_pdc_reporting: i32,
}

/// Parameter modulation state (LFO, linking)
#[derive(Clone, Debug, Default, Facet)]
pub struct FxParamModulation {
    /// Whether LFO modulation is active
    pub lfo_active: bool,
    /// LFO speed
    pub lfo_speed: f64,
    /// LFO modulation strength/depth
    pub lfo_strength: f64,
    /// Whether parameter linking is active
    pub link_active: bool,
    /// Linked FX index (-1 if not linked)
    pub link_fx_index: i32,
    /// Linked parameter index (-1 if not linked)
    pub link_param_index: i32,
}

// =============================================================================
// Request Types (for service methods with many parameters)
// =============================================================================

/// Target for FX operations - combines chain context and FX reference
#[derive(Clone, Debug, Facet)]
pub struct FxTarget {
    /// Which FX chain to operate on
    pub context: FxChainContext,
    /// Which FX in the chain
    pub fx: FxRef,
}

impl FxTarget {
    /// Create a new FX target
    pub fn new(context: FxChainContext, fx: impl Into<FxRef>) -> Self {
        Self {
            context,
            fx: fx.into(),
        }
    }

    /// Create a target for track FX chain
    pub fn track(track_guid: impl Into<String>, fx: impl Into<FxRef>) -> Self {
        Self::new(FxChainContext::track(track_guid), fx)
    }

    /// Create a target for input FX chain
    pub fn input(track_guid: impl Into<String>, fx: impl Into<FxRef>) -> Self {
        Self::new(FxChainContext::input(track_guid), fx)
    }
}

/// Request to set a parameter by index
#[derive(Clone, Debug, Facet)]
pub struct SetParameterRequest {
    /// Target FX
    pub target: FxTarget,
    /// Parameter index
    pub index: u32,
    /// New value (normalized 0.0-1.0)
    pub value: f64,
}

/// Request to set a parameter by name
#[derive(Clone, Debug, Facet)]
pub struct SetParameterByNameRequest {
    /// Target FX
    pub target: FxTarget,
    /// Parameter name
    pub name: String,
    /// New value (normalized 0.0-1.0)
    pub value: f64,
}

/// Request to set a named config parameter
#[derive(Clone, Debug, Facet)]
pub struct SetNamedConfigRequest {
    /// Target FX
    pub target: FxTarget,
    /// Config key
    pub key: String,
    /// Config value
    pub value: String,
}

/// Request to add FX at a specific position
#[derive(Clone, Debug, Facet)]
pub struct AddFxAtRequest {
    /// Target chain context
    pub context: FxChainContext,
    /// Plugin name to add
    pub name: String,
    /// Position in chain
    pub index: u32,
}

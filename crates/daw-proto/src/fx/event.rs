//! FX events for reactive subscriptions

use super::{Fx, FxChainContext};
use facet::Facet;

/// Events emitted when FX state changes
#[repr(u8)]
#[derive(Debug, Clone, Facet)]
pub enum FxEvent {
    /// An FX was added to a chain
    Added { context: FxChainContext, fx: Fx },
    /// An FX was removed from a chain
    Removed {
        context: FxChainContext,
        fx_guid: String,
    },
    /// FX enabled/bypass state changed
    EnabledChanged {
        context: FxChainContext,
        fx_guid: String,
        enabled: bool,
    },
    /// FX was moved within the chain
    Moved {
        context: FxChainContext,
        fx_guid: String,
        old_index: u32,
        new_index: u32,
    },
    /// FX parameter value changed
    ParameterChanged {
        context: FxChainContext,
        fx_guid: String,
        param_index: u32,
        value: f64,
    },
    /// FX preset changed
    PresetChanged {
        context: FxChainContext,
        fx_guid: String,
        preset_name: Option<String>,
    },
    /// FX UI window opened/closed
    WindowChanged {
        context: FxChainContext,
        fx_guid: String,
        open: bool,
    },
}

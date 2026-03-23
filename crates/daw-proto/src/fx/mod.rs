//! FX module
//!
//! This module provides FX (audio plugin) types and the FxService trait
//! for managing audio effects in DAW track chains.

mod error;
mod event;
mod service;
pub mod tree;
mod types;

pub use error::FxError;
pub use event::FxEvent;
pub use service::{FxService, FxServiceClient, FxServiceDispatcher, fx_service_service_descriptor};
pub use tree::{
    FxContainerChannelConfig, FxNode, FxNodeId, FxNodeKind, FxRoutingMode, FxTree,
    FxTreeDepthFirstIter,
};
pub use types::{
    AddFxAtRequest, CreateContainerRequest, EncloseInContainerRequest, Fx, FxChainContext,
    FxChannelConfig, FxLatency, FxParamModulation, FxParameter, FxPinMappings, FxPresetIndex,
    FxRef, FxStateChunk, FxTarget, FxType, InstalledFx, LastTouchedFx, MoveFromContainerRequest,
    MoveToContainerRequest, SetContainerChannelConfigRequest, SetNamedConfigRequest,
    SetParameterByNameRequest, SetParameterRequest,
};

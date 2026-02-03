//! FX module
//!
//! This module provides FX (audio plugin) types and the FxService trait
//! for managing audio effects in DAW track chains.

mod error;
mod event;
mod service;
mod types;

pub use error::FxError;
pub use event::FxEvent;
pub use service::{FxService, FxServiceClient, FxServiceDispatcher};
pub use types::{
    AddFxAtRequest, Fx, FxChainContext, FxLatency, FxParamModulation, FxParameter, FxRef, FxTarget,
    FxType, SetNamedConfigRequest, SetParameterByNameRequest, SetParameterRequest,
};

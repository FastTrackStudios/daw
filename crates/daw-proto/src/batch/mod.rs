//! Batch Instruction Set for DAW RPC
//!
//! Collapses N operations into 1 RPC call by sending a program of instructions
//! that the daw-bridge executes sequentially, resolving cross-step dependencies
//! server-side.

mod args;
mod op;
mod output;
mod program;

pub use args::*;
pub use op::*;
pub use output::*;
pub use program::*;

use vox::service;

/// Service for executing batch instruction programs.
///
/// A single RPC call executes a sequence of instructions, resolving
/// cross-step dependencies server-side. Cost: 1 SHM round-trip + N
/// cheap main-thread dispatches.
#[service]
pub trait BatchService {
    /// Execute a batch program and return all results.
    async fn execute(&self, request: BatchRequest) -> BatchResponse;
}

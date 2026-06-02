//! `impl BatchExecution for Standalone` — minimal pass-through.
//!
//! The batch service collapses N method calls into a single RPC by
//! sending an instruction program. On the REAPER backend each
//! instruction routes into the matching service impl (Tracks, Items,
//! Effects, etc.) via the daw-bridge's dispatch table.
//!
//! On standalone we don't have a typed dispatcher between the proto
//! traits and our impls — but tests don't need batched execution
//! since they can call each trait directly. To keep the trait
//! satisfied (and any code that depends on the BatchExecution trait
//! compile), we accept the request and return an empty results vec.
//!
//! Full routing is feasible (`BatchOp` is an enum we could walk) but
//! it's a ~30-arm match against the standalone trait surface — out
//! of scope until a concrete consumer needs it.
//!
//! Tests that want to drive standalone via batched calls should
//! invoke each method individually instead.

use daw_proto::batch::{BatchExecution, BatchRequest, BatchResponse, StepOutcome, StepResult};

use crate::sync::Standalone;

impl BatchExecution for Standalone {
    fn execute(&self, request: BatchRequest) -> BatchResponse {
        // Mirror REAPER's response shape so client code that walks
        // `results` doesn't panic. Each step reports as Skipped(0)
        // with the operation's own step index — a clear "did not
        // run" signal that won't be mistaken for success.
        let results = request
            .instructions
            .iter()
            .map(|inst| StepResult {
                step: inst.step,
                outcome: StepOutcome::Error(
                    "BatchExecution not routed on standalone — invoke the proto trait directly"
                        .to_string(),
                ),
            })
            .collect();
        BatchResponse { results }
    }
}

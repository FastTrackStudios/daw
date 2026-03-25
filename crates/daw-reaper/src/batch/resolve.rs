//! Step reference resolution — extracts concrete values from previous step outputs.

use daw_proto::batch::{FxChainArg, ProjectArg, StepOutput, TrackArg};
use daw_proto::{FxChainContext, ProjectContext, TrackRef};

/// Resolve a `ProjectArg` into a concrete `ProjectContext`.
pub fn resolve_project_arg(
    arg: &ProjectArg,
    outputs: &[Option<StepOutput>],
) -> Result<ProjectContext, String> {
    match arg {
        ProjectArg::Literal(ctx) => Ok(ctx.clone()),
        ProjectArg::FromStep(n) => {
            let output = outputs
                .get(*n as usize)
                .and_then(|o| o.as_ref())
                .ok_or_else(|| format!("step {} has no output", n))?;
            match output {
                StepOutput::ProjectInfo(pi) => Ok(ProjectContext::Project(pi.guid.clone())),
                StepOutput::OptProjectInfo(Some(pi)) => {
                    Ok(ProjectContext::Project(pi.guid.clone()))
                }
                other => Err(format!(
                    "step {} produced {:?}, expected ProjectInfo",
                    n,
                    std::mem::discriminant(other)
                )),
            }
        }
    }
}

/// Resolve a `TrackArg` into a concrete `TrackRef`.
pub fn resolve_track_arg(
    arg: &TrackArg,
    outputs: &[Option<StepOutput>],
) -> Result<TrackRef, String> {
    match arg {
        TrackArg::Literal(r) => Ok(r.clone()),
        TrackArg::FromStep(n) => {
            let output = outputs
                .get(*n as usize)
                .and_then(|o| o.as_ref())
                .ok_or_else(|| format!("step {} has no output", n))?;
            match output {
                StepOutput::Track(t) => Ok(TrackRef::Guid(t.guid.clone())),
                StepOutput::OptTrack(Some(t)) => Ok(TrackRef::Guid(t.guid.clone())),
                // add_track returns a String GUID
                StepOutput::Str(guid) => Ok(TrackRef::Guid(guid.clone())),
                other => Err(format!(
                    "step {} produced {:?}, expected Track or String",
                    n,
                    std::mem::discriminant(other)
                )),
            }
        }
        TrackArg::FromStepIndex(n, i) => {
            let output = outputs
                .get(*n as usize)
                .and_then(|o| o.as_ref())
                .ok_or_else(|| format!("step {} has no output", n))?;
            match output {
                StepOutput::TrackList(ts) => {
                    let track = ts.get(*i as usize).ok_or_else(|| {
                        format!(
                            "step {} track list has {} items, index {} out of bounds",
                            n,
                            ts.len(),
                            i
                        )
                    })?;
                    Ok(TrackRef::Guid(track.guid.clone()))
                }
                other => Err(format!(
                    "step {} produced {:?}, expected TrackList",
                    n,
                    std::mem::discriminant(other)
                )),
            }
        }
    }
}

/// Resolve an `FxChainArg` into a concrete `FxChainContext`.
pub fn resolve_fx_chain_arg(
    arg: &FxChainArg,
    outputs: &[Option<StepOutput>],
) -> Result<FxChainContext, String> {
    match arg {
        FxChainArg::Literal(ctx) => Ok(ctx.clone()),
        FxChainArg::TrackFromStep(n) => {
            let output = outputs
                .get(*n as usize)
                .and_then(|o| o.as_ref())
                .ok_or_else(|| format!("step {} has no output", n))?;
            match output {
                StepOutput::Track(t) => Ok(FxChainContext::Track(t.guid.clone())),
                StepOutput::OptTrack(Some(t)) => Ok(FxChainContext::Track(t.guid.clone())),
                StepOutput::Str(guid) => Ok(FxChainContext::Track(guid.clone())),
                other => Err(format!(
                    "step {} produced {:?}, expected Track for FxChainContext",
                    n,
                    std::mem::discriminant(other)
                )),
            }
        }
    }
}

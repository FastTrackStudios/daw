//! Batch builder for constructing batch programs with type-safe step handles.
//!
//! # Example
//!
//! ```no_run
//! use daw_control::{BatchBuilder, BatchResponseExt, Daw};
//!
//! # async fn example(daw: &Daw) -> daw_control::Result<()> {
//! let mut b = BatchBuilder::new().with_undo("Setup routing");
//! let project = b.current_project();
//! let tracks = b.get_tracks(&project);
//! let transport = b.get_transport(&project);
//!
//! let response = daw.execute_batch(b.build()).await?;
//! let tracks: Vec<daw_proto::Track> = response.get(&tracks).unwrap();
//! let transport: daw_proto::transport::transport::Transport = response.get(&transport).unwrap();
//! # Ok(())
//! # }
//! ```

use std::marker::PhantomData;

use daw_proto::batch::*;
use daw_proto::*;

/// A typed handle to a step in a batch program.
///
/// The type parameter `T` represents the expected output type of the step.
/// This is used at extraction time to safely downcast the `StepOutput`.
pub struct StepHandle<T> {
    index: u32,
    _phantom: PhantomData<T>,
}

impl<T> StepHandle<T> {
    fn new(index: u32) -> Self {
        Self {
            index,
            _phantom: PhantomData,
        }
    }

    /// Get the step index.
    pub fn index(&self) -> u32 {
        self.index
    }
}

/// Builder for constructing batch programs with automatic step numbering.
pub struct BatchBuilder {
    instructions: Vec<BatchInstruction>,
    options: BatchOptions,
}

impl BatchBuilder {
    /// Create a new empty batch builder.
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            options: BatchOptions::default(),
        }
    }

    /// Set the undo label — all mutations will be grouped in a single undo block.
    pub fn with_undo(mut self, label: impl Into<String>) -> Self {
        self.options.undo_label = Some(label.into());
        self
    }

    /// Enable fail-fast mode — stop on first error.
    pub fn with_fail_fast(mut self) -> Self {
        self.options.fail_fast = true;
        self
    }

    /// Build the batch request.
    pub fn build(self) -> BatchRequest {
        BatchRequest {
            instructions: self.instructions,
            options: self.options,
        }
    }

    /// Add a raw instruction and return a typed handle.
    fn push<T>(&mut self, op: BatchOp) -> StepHandle<T> {
        let index = self.instructions.len() as u32;
        self.instructions.push(BatchInstruction { step: index, op });
        StepHandle::new(index)
    }

    // =========================================================================
    // Project operations
    // =========================================================================

    /// Get the current project.
    pub fn current_project(&mut self) -> StepHandle<Option<ProjectInfo>> {
        self.push(BatchOp::Project(ProjectOp::GetCurrent))
    }

    /// Get a specific project by GUID.
    pub fn get_project(&mut self, guid: impl Into<String>) -> StepHandle<Option<ProjectInfo>> {
        self.push(BatchOp::Project(ProjectOp::Get(guid.into())))
    }

    /// List all open projects.
    pub fn list_projects(&mut self) -> StepHandle<Vec<ProjectInfo>> {
        self.push(BatchOp::Project(ProjectOp::List))
    }

    // =========================================================================
    // Transport operations
    // =========================================================================

    /// Play the project.
    pub fn play(&mut self, project: &StepHandle<Option<ProjectInfo>>) -> StepHandle<()> {
        self.push(BatchOp::Transport(TransportOp::Play(ProjectArg::FromStep(
            project.index,
        ))))
    }

    /// Stop the project.
    pub fn stop(&mut self, project: &StepHandle<Option<ProjectInfo>>) -> StepHandle<()> {
        self.push(BatchOp::Transport(TransportOp::Stop(ProjectArg::FromStep(
            project.index,
        ))))
    }

    /// Get transport state.
    pub fn get_transport(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
    ) -> StepHandle<transport::transport::Transport> {
        self.push(BatchOp::Transport(TransportOp::GetState(
            ProjectArg::FromStep(project.index),
        )))
    }

    /// Get tempo.
    pub fn get_tempo(&mut self, project: &StepHandle<Option<ProjectInfo>>) -> StepHandle<f64> {
        self.push(BatchOp::Transport(TransportOp::GetTempo(
            ProjectArg::FromStep(project.index),
        )))
    }

    /// Set tempo.
    pub fn set_tempo(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        bpm: f64,
    ) -> StepHandle<()> {
        self.push(BatchOp::Transport(TransportOp::SetTempo(
            ProjectArg::FromStep(project.index),
            bpm,
        )))
    }

    // =========================================================================
    // Track operations
    // =========================================================================

    /// Get all tracks in a project.
    pub fn get_tracks(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
    ) -> StepHandle<Vec<Track>> {
        self.push(BatchOp::Track(TrackOp::GetTracks(ProjectArg::FromStep(
            project.index,
        ))))
    }

    /// Get a specific track by reference.
    pub fn get_track(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        track: TrackRef,
    ) -> StepHandle<Option<Track>> {
        self.push(BatchOp::Track(TrackOp::GetTrack(
            ProjectArg::FromStep(project.index),
            TrackArg::Literal(track),
        )))
    }

    /// Get track count.
    pub fn track_count(&mut self, project: &StepHandle<Option<ProjectInfo>>) -> StepHandle<u32> {
        self.push(BatchOp::Track(TrackOp::TrackCount(ProjectArg::FromStep(
            project.index,
        ))))
    }

    /// Add a track and get its GUID.
    pub fn add_track(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        name: impl Into<String>,
        at_index: Option<u32>,
    ) -> StepHandle<String> {
        self.push(BatchOp::Track(TrackOp::AddTrack(
            ProjectArg::FromStep(project.index),
            name.into(),
            at_index,
        )))
    }

    /// Set track muted using a step handle from get_tracks (by index).
    pub fn set_track_muted_by_index(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        tracks: &StepHandle<Vec<Track>>,
        track_index: u32,
        muted: bool,
    ) -> StepHandle<()> {
        self.push(BatchOp::Track(TrackOp::SetMuted(
            ProjectArg::FromStep(project.index),
            TrackArg::FromStepIndex(tracks.index, track_index),
            muted,
        )))
    }

    /// Set track muted using a literal TrackRef.
    pub fn set_track_muted(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        track: TrackRef,
        muted: bool,
    ) -> StepHandle<()> {
        self.push(BatchOp::Track(TrackOp::SetMuted(
            ProjectArg::FromStep(project.index),
            TrackArg::Literal(track),
            muted,
        )))
    }

    /// Set track volume.
    pub fn set_track_volume(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        track: TrackRef,
        volume: f64,
    ) -> StepHandle<()> {
        self.push(BatchOp::Track(TrackOp::SetVolume(
            ProjectArg::FromStep(project.index),
            TrackArg::Literal(track),
            volume,
        )))
    }

    /// Rename a track.
    pub fn rename_track(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        track: TrackRef,
        name: impl Into<String>,
    ) -> StepHandle<()> {
        self.push(BatchOp::Track(TrackOp::RenameTrack(
            ProjectArg::FromStep(project.index),
            TrackArg::Literal(track),
            name.into(),
        )))
    }

    // =========================================================================
    // FX operations
    // =========================================================================

    /// Get the FX list for a track (using step handle for track).
    pub fn get_fx_list_from_track(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        track: &StepHandle<String>,
    ) -> StepHandle<Vec<Fx>> {
        self.push(BatchOp::Fx(FxOp::GetFxList(
            ProjectArg::FromStep(project.index),
            FxChainArg::TrackFromStep(track.index),
        )))
    }

    /// Get the FX list using a literal chain context.
    pub fn get_fx_list(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        chain: FxChainContext,
    ) -> StepHandle<Vec<Fx>> {
        self.push(BatchOp::Fx(FxOp::GetFxList(
            ProjectArg::FromStep(project.index),
            FxChainArg::Literal(chain),
        )))
    }

    /// Get FX parameters.
    pub fn get_fx_parameters(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        chain: FxChainContext,
        fx: FxRef,
    ) -> StepHandle<Vec<FxParameter>> {
        self.push(BatchOp::Fx(FxOp::GetParameters(
            ProjectArg::FromStep(project.index),
            FxChainArg::Literal(chain),
            fx,
        )))
    }

    /// Set FX enabled state.
    pub fn set_fx_enabled(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        chain: FxChainContext,
        fx: FxRef,
        enabled: bool,
    ) -> StepHandle<()> {
        self.push(BatchOp::Fx(FxOp::SetFxEnabled(
            ProjectArg::FromStep(project.index),
            FxChainArg::Literal(chain),
            fx,
            enabled,
        )))
    }

    /// Add an FX plugin to a chain.
    pub fn add_fx(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        chain: FxChainContext,
        name: impl Into<String>,
    ) -> StepHandle<Option<String>> {
        self.push(BatchOp::Fx(FxOp::AddFx(
            ProjectArg::FromStep(project.index),
            FxChainArg::Literal(chain),
            name.into(),
        )))
    }

    /// Set an FX parameter value.
    pub fn set_fx_parameter(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        request: SetParameterRequest,
    ) -> StepHandle<()> {
        self.push(BatchOp::Fx(FxOp::SetParameter(
            ProjectArg::FromStep(project.index),
            request,
        )))
    }

    // =========================================================================
    // Routing operations
    // =========================================================================

    /// Get sends for a track.
    pub fn get_sends(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        track: TrackRef,
    ) -> StepHandle<Vec<TrackRoute>> {
        self.push(BatchOp::Routing(RoutingOp::GetSends(
            ProjectArg::FromStep(project.index),
            TrackArg::Literal(track),
        )))
    }

    /// Add a send between two tracks.
    pub fn add_send(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        source: TrackRef,
        dest: TrackRef,
    ) -> StepHandle<Option<u32>> {
        self.push(BatchOp::Routing(RoutingOp::AddSend(
            ProjectArg::FromStep(project.index),
            TrackArg::Literal(source),
            TrackArg::Literal(dest),
        )))
    }

    // =========================================================================
    // Marker operations
    // =========================================================================

    /// Get all markers.
    pub fn get_markers(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
    ) -> StepHandle<Vec<Marker>> {
        self.push(BatchOp::Marker(MarkerOp::GetMarkers(ProjectArg::FromStep(
            project.index,
        ))))
    }

    /// Add a marker.
    pub fn add_marker(
        &mut self,
        project: &StepHandle<Option<ProjectInfo>>,
        position: f64,
        name: impl Into<String>,
    ) -> StepHandle<u32> {
        self.push(BatchOp::Marker(MarkerOp::AddMarker(
            ProjectArg::FromStep(project.index),
            position,
            name.into(),
        )))
    }

    // =========================================================================
    // Raw op — for ops not covered by convenience methods
    // =========================================================================

    /// Push a raw batch operation. Use this for operations not covered by
    /// convenience methods above.
    pub fn push_raw<T>(&mut self, op: BatchOp) -> StepHandle<T> {
        self.push(op)
    }
}

impl Default for BatchBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Response extraction
// =============================================================================

/// Error type for batch response extraction.
#[derive(Debug)]
pub enum BatchExtractError {
    /// The step was not found in the response.
    StepNotFound(u32),
    /// The step failed with an error.
    StepFailed(String),
    /// The step was skipped because a dependency failed.
    StepSkipped(u32),
    /// The output type didn't match the expected type.
    TypeMismatch { step: u32, expected: &'static str },
}

impl std::fmt::Display for BatchExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StepNotFound(s) => write!(f, "step {} not found in response", s),
            Self::StepFailed(msg) => write!(f, "step failed: {}", msg),
            Self::StepSkipped(dep) => write!(f, "step skipped due to dependency {} failing", dep),
            Self::TypeMismatch { step, expected } => {
                write!(
                    f,
                    "step {} output type mismatch, expected {}",
                    step, expected
                )
            }
        }
    }
}

impl std::error::Error for BatchExtractError {}

/// Extension trait for extracting typed results from a `BatchResponse`.
pub trait BatchResponseExt {
    /// Extract a typed result from a batch response using a step handle.
    fn get<T: FromStepOutput>(&self, handle: &StepHandle<T>) -> Result<T, BatchExtractError>;
}

impl BatchResponseExt for BatchResponse {
    fn get<T: FromStepOutput>(&self, handle: &StepHandle<T>) -> Result<T, BatchExtractError> {
        let result = self
            .results
            .iter()
            .find(|r| r.step == handle.index)
            .ok_or(BatchExtractError::StepNotFound(handle.index))?;

        match &result.outcome {
            StepOutcome::Ok(output) => T::from_output(output, handle.index),
            StepOutcome::Error(msg) => Err(BatchExtractError::StepFailed(msg.clone())),
            StepOutcome::Skipped(dep) => Err(BatchExtractError::StepSkipped(*dep)),
        }
    }
}

/// Trait for extracting typed values from `StepOutput`.
pub trait FromStepOutput: Sized {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError>;
}

// Implement FromStepOutput for common types

impl FromStepOutput for () {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::Unit => Ok(()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "()",
            }),
        }
    }
}

impl FromStepOutput for bool {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::Bool(v) => Ok(*v),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "bool",
            }),
        }
    }
}

impl FromStepOutput for u32 {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::U32(v) => Ok(*v),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "u32",
            }),
        }
    }
}

impl FromStepOutput for f64 {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::F64(v) => Ok(*v),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "f64",
            }),
        }
    }
}

impl FromStepOutput for String {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::Str(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "String",
            }),
        }
    }
}

impl FromStepOutput for Option<String> {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::OptStr(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "Option<String>",
            }),
        }
    }
}

impl FromStepOutput for Option<ProjectInfo> {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::OptProjectInfo(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "Option<ProjectInfo>",
            }),
        }
    }
}

impl FromStepOutput for Vec<ProjectInfo> {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::ProjectInfoList(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "Vec<ProjectInfo>",
            }),
        }
    }
}

impl FromStepOutput for Vec<Track> {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::TrackList(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "Vec<Track>",
            }),
        }
    }
}

impl FromStepOutput for Option<Track> {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::OptTrack(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "Option<Track>",
            }),
        }
    }
}

impl FromStepOutput for Vec<Fx> {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::FxList(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "Vec<Fx>",
            }),
        }
    }
}

impl FromStepOutput for Vec<FxParameter> {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::FxParameterList(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "Vec<FxParameter>",
            }),
        }
    }
}

impl FromStepOutput for transport::transport::Transport {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::Transport(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "Transport",
            }),
        }
    }
}

impl FromStepOutput for Vec<TrackRoute> {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::RouteList(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "Vec<TrackRoute>",
            }),
        }
    }
}

impl FromStepOutput for Option<u32> {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::OptU32(v) => Ok(*v),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "Option<u32>",
            }),
        }
    }
}

impl FromStepOutput for Vec<Marker> {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::MarkerList(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "Vec<Marker>",
            }),
        }
    }
}

impl FromStepOutput for TimeSignature {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::TimeSignature(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "TimeSignature",
            }),
        }
    }
}

impl FromStepOutput for Vec<Item> {
    fn from_output(output: &StepOutput, step: u32) -> Result<Self, BatchExtractError> {
        match output {
            StepOutput::ItemList(v) => Ok(v.clone()),
            _ => Err(BatchExtractError::TypeMismatch {
                step,
                expected: "Vec<Item>",
            }),
        }
    }
}

//! Project handle

use std::sync::Arc;

use crate::{DawClients, FxChain, Markers, ProjectItems, Regions, TempoMap, Tracks, Transport};
use daw_proto::FxChainContext;

/// Project handle - lightweight wrapper around project GUID
///
/// This handle represents a specific DAW project. It stores only the project GUID
/// and provides methods to access project subsystems (transport, tracks, etc.).
///
/// Like reaper-rs, this handle is lightweight and cheap to clone.
///
/// # Example
///
/// ```no_run
/// use daw_control::Daw;
///
/// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
/// let daw = Daw::new(handle);
/// let project = daw.current_project().await?;
/// println!("Project GUID: {}", project.guid());
///
/// // Access transport
/// project.transport().play().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Project {
    guid: String,
    clients: Arc<DawClients>,
}

impl Project {
    /// Create a new project handle
    pub(crate) fn new(guid: String, clients: Arc<DawClients>) -> Self {
        Self { guid, clients }
    }

    /// Get the project GUID
    pub fn guid(&self) -> &str {
        &self.guid
    }

    /// Get transport accessor for this project
    ///
    /// Returns a handle to control and monitor the transport (playback, recording, etc.)
    /// for this specific project.
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
    /// // Control transport
    /// project.transport().play().await?;
    /// project.transport().stop().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn transport(&self) -> Transport {
        Transport::new(self.guid.clone(), self.clients.clone())
    }

    /// Get markers accessor for this project
    ///
    /// Returns a handle to query and manipulate markers in this project.
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
    /// // Query and manipulate markers
    /// let markers = project.markers().all().await?;
    /// project.markers().add(10.0, "Verse 1").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn markers(&self) -> Markers {
        Markers::new(self.guid.clone(), self.clients.clone())
    }

    /// Get regions accessor for this project
    ///
    /// Returns a handle to query and manipulate regions in this project.
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
    /// // Query and manipulate regions
    /// let regions = project.regions().all().await?;
    /// project.regions().add(0.0, 30.0, "Intro").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn regions(&self) -> Regions {
        Regions::new(self.guid.clone(), self.clients.clone())
    }

    /// Get tempo map accessor for this project
    ///
    /// Returns a handle to query and manipulate tempo/time signature changes
    /// in this project.
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
    /// // Query tempo
    /// let bpm = project.tempo_map().tempo_at(10.0).await?;
    /// let (measure, beat, frac) = project.tempo_map().time_to_musical(30.0).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn tempo_map(&self) -> TempoMap {
        TempoMap::new(self.guid.clone(), self.clients.clone())
    }

    /// Get tracks accessor for this project
    ///
    /// Returns a handle to enumerate tracks and perform batch track operations.
    /// Individual track operations are performed through [`TrackHandle`](crate::TrackHandle).
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
    /// // Enumerate tracks
    /// for track in project.tracks().all().await? {
    ///     println!("Track: {}", track.name);
    /// }
    ///
    /// // Get specific track and control it
    /// if let Some(vocals) = project.tracks().by_name("Vocals").await? {
    ///     vocals.solo_exclusive().await?;
    ///     vocals.fx_chain().add("ReaComp").await?;
    /// }
    ///
    /// // Batch operations
    /// project.tracks().clear_solo().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn tracks(&self) -> Tracks {
        Tracks::new(self.guid.clone(), self.clients.clone())
    }

    /// Get monitoring FX chain (global, not per-track)
    ///
    /// Returns a handle to the monitoring FX chain, which is applied globally
    /// to the monitoring output.
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
    /// // Add monitoring FX
    /// let mon_chain = project.monitoring_fx();
    /// for fx in mon_chain.all().await? {
    ///     println!("Monitoring FX: {}", fx.name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn monitoring_fx(&self) -> FxChain {
        FxChain::new(
            FxChainContext::Monitoring,
            self.guid.clone(),
            self.clients.clone(),
        )
    }

    // =========================================================================
    // Position Conversions
    // =========================================================================
    //
    // Following reaper-rs design philosophy:
    // - Position conversions require project context (tempo map)
    // - Conversions return rich result types with measure/beat metadata
    // - No implicit conversions between position types

    /// Convert time position to beats
    ///
    /// Returns beat position with measure context and time signature information.
    /// Requires the project's tempo map for accurate conversion.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    /// use daw_proto::{PositionInSeconds, MeasureMode};
    ///
    /// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
    /// let daw = Daw::new(handle);
    /// let project = daw.current_project().await?;
    ///
    /// let pos = PositionInSeconds::from_seconds(30.0);
    /// let result = project.time_to_beats(pos, MeasureMode::IgnoreMeasure).await?;
    /// println!("Beat: {}, Measure: {}", result.full_beats.as_beats(), result.measure_index);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn time_to_beats(
        &self,
        position: daw_proto::PositionInSeconds,
        measure_mode: daw_proto::MeasureMode,
    ) -> eyre::Result<daw_proto::TimeToBeatsResult> {
        let result = self
            .clients
            .position_conversion
            .time_to_beats(
                daw_proto::ProjectContext::project(&self.guid),
                position,
                measure_mode,
            )
            .await?;
        Ok(result)
    }

    /// Convert beat position to time
    ///
    /// Returns time position in seconds based on the project's tempo map.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    /// use daw_proto::{PositionInBeats, MeasureMode};
    ///
    /// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
    /// let daw = Daw::new(handle);
    /// let project = daw.current_project().await?;
    ///
    /// let pos = PositionInBeats::from_beats(16.0);
    /// let seconds = project.beats_to_time(pos, MeasureMode::IgnoreMeasure).await?;
    /// println!("Time: {}s", seconds.as_seconds());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn beats_to_time(
        &self,
        position: daw_proto::PositionInBeats,
        measure_mode: daw_proto::MeasureMode,
    ) -> eyre::Result<daw_proto::PositionInSeconds> {
        let result = self
            .clients
            .position_conversion
            .beats_to_time(
                daw_proto::ProjectContext::project(&self.guid),
                position,
                measure_mode,
            )
            .await?;
        Ok(result)
    }

    /// Convert time position to quarter notes
    ///
    /// Returns quarter note position with measure context and time signature.
    /// Quarter notes are REAPER's native time mapping unit.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    /// use daw_proto::PositionInSeconds;
    ///
    /// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
    /// let daw = Daw::new(handle);
    /// let project = daw.current_project().await?;
    ///
    /// let pos = PositionInSeconds::from_seconds(30.0);
    /// let result = project.time_to_quarter_notes(pos).await?;
    /// println!("QN: {}", result.quarter_notes.as_quarter_notes());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn time_to_quarter_notes(
        &self,
        position: daw_proto::PositionInSeconds,
    ) -> eyre::Result<daw_proto::TimeToQuarterNotesResult> {
        let result = self
            .clients
            .position_conversion
            .time_to_quarter_notes(daw_proto::ProjectContext::project(&self.guid), position)
            .await?;
        Ok(result)
    }

    /// Convert quarter notes to time position
    ///
    /// Returns time position in seconds based on the project's tempo map.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    /// use daw_proto::PositionInQuarterNotes;
    ///
    /// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
    /// let daw = Daw::new(handle);
    /// let project = daw.current_project().await?;
    ///
    /// let pos = PositionInQuarterNotes::from_quarter_notes(16.0);
    /// let seconds = project.quarter_notes_to_time(pos).await?;
    /// println!("Time: {}s", seconds.as_seconds());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn quarter_notes_to_time(
        &self,
        position: daw_proto::PositionInQuarterNotes,
    ) -> eyre::Result<daw_proto::PositionInSeconds> {
        let result = self
            .clients
            .position_conversion
            .quarter_notes_to_time(daw_proto::ProjectContext::project(&self.guid), position)
            .await?;
        Ok(result)
    }

    /// Convert quarter notes to measure information
    ///
    /// Returns the measure index and start/end positions for that measure.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    /// use daw_proto::PositionInQuarterNotes;
    ///
    /// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
    /// let daw = Daw::new(handle);
    /// let project = daw.current_project().await?;
    ///
    /// let pos = PositionInQuarterNotes::from_quarter_notes(16.0);
    /// let measure = project.quarter_notes_to_measure(pos).await?;
    /// println!("Measure {}: {} to {} QN",
    ///     measure.measure_index,
    ///     measure.start.as_quarter_notes(),
    ///     measure.end.as_quarter_notes()
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub async fn quarter_notes_to_measure(
        &self,
        position: daw_proto::PositionInQuarterNotes,
    ) -> eyre::Result<daw_proto::QuarterNotesToMeasureResult> {
        let result = self
            .clients
            .position_conversion
            .quarter_notes_to_measure(daw_proto::ProjectContext::project(&self.guid), position)
            .await?;
        Ok(result)
    }

    /// Convert beats to quarter notes
    ///
    /// In most cases beats == quarter notes, but this can vary with time signature.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    /// use daw_proto::PositionInBeats;
    ///
    /// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
    /// let daw = Daw::new(handle);
    /// let project = daw.current_project().await?;
    ///
    /// let beats = PositionInBeats::from_beats(8.0);
    /// let qn = project.beats_to_quarter_notes(beats).await?;
    /// println!("QN: {}", qn.as_quarter_notes());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn beats_to_quarter_notes(
        &self,
        position: daw_proto::PositionInBeats,
    ) -> eyre::Result<daw_proto::PositionInQuarterNotes> {
        let result = self
            .clients
            .position_conversion
            .beats_to_quarter_notes(daw_proto::ProjectContext::project(&self.guid), position)
            .await?;
        Ok(result)
    }

    /// Convert quarter notes to beats
    ///
    /// In most cases quarter notes == beats, but this can vary with time signature.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use daw_control::Daw;
    /// use daw_proto::PositionInQuarterNotes;
    ///
    /// # async fn example(handle: roam::session::ConnectionHandle) -> eyre::Result<()> {
    /// let daw = Daw::new(handle);
    /// let project = daw.current_project().await?;
    ///
    /// let qn = PositionInQuarterNotes::from_quarter_notes(8.0);
    /// let beats = project.quarter_notes_to_beats(qn).await?;
    /// println!("Beats: {}", beats.as_beats());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn quarter_notes_to_beats(
        &self,
        position: daw_proto::PositionInQuarterNotes,
    ) -> eyre::Result<daw_proto::PositionInBeats> {
        let result = self
            .clients
            .position_conversion
            .quarter_notes_to_beats(daw_proto::ProjectContext::project(&self.guid), position)
            .await?;
        Ok(result)
    }

    // =========================================================================
    // Items Access
    // =========================================================================

    /// Get access to all items in the project
    ///
    /// Returns a handle for project-wide item operations like getting all items
    /// or selected items.
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
    /// // Get all selected items
    /// let selected = project.items().selected().await?;
    /// for item in selected {
    ///     println!("Selected item: {}", item.guid());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn items(&self) -> ProjectItems {
        ProjectItems::new(self.guid.clone(), self.clients.clone())
    }
}

impl std::fmt::Debug for Project {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Project").field("guid", &self.guid).finish()
    }
}

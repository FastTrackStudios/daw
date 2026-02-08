//! Project-level MIDI analysis handle.

use std::sync::Arc;

use crate::DawClients;
use daw_proto::{MidiChartData, MidiChartRequest, ProjectContext};
use eyre::Result;

/// Read-only project MIDI analysis access.
#[derive(Clone)]
pub struct MidiAnalysis {
    project_id: String,
    clients: Arc<DawClients>,
}

impl MidiAnalysis {
    pub(crate) fn new(project_id: String, clients: Arc<DawClients>) -> Self {
        Self {
            project_id,
            clients,
        }
    }

    /// Generate chart/chord analysis for this project.
    pub async fn generate_chart_data(&self, track_tag: Option<String>) -> Result<MidiChartData> {
        let req = MidiChartRequest::new(
            ProjectContext::Project(self.project_id.clone()),
            track_tag,
        );
        let result = self.clients.midi_analysis.generate_chart_data(req).await?;
        Ok(result)
    }

    /// Get a lightweight source fingerprint for change detection.
    pub async fn source_fingerprint(&self, track_tag: Option<String>) -> Result<String> {
        let req = MidiChartRequest::new(
            ProjectContext::Project(self.project_id.clone()),
            track_tag,
        );
        let result = self.clients.midi_analysis.source_fingerprint(req).await?;
        Ok(result)
    }
}

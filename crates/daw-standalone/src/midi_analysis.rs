//! Standalone implementation for project-level MIDI analysis.

use crate::StandaloneMidi;
use crate::midi::TakeMidiData;
use crate::platform::RwLock;
use daw_proto::{
    MidiAnalysisService, MidiChartData, MidiChartRequest, MidiDetectedChord, ProjectContext,
};
use keyflow::chord::{MidiNote as KeyflowMidiNote, detect_chords_from_midi_notes};
use keyflow::engraver::import::{
    MidiChartConfig, MidiFile, MidiNote as ImportMidiNote, MidiTrack, TempoEvent,
    TimeSignatureEvent, generate_chart_text,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

const DEFAULT_TRACK_NAME: &str = "CHORDS";
const MIN_CHORD_DURATION_PPQ: i64 = 120;
const DEFAULT_PPQ: u32 = 960;

#[derive(Clone)]
pub struct StandaloneMidiAnalysis {
    takes: Arc<RwLock<Vec<TakeMidiData>>>,
}

impl StandaloneMidiAnalysis {
    pub(crate) fn new(takes: Arc<RwLock<Vec<TakeMidiData>>>) -> Self {
        Self { takes }
    }

    pub fn from_midi(midi: &StandaloneMidi) -> Self {
        Self::new(midi.shared_takes())
    }

    fn track_tag_matches(track_tag: Option<&str>) -> bool {
        let Some(tag) = track_tag else {
            return true;
        };
        DEFAULT_TRACK_NAME
            .to_ascii_lowercase()
            .contains(&tag.to_ascii_lowercase())
    }

    fn make_fingerprint(project: &ProjectContext, notes: &[ImportMidiNote]) -> String {
        let mut hasher = DefaultHasher::new();
        match project {
            ProjectContext::Current => "current".hash(&mut hasher),
            ProjectContext::Project(guid) => guid.hash(&mut hasher),
        }
        for note in notes {
            note.pitch.hash(&mut hasher);
            note.velocity.hash(&mut hasher);
            note.start_tick.hash(&mut hasher);
            note.duration_ticks.hash(&mut hasher);
            note.channel.hash(&mut hasher);
        }
        format!("{:x}", hasher.finish())
    }

    fn import_notes_from_take(source_take: &TakeMidiData) -> Vec<ImportMidiNote> {
        source_take
            .notes
            .iter()
            .map(|note| ImportMidiNote {
                pitch: note.pitch,
                velocity: note.velocity,
                start_tick: note.start_ppq.max(0.0).round() as u32,
                duration_ticks: note.length_ppq.max(1.0).round() as u32,
                channel: note.channel,
            })
            .collect()
    }
}

impl MidiAnalysisService for StandaloneMidiAnalysis {
    async fn source_fingerprint(&self, request: MidiChartRequest) -> Result<String, String> {
        if !Self::track_tag_matches(request.track_tag.as_deref()) {
            return Err(format!(
                "No track matched tag '{}'",
                request.track_tag.as_deref().unwrap_or_default()
            ));
        }

        let takes = self.takes.read().await;
        let Some(source_take) = takes.iter().find(|take| !take.notes.is_empty()) else {
            return Err("No MIDI notes available".to_string());
        };
        let import_notes = Self::import_notes_from_take(source_take);
        if import_notes.is_empty() {
            return Err("No MIDI notes available".to_string());
        }
        Ok(Self::make_fingerprint(&request.project, &import_notes))
    }

    async fn generate_chart_data(
        &self,
        request: MidiChartRequest,
    ) -> Result<MidiChartData, String> {
        if !Self::track_tag_matches(request.track_tag.as_deref()) {
            return Err(format!(
                "No track matched tag '{}'",
                request.track_tag.as_deref().unwrap_or_default()
            ));
        }

        let takes = self.takes.read().await;
        let Some(source_take) = takes.iter().find(|take| !take.notes.is_empty()) else {
            return Err("No MIDI notes available".to_string());
        };

        let import_notes = Self::import_notes_from_take(source_take);

        if import_notes.is_empty() {
            return Err("No MIDI notes available".to_string());
        }

        let midi_file = MidiFile::from_parts(
            DEFAULT_PPQ,
            vec![MidiTrack {
                index: 0,
                name: Some(DEFAULT_TRACK_NAME.to_string()),
                notes: import_notes.clone(),
                channel: None,
            }],
            vec![TempoEvent {
                tick: 0,
                microseconds_per_quarter: 500_000, // 120 BPM
            }],
            vec![TimeSignatureEvent {
                tick: 0,
                numerator: 4,
                denominator: 4,
            }],
            Vec::new(),
            vec![Some(DEFAULT_TRACK_NAME.to_string())],
            None,
        );

        let chart_text = generate_chart_text(&midi_file, &MidiChartConfig::default());

        let keyflow_notes: Vec<KeyflowMidiNote> = source_take
            .notes
            .iter()
            .map(|note| {
                KeyflowMidiNote::new(
                    note.pitch,
                    note.start_ppq.round() as i64,
                    (note.start_ppq + note.length_ppq).round() as i64,
                    note.channel,
                    note.velocity,
                )
            })
            .collect();

        let chords = detect_chords_from_midi_notes(&keyflow_notes, MIN_CHORD_DURATION_PPQ)
            .into_iter()
            .map(|chord| MidiDetectedChord {
                symbol: chord.chord.to_string(),
                start_ppq: chord.start_ppq,
                end_ppq: chord.end_ppq,
                root_pitch: chord.root_pitch,
                velocity: chord.velocity,
            })
            .collect();

        Ok(MidiChartData {
            source_track_name: DEFAULT_TRACK_NAME.to_string(),
            source_fingerprint: Self::make_fingerprint(&request.project, &import_notes),
            chart_text,
            chords,
        })
    }
}

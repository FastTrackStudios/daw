//! REAPER project-level MIDI analysis service.

use crate::main_thread;
use crate::project_context::find_project_by_guid;
use crate::safe_wrappers::markers as markers_sw;
use crate::safe_wrappers::midi as midi_sw;
use crate::safe_wrappers::time_map as time_sw;
use daw_proto::{
    MidiAnalysisService, MidiChartData, MidiChartRequest, MidiDetectedChord, ProjectContext,
};
use keyflow::chord::{MidiNote as KeyflowMidiNote, detect_chords_from_midi_notes};
use keyflow::engraver::import::{
    MarkerEvent, MarkerType, MidiChartConfig, MidiFile, MidiNote as ImportMidiNote, MidiTrack,
    TempoEvent, TimeSignatureEvent, generate_chart_text,
};
use reaper_high::{Project, Reaper, Track};
use reaper_medium::MediaItemTake;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const REAPER_PPQ: u32 = 960;
const MIN_CHORD_DURATION_PPQ: i64 = 120;

#[derive(Clone)]
pub struct ReaperMidiAnalysis;

impl ReaperMidiAnalysis {
    pub fn new() -> Self {
        Self
    }

    fn resolve_project(project: &ProjectContext) -> Option<Project> {
        match project {
            ProjectContext::Current => Some(Reaper::get().current_project()),
            ProjectContext::Project(guid) => find_project_by_guid(guid),
        }
    }

    fn find_track_by_tag(project: &Project, tag: Option<&str>) -> Option<Track> {
        let needle = tag.map(|t| t.to_ascii_lowercase());
        for track in project.tracks() {
            let name = track.name()?.to_str().to_string();
            if let Some(ref n) = needle {
                if name.to_ascii_lowercase().contains(n) {
                    return Some(track);
                }
            } else {
                return Some(track);
            }
        }
        None
    }

    fn get_first_midi_take(track: &Track) -> Option<(MediaItemTake, f64)> {
        let reaper = Reaper::get();
        let medium = reaper.medium_reaper();
        let low = medium.low();

        for item in track.items() {
            let raw_item = item.raw();
            let take = crate::safe_wrappers::item::get_active_take(medium, raw_item)?;
            if !midi_sw::take_is_midi(low, take) {
                continue;
            }
            let item_start_time = crate::safe_wrappers::item::get_media_item_info_value(
                low,
                raw_item,
                c"D_POSITION",
            );
            return Some((take, item_start_time));
        }
        None
    }

    fn read_keyflow_notes(take: MediaItemTake) -> Vec<KeyflowMidiNote> {
        let low = Reaper::get().medium_reaper().low();
        let counts = midi_sw::count_events(low, take);

        let mut notes = Vec::with_capacity(counts.notes.max(0) as usize);
        for i in 0..counts.notes {
            let Some(n) = midi_sw::get_note(low, take, i) else {
                continue;
            };
            if n.muted {
                continue;
            }
            notes.push(KeyflowMidiNote::new(
                n.pitch as u8,
                n.start_ppq.round() as i64,
                n.end_ppq.round() as i64,
                n.channel as u8,
                n.velocity as u8,
            ));
        }
        notes
    }

    fn get_beats_at_time(project: Project, time_seconds: f64) -> f64 {
        let low = Reaper::get().medium_reaper().low();
        let result = time_sw::time_to_beats(low, project.context(), time_seconds);
        result.full_beats
    }

    fn time_to_tick(project: Project, time_seconds: f64) -> u32 {
        let beats = Self::get_beats_at_time(project, time_seconds);
        (beats * f64::from(REAPER_PPQ)).round().max(0.0) as u32
    }

    fn gather_markers(project: Project) -> Vec<MarkerEvent> {
        let low = Reaper::get().medium_reaper().low();
        let mut markers = Vec::new();
        let mut idx = 0;
        loop {
            let Some(result) = markers_sw::enum_project_markers(low, idx) else {
                break;
            };
            idx += 1;
            if result.name.is_empty() {
                continue;
            }
            markers.push(MarkerEvent {
                tick: Self::time_to_tick(project, result.pos),
                text: result.name,
                marker_type: MarkerType::Marker,
            });
        }
        markers.sort_by_key(|m| m.tick);
        markers
    }

    fn import_notes(notes: &[KeyflowMidiNote], item_start_tick: u32) -> Vec<ImportMidiNote> {
        notes
            .iter()
            .map(|note| {
                let abs_start = item_start_tick + (note.start_ppq.max(0) as u32);
                let abs_end = item_start_tick + (note.end_ppq.max(0) as u32);
                ImportMidiNote {
                    pitch: note.pitch,
                    velocity: note.velocity,
                    start_tick: abs_start,
                    duration_ticks: abs_end.saturating_sub(abs_start),
                    channel: note.channel,
                }
            })
            .collect()
    }

    fn make_source_fingerprint(
        source_track_name: &str,
        import_notes: &[ImportMidiNote],
        markers: &[MarkerEvent],
    ) -> String {
        let mut hasher = DefaultHasher::new();
        source_track_name.hash(&mut hasher);
        import_notes.len().hash(&mut hasher);
        for note in import_notes {
            note.pitch.hash(&mut hasher);
            note.velocity.hash(&mut hasher);
            note.start_tick.hash(&mut hasher);
            note.duration_ticks.hash(&mut hasher);
            note.channel.hash(&mut hasher);
        }
        markers.len().hash(&mut hasher);
        for marker in markers {
            marker.tick.hash(&mut hasher);
            marker.text.hash(&mut hasher);
        }
        format!("{:x}", hasher.finish())
    }

    fn build_chart_data(
        project: Project,
        source_track_name: String,
        notes: Vec<KeyflowMidiNote>,
        item_start_time: f64,
    ) -> Result<MidiChartData, String> {
        if notes.is_empty() {
            return Err("No MIDI notes found".to_string());
        }

        let item_start_tick = Self::time_to_tick(project, item_start_time);
        let import_notes = Self::import_notes(&notes, item_start_tick);

        let markers = Self::gather_markers(project);
        let source_fingerprint =
            Self::make_source_fingerprint(&source_track_name, &import_notes, &markers);
        let midi_file = MidiFile::from_parts(
            REAPER_PPQ,
            vec![MidiTrack {
                index: 0,
                name: Some(source_track_name.clone()),
                notes: import_notes.clone(),
                channel: None,
            }],
            vec![TempoEvent {
                tick: 0,
                microseconds_per_quarter: 500_000,
            }],
            vec![TimeSignatureEvent {
                tick: 0,
                numerator: 4,
                denominator: 4,
            }],
            markers,
            vec![Some(source_track_name.clone())],
            None,
        );
        let chart_text = generate_chart_text(&midi_file, &MidiChartConfig::default());

        let chords = detect_chords_from_midi_notes(&notes, MIN_CHORD_DURATION_PPQ)
            .into_iter()
            .map(|chord| MidiDetectedChord {
                symbol: chord.chord.to_string(),
                start_ppq: chord.start_ppq,
                end_ppq: chord.end_ppq,
                root_pitch: chord.root_pitch,
                velocity: chord.velocity,
            })
            .collect::<Vec<_>>();

        Ok(MidiChartData {
            source_track_name,
            source_fingerprint,
            chart_text,
            chords,
        })
    }
}

impl Default for ReaperMidiAnalysis {
    fn default() -> Self {
        Self::new()
    }
}

impl MidiAnalysisService for ReaperMidiAnalysis {
    async fn source_fingerprint(
        &self,
        request: MidiChartRequest,
    ) -> Result<String, String> {
        main_thread::query(move || {
            let Some(project) = Self::resolve_project(&request.project) else {
                return Err("Project not found".to_string());
            };
            let Some(track) = Self::find_track_by_tag(&project, request.track_tag.as_deref())
            else {
                let tag = request.track_tag.unwrap_or_else(|| "<none>".to_string());
                return Err(format!("No track matched tag '{}'", tag));
            };
            let track_name = track
                .name()
                .map(|name| name.to_str().to_string())
                .unwrap_or_else(|| "Unnamed Track".to_string());
            let Some((take, item_start_time)) = Self::get_first_midi_take(&track) else {
                return Err(format!("Track '{}' has no MIDI take", track_name));
            };
            let notes = Self::read_keyflow_notes(take);
            if notes.is_empty() {
                return Err("No MIDI notes found".to_string());
            }
            let item_start_tick = Self::time_to_tick(project, item_start_time);
            let import_notes = Self::import_notes(&notes, item_start_tick);
            let markers = Self::gather_markers(project);
            Ok(Self::make_source_fingerprint(
                &track_name,
                &import_notes,
                &markers,
            ))
        })
        .await
        .unwrap_or_else(|| Err("main thread unavailable".to_string()))
    }

    async fn generate_chart_data(
        &self,
        request: MidiChartRequest,
    ) -> Result<MidiChartData, String> {
        main_thread::query(move || {
            let Some(project) = Self::resolve_project(&request.project) else {
                return Err("Project not found".to_string());
            };
            let Some(track) = Self::find_track_by_tag(&project, request.track_tag.as_deref())
            else {
                let tag = request.track_tag.unwrap_or_else(|| "<none>".to_string());
                return Err(format!("No track matched tag '{}'", tag));
            };
            let track_name = track
                .name()
                .map(|name| name.to_str().to_string())
                .unwrap_or_else(|| "Unnamed Track".to_string());
            let Some((take, item_start_time)) = Self::get_first_midi_take(&track) else {
                return Err(format!("Track '{}' has no MIDI take", track_name));
            };
            let notes = Self::read_keyflow_notes(take);
            Self::build_chart_data(project, track_name, notes, item_start_time)
        })
        .await
        .unwrap_or_else(|| Err("main thread unavailable".to_string()))
    }
}

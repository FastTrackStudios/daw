//! Load a REAPER project (RPP) file and its audio into the AudioEngine.
//!
//! Parses the RPP to extract tracks, items, source offsets, and file paths,
//! then decodes each audio file and loads it into the mixer at the correct
//! timeline position.

use super::{AudioEngine, DecodedAudio, TrackHandle, decoder};
use dawfile_reaper::types::item::SourceType;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Information about a loaded track from an RPP file.
#[derive(Debug, Clone)]
pub struct LoadedTrack {
    /// Handle to control this track in the audio engine
    pub handle: TrackHandle,
    /// Track name from RPP
    pub track_name: String,
    /// Source audio file path
    pub source_path: PathBuf,
    /// Timeline position of this item in seconds
    pub position: f64,
    /// Duration of this item on the timeline in seconds
    pub length: f64,
    /// Source offset (where in the audio file playback starts) in seconds
    pub source_offset: f64,
    /// Duration of the decoded audio in seconds
    pub audio_duration: f64,
}

/// Result of loading an RPP project.
#[derive(Debug)]
pub struct LoadedProject {
    /// All loaded audio tracks
    pub tracks: Vec<LoadedTrack>,
    /// Project sample rate from RPP
    pub sample_rate: u32,
    /// Total project duration (end of last item)
    pub duration: f64,
    /// Files that failed to load (path, error reason)
    pub failed: Vec<(PathBuf, String)>,
}

/// Audio item extracted from RPP, before loading.
struct RppAudioItem {
    track_name: String,
    position: f64,
    length: f64,
    source_offset: f64,
    source_path: PathBuf,
    playrate: f64,
}

/// Load an RPP project file and all its audio into the engine.
///
/// Parses the RPP at `rpp_path`, resolves audio file paths relative to the RPP
/// directory, decodes each unique audio file once, then adds items to the engine
/// with correct timeline positions and source offsets.
pub fn load_rpp_project(
    engine: &AudioEngine,
    rpp_path: impl AsRef<Path>,
) -> Result<LoadedProject, String> {
    let rpp_path = rpp_path.as_ref();
    let rpp_dir = rpp_path
        .parent()
        .ok_or_else(|| format!("Cannot determine directory for: {}", rpp_path.display()))?;

    info!("Loading RPP: {}", rpp_path.display());

    // Parse RPP — only need tracks and items
    let options = dawfile_reaper::types::project::DecodeOptions {
        parse_tracks: true,
        parse_project_items: false,
        parse_project_envelopes: false,
        parse_project_fxchains: false,
        parse_markers_regions: false,
        parse_tempo_envelope: false,
        track_options: dawfile_reaper::types::track::TrackParseOptions {
            parse_items: true,
            parse_envelopes: false,
            parse_fx_chain: false,
        },
    };

    let content = std::fs::read_to_string(rpp_path)
        .map_err(|e| format!("Failed to read RPP file: {e}"))?;

    let project = dawfile_reaper::parse_project_text_with_options(&content, options)
        .map_err(|e| format!("Failed to parse RPP: {e}"))?;

    let sample_rate = project
        .properties
        .sample_rate
        .map(|(sr, _, _)| sr as u32)
        .unwrap_or(48000);

    info!(
        "Parsed RPP: {} tracks, sample rate: {} Hz",
        project.tracks.len(),
        sample_rate
    );

    // Extract all audio items
    let mut audio_items = Vec::new();
    for track in &project.tracks {
        for item in &track.items {
            // Get the selected take, or first take
            let take = item
                .takes
                .iter()
                .find(|t| t.is_selected)
                .or_else(|| item.takes.first());

            let Some(take) = take else { continue };
            let Some(source) = &take.source else { continue };

            // Only audio sources
            let is_audio = matches!(
                source.source_type,
                SourceType::Wave | SourceType::Mp3 | SourceType::Flac | SourceType::Vorbis
            );
            if !is_audio {
                continue;
            }

            // Resolve file path
            let file_path = if Path::new(&source.file_path).is_absolute() {
                PathBuf::from(&source.file_path)
            } else {
                rpp_dir.join(&source.file_path)
            };

            let playrate = item
                .playrate
                .as_ref()
                .map(|pr| pr.rate)
                .unwrap_or(1.0);

            audio_items.push(RppAudioItem {
                track_name: if track.name.is_empty() {
                    format!("Track {}", track.items.len())
                } else {
                    track.name.clone()
                },
                position: item.position,
                length: item.length,
                source_offset: take.slip_offset,
                source_path: file_path,
                playrate,
            });
        }
    }

    info!("Found {} audio items", audio_items.len());

    // Decode unique audio files (many items may reference the same file)
    let mut decoded_cache: HashMap<PathBuf, Option<DecodedAudio>> = HashMap::new();
    let mut failed = Vec::new();

    for item in &audio_items {
        if decoded_cache.contains_key(&item.source_path) {
            continue;
        }

        match std::fs::read(&item.source_path) {
            Ok(bytes) => {
                let ext = item
                    .source_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");

                match decoder::decode_audio_with_extension(&bytes, ext) {
                    Some(audio) => {
                        info!(
                            "Decoded: {} ({:.1}s, {} ch, {} Hz)",
                            item.source_path.display(),
                            audio.duration_seconds(),
                            audio.channels,
                            audio.sample_rate
                        );
                        decoded_cache.insert(item.source_path.clone(), Some(audio));
                    }
                    None => {
                        warn!("Failed to decode: {}", item.source_path.display());
                        failed.push((
                            item.source_path.clone(),
                            "Symphonia decode failed".to_string(),
                        ));
                        decoded_cache.insert(item.source_path.clone(), None);
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read: {} — {e}", item.source_path.display());
                failed.push((item.source_path.clone(), format!("Read failed: {e}")));
                decoded_cache.insert(item.source_path.clone(), None);
            }
        }
    }

    // Load items into the engine
    let engine_sample_rate = engine.sample_rate();
    let mut loaded_tracks = Vec::new();
    let mut max_end = 0.0f64;

    for item in &audio_items {
        let Some(Some(decoded)) = decoded_cache.get(&item.source_path) else {
            continue;
        };

        // Create a positioned audio buffer:
        // - Silence from t=0 to item.position
        // - Audio from source_offset for item.length
        let positioned = create_positioned_audio(
            decoded,
            item.position,
            item.length,
            item.source_offset,
            item.playrate,
            engine_sample_rate,
        );

        let audio_duration = positioned.duration_seconds();
        let handle = engine.add_track(positioned);

        loaded_tracks.push(LoadedTrack {
            handle,
            track_name: item.track_name.clone(),
            source_path: item.source_path.clone(),
            position: item.position,
            length: item.length,
            source_offset: item.source_offset,
            audio_duration,
        });

        max_end = max_end.max(item.position + item.length);
    }

    info!(
        "Loaded {} tracks, duration: {:.1}s, {} failed",
        loaded_tracks.len(),
        max_end,
        failed.len()
    );

    Ok(LoadedProject {
        tracks: loaded_tracks,
        sample_rate,
        duration: max_end,
        failed,
    })
}

/// Create a positioned audio buffer where silence fills the gap before the item
/// starts on the timeline, then the audio plays from the source offset.
fn create_positioned_audio(
    source: &DecodedAudio,
    position: f64,
    length: f64,
    source_offset: f64,
    playrate: f64,
    output_sample_rate: u32,
) -> DecodedAudio {
    let channels = source.channels as usize;
    if channels == 0 {
        return source.clone();
    }

    // Calculate frame counts at the output sample rate
    let silence_frames = (position * output_sample_rate as f64) as usize;
    let item_frames = (length * output_sample_rate as f64) as usize;
    let total_frames = silence_frames + item_frames;

    // Source offset in the source's sample rate
    let src_offset_frames =
        (source_offset * source.sample_rate as f64) as usize;

    let mut samples = vec![0.0f32; total_frames * channels];

    // Copy audio from source into the positioned buffer
    for frame in 0..item_frames {
        // Map output frame to source frame, accounting for playrate and sample rate
        let src_frame_f = src_offset_frames as f64
            + frame as f64 * playrate * source.sample_rate as f64 / output_sample_rate as f64;
        let src_frame = src_frame_f as usize;

        if src_frame >= source.frame_count() {
            break;
        }

        let dst_offset = (silence_frames + frame) * channels;
        let src_offset = src_frame * source.channels as usize;

        for ch in 0..channels {
            let src_ch = if ch < source.channels as usize { ch } else { 0 };
            if let Some(&sample) = source.samples.get(src_offset + src_ch) {
                samples[dst_offset + ch] = sample;
            }
        }
    }

    DecodedAudio {
        samples,
        channels: channels as u16,
        sample_rate: output_sample_rate,
    }
}

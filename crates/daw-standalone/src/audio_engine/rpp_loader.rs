//! Load a REAPER project (RPP) and its audio into the AudioEngine.
//!
//! The loader is I/O-agnostic: it takes RPP text and a closure that resolves
//! audio file paths to bytes. This works on native (read from disk) and
//! WASM (files provided via browser upload/fetch).

use super::{AudioEngine, DecodedAudio, TrackHandle, decoder};
use dawfile_reaper::types::item::SourceType;
use std::collections::HashMap;
use tracing::{info, warn};

/// Information about a loaded track from an RPP file.
#[derive(Debug, Clone)]
pub struct LoadedTrack {
    /// Handle to control this track in the audio engine
    pub handle: TrackHandle,
    /// Track name from RPP
    pub track_name: String,
    /// Source audio file name (as referenced in RPP)
    pub source_file: String,
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
    /// Files that failed to load (filename, error reason)
    pub failed: Vec<(String, String)>,
}

/// Audio item extracted from RPP, before loading.
struct RppAudioItem {
    track_name: String,
    position: f64,
    length: f64,
    source_offset: f64,
    /// The file path as written in the RPP (may be relative or absolute)
    source_file: String,
    playrate: f64,
}

/// Parse RPP text and extract audio items (no I/O).
fn extract_audio_items(rpp_text: &str) -> Result<(Vec<RppAudioItem>, u32), String> {
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

    let project = dawfile_reaper::parse_project_text_with_options(rpp_text, options)
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

    let mut audio_items = Vec::new();
    for track in &project.tracks {
        for item in &track.items {
            let take = item
                .takes
                .iter()
                .find(|t| t.is_selected)
                .or_else(|| item.takes.first());

            let Some(take) = take else { continue };
            let Some(source) = &take.source else { continue };

            let is_audio = matches!(
                source.source_type,
                SourceType::Wave | SourceType::Mp3 | SourceType::Flac | SourceType::Vorbis
            );
            if !is_audio {
                continue;
            }

            let playrate = item
                .playrate
                .as_ref()
                .map(|pr| pr.rate)
                .unwrap_or(1.0);

            audio_items.push(RppAudioItem {
                track_name: if track.name.is_empty() {
                    format!("Track {}", audio_items.len() + 1)
                } else {
                    track.name.clone()
                },
                position: item.position,
                length: item.length,
                source_offset: take.slip_offset,
                source_file: source.file_path.clone(),
                playrate,
            });
        }
    }

    info!("Found {} audio items", audio_items.len());
    Ok((audio_items, sample_rate))
}

/// Load an RPP project into the audio engine.
///
/// `rpp_text` is the RPP file content as a string.
/// `resolve_audio` is called for each unique audio file path referenced in the RPP,
/// and should return the file's raw bytes (or None if unavailable).
///
/// Works on all platforms — the caller handles I/O.
pub fn load_rpp(
    engine: &AudioEngine,
    rpp_text: &str,
    mut resolve_audio: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Result<LoadedProject, String> {
    let (audio_items, sample_rate) = extract_audio_items(rpp_text)?;

    // Decode unique audio files
    let mut decoded_cache: HashMap<String, Option<DecodedAudio>> = HashMap::new();
    let mut failed = Vec::new();

    for item in &audio_items {
        if decoded_cache.contains_key(&item.source_file) {
            continue;
        }

        match resolve_audio(&item.source_file) {
            Some(bytes) => {
                // Get extension from the file path
                let ext = item
                    .source_file
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_lowercase();

                match decoder::decode_audio_with_extension(&bytes, &ext) {
                    Some(audio) => {
                        info!(
                            "Decoded: {} ({:.1}s, {} ch, {} Hz)",
                            item.source_file,
                            audio.duration_seconds(),
                            audio.channels,
                            audio.sample_rate
                        );
                        decoded_cache.insert(item.source_file.clone(), Some(audio));
                    }
                    None => {
                        warn!("Failed to decode: {}", item.source_file);
                        failed.push((
                            item.source_file.clone(),
                            "Decode failed".to_string(),
                        ));
                        decoded_cache.insert(item.source_file.clone(), None);
                    }
                }
            }
            None => {
                warn!("Audio file not found: {}", item.source_file);
                failed.push((item.source_file.clone(), "File not provided".to_string()));
                decoded_cache.insert(item.source_file.clone(), None);
            }
        }
    }

    // Load items into the engine
    let engine_sample_rate = engine.sample_rate();
    let mut loaded_tracks = Vec::new();
    let mut max_end = 0.0f64;

    for item in &audio_items {
        let Some(Some(decoded)) = decoded_cache.get(&item.source_file) else {
            continue;
        };

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
            source_file: item.source_file.clone(),
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

/// Get the list of audio files referenced by an RPP project.
///
/// Useful for knowing which files to upload before calling `load_rpp`.
pub fn list_audio_files(rpp_text: &str) -> Result<Vec<String>, String> {
    let (items, _) = extract_audio_items(rpp_text)?;
    let mut files: Vec<String> = items.iter().map(|i| i.source_file.clone()).collect();
    files.sort();
    files.dedup();
    Ok(files)
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

    let silence_frames = (position * output_sample_rate as f64) as usize;
    let item_frames = (length * output_sample_rate as f64) as usize;
    let total_frames = silence_frames + item_frames;

    let src_offset_frames =
        (source_offset * source.sample_rate as f64) as usize;

    let mut samples = vec![0.0f32; total_frames * channels];

    for frame in 0..item_frames {
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

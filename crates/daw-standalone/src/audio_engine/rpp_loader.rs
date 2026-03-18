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
    /// Number of items merged into this track
    pub item_count: usize,
    /// Duration of the track's audio in seconds
    pub audio_duration: f64,
}

/// Result of loading an RPP project.
#[derive(Debug)]
pub struct LoadedProject {
    /// All loaded audio tracks (one per RPP track that has audio)
    pub tracks: Vec<LoadedTrack>,
    /// Project sample rate from RPP
    pub sample_rate: u32,
    /// Total project duration (end of last item)
    pub duration: f64,
    /// Files that failed to load (filename, error reason)
    pub failed: Vec<(String, String)>,
}

/// A single audio item within a track
struct AudioItem {
    position: f64,
    length: f64,
    source_offset: f64,
    source_file: String,
    playrate: f64,
}

/// All audio items grouped under one RPP track
struct TrackItems {
    track_name: String,
    items: Vec<AudioItem>,
}

/// Parse RPP text and extract audio items grouped by track.
fn extract_tracks_with_items(rpp_text: &str) -> Result<(Vec<TrackItems>, u32), String> {
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

    let mut result = Vec::new();

    for track in &project.tracks {
        let mut audio_items = Vec::new();

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

            let playrate = item.playrate.as_ref().map(|pr| pr.rate).unwrap_or(1.0);

            audio_items.push(AudioItem {
                position: item.position,
                length: item.length,
                source_offset: take.slip_offset,
                source_file: source.file_path.clone(),
                playrate,
            });
        }

        if !audio_items.is_empty() {
            let track_name = if track.name.is_empty() {
                format!("Track {}", result.len() + 1)
            } else {
                track.name.clone()
            };

            result.push(TrackItems {
                track_name,
                items: audio_items,
            });
        }
    }

    let total_items: usize = result.iter().map(|t| t.items.len()).sum();
    info!(
        "Found {} audio tracks with {} total items",
        result.len(),
        total_items
    );
    Ok((result, sample_rate))
}

/// Load an RPP project into the audio engine.
///
/// `rpp_text` is the RPP file content as a string.
/// `resolve_audio` is called for each unique audio file path referenced in the RPP,
/// and should return the file's raw bytes (or None if unavailable).
///
/// Items on the same RPP track are merged into a single audio buffer,
/// giving you one mixer track per RPP track.
pub fn load_rpp(
    engine: &AudioEngine,
    rpp_text: &str,
    mut resolve_audio: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Result<LoadedProject, String> {
    let (track_items, sample_rate) = extract_tracks_with_items(rpp_text)?;

    // Collect all unique audio file paths
    let mut all_files: Vec<String> = Vec::new();
    for ti in &track_items {
        for item in &ti.items {
            if !all_files.contains(&item.source_file) {
                all_files.push(item.source_file.clone());
            }
        }
    }

    // Decode unique audio files
    let mut decoded_cache: HashMap<String, Option<DecodedAudio>> = HashMap::new();
    let mut failed = Vec::new();

    for file_path in &all_files {
        match resolve_audio(file_path) {
            Some(bytes) => {
                let ext = file_path.rsplit('.').next().unwrap_or("").to_lowercase();

                match decoder::decode_audio_with_extension(&bytes, &ext) {
                    Some(audio) => {
                        info!(
                            "Decoded: {} ({:.1}s, {} ch, {} Hz)",
                            file_path,
                            audio.duration_seconds(),
                            audio.channels,
                            audio.sample_rate
                        );
                        decoded_cache.insert(file_path.clone(), Some(audio));
                    }
                    None => {
                        warn!("Failed to decode: {}", file_path);
                        failed.push((file_path.clone(), "Decode failed".to_string()));
                        decoded_cache.insert(file_path.clone(), None);
                    }
                }
            }
            None => {
                warn!("Audio file not found: {}", file_path);
                failed.push((file_path.clone(), "File not provided".to_string()));
                decoded_cache.insert(file_path.clone(), None);
            }
        }
    }

    // Build one merged audio buffer per track
    let engine_sample_rate = engine.sample_rate();
    let mut loaded_tracks = Vec::new();
    let mut max_end = 0.0f64;

    for ti in &track_items {
        // Find the end of the last item to determine total track length
        let track_end = ti
            .items
            .iter()
            .map(|item| item.position + item.length)
            .fold(0.0f64, f64::max);

        if track_end <= 0.0 {
            continue;
        }

        max_end = max_end.max(track_end);

        // Determine output channels (from first successfully decoded source)
        let out_channels = ti
            .items
            .iter()
            .filter_map(|item| decoded_cache.get(&item.source_file)?.as_ref())
            .map(|audio| audio.channels as usize)
            .next()
            .unwrap_or(2);

        // Create the merged buffer for this track
        let total_frames = (track_end * engine_sample_rate as f64) as usize;
        let mut samples = vec![0.0f32; total_frames * out_channels];

        let mut items_rendered = 0usize;

        for item in &ti.items {
            let Some(Some(decoded)) = decoded_cache.get(&item.source_file) else {
                continue;
            };

            let silence_frames = (item.position * engine_sample_rate as f64) as usize;
            let item_frames = (item.length * engine_sample_rate as f64) as usize;
            let src_offset_frames = (item.source_offset * decoded.sample_rate as f64) as usize;

            for frame in 0..item_frames {
                let dst_frame = silence_frames + frame;
                if dst_frame >= total_frames {
                    break;
                }

                let src_frame_f = src_offset_frames as f64
                    + frame as f64 * item.playrate * decoded.sample_rate as f64
                        / engine_sample_rate as f64;
                let src_frame = src_frame_f as usize;

                if src_frame >= decoded.frame_count() {
                    break;
                }

                let dst_offset = dst_frame * out_channels;
                let src_offset = src_frame * decoded.channels as usize;

                for ch in 0..out_channels {
                    let src_ch = if ch < decoded.channels as usize {
                        ch
                    } else {
                        0
                    };
                    if let Some(&sample) = decoded.samples.get(src_offset + src_ch) {
                        // Additive mix (items on the same track can overlap)
                        samples[dst_offset + ch] += sample;
                    }
                }
            }

            items_rendered += 1;
        }

        if items_rendered == 0 {
            continue;
        }

        let merged = DecodedAudio {
            samples,
            channels: out_channels as u16,
            sample_rate: engine_sample_rate,
        };

        let audio_duration = merged.duration_seconds();
        let handle = engine.add_track(merged);

        loaded_tracks.push(LoadedTrack {
            handle,
            track_name: ti.track_name.clone(),
            item_count: items_rendered,
            audio_duration,
        });
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

/// Get the list of unique audio files referenced by an RPP project.
pub fn list_audio_files(rpp_text: &str) -> Result<Vec<String>, String> {
    let (tracks, _) = extract_tracks_with_items(rpp_text)?;
    let mut files: Vec<String> = tracks
        .iter()
        .flat_map(|t| t.items.iter().map(|i| i.source_file.clone()))
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

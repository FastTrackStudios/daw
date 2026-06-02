//! Audio source materialization — turn `source_file_path` strings on
//! takes into decoded PCM stashed in `ProjectState.audio_sources`.
//!
//! The function takes a caller-supplied resolver closure so the same
//! code works on native (read from disk) and WASM (file bytes
//! provided via browser upload / fetch). Decoding is via Symphonia
//! (`feature = "decode"` or `feature = "audio"`).
//!
//! Typical use:
//!
//! ```ignore
//! use daw_standalone::audio_engine::materialize::{materialize_audio, MaterializeReport};
//!
//! let report: MaterializeReport = materialize_audio(&daw, &project_guid, |path| {
//!     std::fs::read(path).map_err(|e| e.to_string())
//! });
//! eprintln!("loaded {} sources, skipped {}", report.loaded, report.failed.len());
//! ```

use std::sync::Arc;

use super::decoder::{DecodedAudio, decode_audio_with_extension};
use crate::sync::Standalone;

/// Per-take materialization outcome.
#[derive(Debug, Default)]
pub struct MaterializeReport {
    /// Number of takes whose audio was decoded + stashed.
    pub loaded: usize,
    /// Number of takes that had no source path (MIDI / empty).
    pub skipped_no_source: usize,
    /// `(take_guid, reason)` for takes whose resolver call or decode
    /// failed. Caller decides how to surface — log warnings, fail
    /// the load, retry, etc.
    pub failed: Vec<(String, String)>,
}

/// Walk every take in `project_guid` and call `resolve(path)` for
/// each one whose source has a file path. Successful decodes land in
/// `ProjectState.audio_sources[take_guid]`.
pub fn materialize_audio<F>(
    daw: &Standalone,
    project_guid: &str,
    mut resolve: F,
) -> MaterializeReport
where
    F: FnMut(&str) -> Result<Vec<u8>, String>,
{
    let mut report = MaterializeReport::default();

    // Snapshot the list of (take_guid, source_path) outside the
    // project lock so the resolver doesn't run while we're holding
    // it. The resolver may do filesystem / network I/O.
    let pending: Vec<(String, String)> = daw
        .with_project(project_guid, |p| {
            let mut out = Vec::new();
            for take_list in p.takes.values() {
                for take in &take_list.takes {
                    if let Some(path) = &take.source_file_path
                        && !path.is_empty()
                    {
                        out.push((take.guid.clone(), path.clone()));
                    }
                }
            }
            out
        })
        .unwrap_or_default();

    for (take_guid, path) in pending {
        let bytes = match resolve(&path) {
            Ok(b) => b,
            Err(e) => {
                report.failed.push((take_guid, e));
                continue;
            }
        };
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let decoded = match decode_audio_with_extension(&bytes, &ext) {
            Some(d) => d,
            None => {
                report
                    .failed
                    .push((take_guid, format!("decode failed for {path}")));
                continue;
            }
        };
        attach_audio_source(daw, project_guid, &take_guid, decoded);
        report.loaded += 1;
    }

    // Count takes that legitimately had no source.
    let _ = daw.with_project(project_guid, |p| {
        for take_list in p.takes.values() {
            for take in &take_list.takes {
                let has_path = take
                    .source_file_path
                    .as_deref()
                    .map(|s| !s.is_empty())
                    .unwrap_or(false);
                if !has_path {
                    report.skipped_no_source += 1;
                }
            }
        }
    });

    report
}

/// Attach decoded audio for a specific take, bypassing the resolver
/// flow. Useful for tests + injecting synthesized audio.
pub fn attach_audio_source(
    daw: &Standalone,
    project_guid: &str,
    take_guid: &str,
    decoded: DecodedAudio,
) {
    let _ = daw.with_project_mut(project_guid, |p| {
        p.audio_sources
            .insert(take_guid.to_string(), Arc::new(decoded));
    });
}

/// Drop the decoded source for a take (e.g. unload to save memory
/// while keeping project structure).
pub fn detach_audio_source(daw: &Standalone, project_guid: &str, take_guid: &str) {
    let _ = daw.with_project_mut(project_guid, |p| {
        p.audio_sources.remove(take_guid);
    });
}

/// Convenience: materialize every take's audio via whatever
/// `BayFileResolver` is currently installed on the project Media Bay.
/// Same shape as [`materialize_audio`] but the resolver isn't passed
/// explicitly — the bay handles WASM vs native indirection.
///
/// Returns `Err` if no bay resolver is installed.
pub fn materialize_via_bay(
    daw: &Standalone,
    project_guid: &str,
) -> Result<MaterializeReport, String> {
    let bay = daw.media_bay();
    Ok(materialize_audio(daw, project_guid, |path| {
        bay.resolve_file(path)
    }))
}

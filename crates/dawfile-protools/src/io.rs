//! File I/O entry points.
//!
//! High-level functions for reading Pro Tools session files.

use crate::error::PtResult;
use crate::parse;
use crate::types::ProToolsSession;
use std::path::Path;

/// Read and parse a Pro Tools session file.
///
/// Supports `.ptf` (PT 5-9), `.ptx` (PT 10-12), and `.pts` (PT 5) formats.
///
/// The `target_sample_rate` parameter specifies the sample rate to convert
/// all positions to. Pass `0` to use the session's native sample rate
/// (i.e., no conversion).
///
/// # Example
///
/// ```no_run
/// let session = dawfile_protools::read_session("session.ptx", 48000)?;
/// println!("PT version: {}", session.version);
/// println!("Tracks: {}", session.audio_tracks.len());
/// println!("Regions: {}", session.audio_regions.len());
/// # Ok::<(), dawfile_protools::PtError>(())
/// ```
pub fn read_session(path: impl AsRef<Path>, target_sample_rate: u32) -> PtResult<ProToolsSession> {
    let mut data = std::fs::read(path.as_ref())?;

    // If target_sample_rate is 0, we'll detect it from the file and use 1:1
    let target = if target_sample_rate == 0 {
        0 // parse_session will detect and use session rate
    } else {
        target_sample_rate
    };

    parse::parse_session(&mut data, target)
}

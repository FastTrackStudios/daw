//! Pro Tools session file parser (.ptf, .ptx, .pts) — pure Rust.
//!
//! This crate parses Pro Tools session files (versions 5-12) and extracts:
//! - Audio file references (WAV/AIF filenames and lengths)
//! - Audio regions (timeline positions, sample offsets, lengths)
//! - Audio tracks with region assignments
//! - MIDI events, regions, and tracks
//! - Session metadata (version, sample rate)
//!
//! The parser handles XOR decryption, block tree construction, and
//! version-dependent parsing paths.
//!
//! # Supported formats
//!
//! | Extension | Pro Tools Version | Status |
//! |-----------|-------------------|--------|
//! | `.pts`    | 5                 | Supported |
//! | `.ptf`    | 5-9               | Supported |
//! | `.ptx`    | 10-12             | Supported |
//!
//! # Example
//!
//! ```no_run
//! let session = dawfile_protools::read_session("session.ptx", 48000)?;
//!
//! for track in &session.audio_tracks {
//!     println!("Track: {} ({} regions)", track.name, track.regions.len());
//! }
//!
//! for region in &session.midi_regions {
//!     println!("MIDI region: {} ({} events)", region.name, region.events.len());
//! }
//! # Ok::<(), dawfile_protools::PtError>(())
//! ```
//!
//! # Capability declaration
//!
//! ```rust
//! let support = dawfile_protools::feature_support();
//! assert!(support.can_read(daw_proto::Capability::Tracks));
//! assert!(!support.can_write(daw_proto::Capability::Tracks)); // read-only format
//! ```

#![deny(unsafe_code)]

pub mod block;
pub mod content_type;
pub mod convert;
pub mod cursor;
pub mod decrypt;
pub mod error;
pub mod io;
pub mod parse;
pub mod raw_block;
pub mod types;
pub mod write;

// Re-export the primary public API
pub use convert::{feature_support, session_summary};
pub use error::{PtError, PtResult};
pub use io::read_session;
pub use raw_block::{RawSession, parse_raw};
pub use types::*;
pub use write::write_session;

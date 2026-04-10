//! AuRg chunk parser — audio region pool segment (POOLSEG).
//!
//! Each top-level `AuRg` chunk contains one `POOLSEG::serialize` record
//! describing an audio region's placement and source reference.
//!
//! ## POOLSEG binary layout (serial, no struct-alignment padding)
//!
//! | Serial byte | Field                | Type     | Notes                                   |
//! |-------------|----------------------|----------|-----------------------------------------|
//! | 0–1         | `usedcount`          | u16 LE   |                                         |
//! | 2–3         | `poolflags`          | u16 LE   |                                         |
//! | 4           | `mTakeNumber`        | u8       |                                         |
//! | 5           | `segFlags`           | u8       |                                         |
//! | 6–9         | `offset`             | i32 LE   | Source file start in sample frames      |
//! | 10–13       | `offset_reserve`     | i32 LE   |                                         |
//! | 14–21       | `timeStamp`          | i64 LE   | Recording session timestamp (samples)   |
//! | 22–25       | `frames`             | i32 LE   | Region duration in sample frames        |
//! | 26–29       | unused               | 4 bytes  | Always zero                             |
//! | 30–33       | `timeStampFileOffset`| i32 LE   |                                         |
//! | 34–37       | `hotspot`            | i32 LE   |                                         |
//! | 38          | `mSelected`          | u8       |                                         |
//! | 39          | `mLock`              | u8       |                                         |
//! | 40–41       | `mElasticMode`       | 2 bytes  |                                         |
//! | 42–49       | `mModificationDate`  | i64 LE   | Windows FILETIME (100-ns since 1601)    |
//! | 50–73       | `__unused_bytes`     | 24 bytes |                                         |
//! | 74–75       | name `length`        | u16 LE   | Number of ASCII/UTF-8 bytes             |
//! | 76..        | name                 | UTF-8    | `length` bytes (not null-terminated)    |
//! | +0–1        | `mFadeType`          | u8 ×2    |                                         |
//! | …           | fade/loop fields     | various  |                                         |
//! | +N+0–7      | `oldindex`…          | u16      |                                         |
//! | +N+8–15     | `startClockComp`     | i64 LE   | Arrangement start position in ticks     |
//! | +N+16–23    | `endClockComp`       | i64 LE   | Arrangement end position in ticks       |
//!
//! ### Offset to `startClockComp`
//!
//! After the variable-length name, 49 bytes of fixed fields appear before
//! `oldindex` (u16), then `startClockComp` (i64) at `76 + name_len + 49`.
//! See [`POOLSEG_START_CLOCK_AFTER_NAME`].
//!
//! Empirically confirmed against the `FileDecrypt.logicx` fixture:
//! `startClockComp = 0` for clips placed at bar 1, beat 1. ✓

/// Bytes from end-of-name to `startClockComp` (inclusive of oldindex).
///
/// Fields: mFadeType(1)+mFadeCurve(1)+indexLeftFile(4)+offsetLeftFile(4)+
///         indexRightFile(4)+offsetRightFile(4)+mFadeID(4)+loopStart(4)+
///         loopLen(4)+keyNote(1)+detune(1)+mFadeStyle(1)+mTakeNumberOnLane(1)+
///         mFadeNumSourceFrames(4)+mFadeNumSourceFramesRight(4)+userTimeStamp(4)+
///         mGainLeftFile(1)+mGainRightFile(1)+oldindex(2)
///         = 2+20+8+4+4+12+2+2 = 51 bytes
const POOLSEG_FIXED_BEFORE_CLOCK: usize = 51;

/// Key data extracted from an `AuRg` chunk payload.
#[derive(Debug, Clone)]
pub struct AudioRegion {
    /// User-visible name of this region (e.g. `"Audio Track 1 #01"`).
    pub name: String,
    /// Take number within a Take Folder.
    ///
    /// - `0` = comp result layer (the composite output; `start_ticks`/`end_ticks` are 0)
    /// - `1..N` = individual recorded takes
    pub take_number: u8,
    /// Source file start offset in sample frames.
    pub source_offset_frames: i32,
    /// Region duration in sample frames.
    pub duration_frames: i32,
    /// Arrangement start position in ticks (Logic clock, 240 PPQ).
    /// Zero = bar 1 beat 1. Convert to beats by dividing by 240.
    /// For take regions (take_number ≥ 1): the start of the active comp range.
    pub start_ticks: i64,
    /// Arrangement end position in ticks (may be zero for non-comped regions).
    /// For take regions (take_number ≥ 1): the end of the active comp range.
    pub end_ticks: i64,
    /// Whether this region is selected in the project.
    pub selected: bool,
    /// Whether this region is locked.
    pub locked: bool,
}

/// Parse an `AuRg` chunk payload into an [`AudioRegion`].
///
/// Returns `None` if the payload is too short.
pub fn parse_aurg(data: &[u8]) -> Option<AudioRegion> {
    // Minimum fixed header before name: 76 bytes (name_length at byte 74, minimum 0-len name)
    if data.len() < 76 {
        return None;
    }

    let take_number = data[4];
    let source_offset_frames = i32::from_le_bytes(data[6..10].try_into().ok()?);
    let duration_frames = i32::from_le_bytes(data[22..26].try_into().ok()?);
    let selected = data[38] != 0;
    let locked = data[39] != 0;

    // Name: u16 length at bytes 74–75, then UTF-8 bytes
    let name_len = u16::from_le_bytes([data[74], data[75]]) as usize;
    let name_start = 76;
    let name_end = name_start + name_len;

    if data.len() < name_end {
        return None;
    }

    let name = String::from_utf8_lossy(&data[name_start..name_end]).into_owned();

    // startClockComp: name_end + POOLSEG_FIXED_BEFORE_CLOCK bytes
    let clock_start = name_end + POOLSEG_FIXED_BEFORE_CLOCK;
    let clock_end = clock_start + 8;
    let end_clock_start = clock_end;
    let end_clock_end = end_clock_start + 8;

    let start_ticks = if data.len() >= clock_end {
        i64::from_le_bytes(data[clock_start..clock_end].try_into().ok()?)
    } else {
        0
    };

    let end_ticks = if data.len() >= end_clock_end {
        i64::from_le_bytes(data[end_clock_start..end_clock_end].try_into().ok()?)
    } else {
        0
    };

    Some(AudioRegion {
        name,
        take_number,
        source_offset_frames,
        duration_frames,
        start_ticks,
        end_ticks,
        selected,
        locked,
    })
}

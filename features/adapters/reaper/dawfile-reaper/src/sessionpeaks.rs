//! `.sessionpeaks` — the session's own waveform cache, in REAPER's format.
//!
//! # The file IS RPKN
//!
//! A `.sessionpeaks` file is byte-for-byte a `.reapeaks` file: the same
//! `RPKN` magic, the same header, the same `sr/300` ×15 ×20 mipmap ladder,
//! the same size-and-mtime source stamp, the same interleaved
//! `(max, min)` i16 pairs. It is **not** a superset. Rename one and
//! REAPER reads it; rename a REAPER one and we read it.
//! [`super::reapeaks`] is the single codec for both.
//!
//! That is a deliberate refusal to extend. The temptation is to add a
//! fourth, finer level, a content hash instead of the size-and-mtime
//! stamp, or a per-channel RMS band — all of which would be useful, and
//! any of which would turn the file into something REAPER opens as
//! garbage. The interchange is worth more than the extras: these sessions
//! are worked on in REAPER *and* in the session app, often on the same
//! folder, and a cache only one of them can use is a cache that gets
//! built twice.
//!
//! What differs is only **where the file sits and what it is called**:
//!
//! | writer | path |
//! |---|---|
//! | session | `Media/Peaks/Bass.wav.sessionpeaks` |
//! | REAPER (peaks folder) | `Media/peaks/Bass.wav.reapeaks` |
//! | REAPER (beside media) | `Media/Bass.wav.reapeaks` |
//!
//! `Media/Peaks/` mirrors `Media/Proxies/` — one folder per derived
//! artifact, beside the media it derives from, inside the session folder
//! so it is versioned, synced and *served* with everything else in it.
//! That last part is the point of the separate extension: a share link
//! serves any non-media file whole, so the browser fetches
//! `<link>/doc/Media/Peaks/Bass.wav.sessionpeaks` and draws real
//! waveforms without ever touching the audio.
//!
//! [`read_any`] looks in all three places, newest-first by preference, so
//! a session REAPER has already scanned costs nothing to open.

use std::path::{Path, PathBuf};

use crate::reapeaks::{ReaPeaks, ReaPeaksError};

/// The extension the session writes. See the module docs: the *bytes*
/// are `.reapeaks`, only the name is ours.
pub const EXTENSION: &str = "sessionpeaks";

/// The folder the session writes into, beside the media — the same shape
/// as `Media/Proxies/`.
pub const FOLDER: &str = "Peaks";

/// REAPER's own extension, which [`read_any`] also accepts.
pub const REAPER_EXTENSION: &str = "reapeaks";

/// REAPER's peaks folder, lowercase as REAPER writes it.
pub const REAPER_FOLDER: &str = "peaks";

/// Where the session writes `media`'s cache:
/// `Media/Bass.wav` → `Media/Peaks/Bass.wav.sessionpeaks`.
///
/// The media file's *whole* name is kept, extension included, so
/// `Bass.wav` and `Bass.ogg` do not collide — REAPER's rule, and the
/// reason `Media/Proxies/Bass.ogg` (which drops it) is not the model
/// here.
///
/// `None` for a path with no file name, and for a file already inside a
/// peaks folder — nothing caches a cache.
#[must_use]
pub fn cache_path(media: &Path) -> Option<PathBuf> {
    let dir = media.parent()?;
    let name = media.file_name()?;
    if dir
        .file_name()
        .is_some_and(|n| n == FOLDER || n == REAPER_FOLDER)
    {
        return None;
    }
    let mut file = name.to_owned();
    file.push(".");
    file.push(EXTENSION);
    Some(dir.join(FOLDER).join(file))
}

/// Every place a cache for `media` may already be, in the order to try:
/// ours, then REAPER's peaks folder, then REAPER's sidecar beside the
/// media.
///
/// Ours first because it is the one this tool keeps current; REAPER's
/// two after, because a session that has been opened in REAPER already
/// has them and rescanning gigabytes to write a duplicate would be
/// silly.
#[must_use]
pub fn candidates(media: &Path) -> Vec<PathBuf> {
    let Some(dir) = media.parent() else {
        return Vec::new();
    };
    let Some(name) = media.file_name() else {
        return Vec::new();
    };
    let with = |ext: &str| {
        let mut file = name.to_owned();
        file.push(".");
        file.push(ext);
        file
    };
    match cache_path(media) {
        Some(ours) => vec![
            ours,
            dir.join(REAPER_FOLDER).join(with(REAPER_EXTENSION)),
            dir.join(with(REAPER_EXTENSION)),
        ],
        // Already inside a peaks folder: only the sidecar rule applies.
        None => vec![dir.join(with(REAPER_EXTENSION))],
    }
}

/// The first readable cache for `media`, and where it was found.
///
/// A file that is present but unparseable is skipped rather than
/// returned as an error: the next candidate may well be good, and a
/// truncated cache (an interrupted REAPER scan) is a thing that happens.
#[must_use]
pub fn read_any(media: &Path) -> Option<(PathBuf, ReaPeaks)> {
    candidates(media)
        .into_iter()
        .find_map(|path| ReaPeaks::read(&path).ok().map(|peaks| (path, peaks)))
}

/// The stamp `media` should be carrying: its size and its mtime in unix
/// seconds, packed the way REAPER packs them ([`crate::reapeaks::stamp`]).
#[must_use]
pub fn media_stamp(media: &Path) -> Option<u64> {
    let meta = std::fs::metadata(media).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(crate::reapeaks::stamp(meta.len(), mtime))
}

/// Whether `peaks` still describes `media`: same size, same mtime, and
/// there is something in it.
///
/// This is REAPER's own test, which is why a cache REAPER built validates
/// here and a cache built here validates there. A file whose mtime has
/// moved but whose bytes have not (a copy, a sync, a restore from a
/// backup) reads as stale — the conservative direction, costing one
/// rescan rather than drawing last week's waveform.
#[must_use]
pub fn is_current(peaks: &ReaPeaks, media: &Path) -> bool {
    peaks.levels.first().is_some_and(|l| l.count > 0)
        && media_stamp(media).is_some_and(|s| peaks.source_stamp == s)
}

/// Write `peaks` as `media`'s `.sessionpeaks`, creating `Media/Peaks/`,
/// and return where it landed.
///
/// Written aside and renamed into place, so a sync agent watching the
/// folder never picks up half a cache — the same rule `session proxies`
/// follows.
///
/// # Errors
///
/// The peaks folder could not be created or written.
pub fn write(media: &Path, peaks: &ReaPeaks) -> std::io::Result<PathBuf> {
    let path = cache_path(media).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} has no peaks folder", media.display()),
        )
    })?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension(format!("{EXTENSION}.partial"));
    std::fs::write(&partial, peaks.to_bytes())?;
    std::fs::rename(&partial, &path)?;
    Ok(path)
}

/// What can go wrong turning media into peaks.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse: {0}")]
    Parse(#[from] ReaPeaksError),
    #[error("{0} is not audio this can scan (wav, ogg)")]
    Unsupported(String),
    #[error("decode: {0}")]
    Decode(String),
}

/// Peaks for an Ogg Vorbis stream held in memory — the browser's path,
/// and the CLI's when a session has proxies but no originals.
///
/// Forward-only: the stream is decoded once, packet by packet, into
/// [`crate::reapeaks::PeaksBuilder`]. Nothing is held but the compressed
/// bytes and one packet.
///
/// `source_stamp` is left at 0 — an Ogg proxy's own size and mtime say
/// nothing about the WAV behind it, and in a browser there is neither.
/// Callers that have the original stamp it themselves.
///
/// Note that a proxy's frame count may exceed the WAV's by the encoder's
/// final-block padding (silence), so a proxy-built cache can carry one
/// more peak than a WAV-built one. Both draw the same waveform.
///
/// # Errors
///
/// Not an Ogg Vorbis stream, or corrupt past what the decoder can skip.
#[cfg(feature = "peaks-ogg")]
pub fn build_from_ogg(bytes: std::sync::Arc<[u8]>) -> Result<ReaPeaks, BuildError> {
    use fts_sample::ogg_stream::OggStream;

    let mut stream = OggStream::open(bytes).map_err(|e| BuildError::Decode(e.to_string()))?;
    let mut builder =
        crate::reapeaks::PeaksBuilder::new(usize::from(stream.channels()), stream.sample_rate());
    let mut block: Vec<f32> = Vec::new();
    loop {
        block.clear();
        match stream
            .decode(&mut block)
            .map_err(|e| BuildError::Decode(e.to_string()))?
        {
            Some(_) => builder.push_interleaved(&block),
            None => break,
        }
    }
    Ok(builder.finish())
}

/// Peaks for a WAV on disk, read through a memory map.
///
/// One sequential pass; the pages the scan touches are the only thing
/// resident, so a 700 MB stem costs no RAM worth naming. Stamped with
/// the file's mtime, so [`is_current`] can invalidate it later.
///
/// # Errors
///
/// The file could not be opened, or is not PCM a map can address.
#[cfg(feature = "peaks-wav")]
pub fn build_from_wav(media: &Path) -> Result<ReaPeaks, BuildError> {
    use fts_sample::mapped::PcmFile;

    let pcm = PcmFile::open(media).map_err(|e| BuildError::Decode(e.to_string()))?;
    let mut peaks = ReaPeaks::compute(
        usize::from(pcm.channels().max(1)),
        pcm.sample_rate(),
        pcm.frames(),
        |frame, channel| pcm.sample(frame, channel),
    );
    peaks.source_stamp = media_stamp(media).unwrap_or(0);
    Ok(peaks)
}

/// Peaks for a media file, by extension: WAV through the map, Ogg
/// through the decoder.
///
/// # Errors
///
/// The extension is neither, or the scan failed.
#[cfg(all(feature = "peaks-wav", feature = "peaks-ogg"))]
pub fn build(media: &Path) -> Result<ReaPeaks, BuildError> {
    let ext = media
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "wav" | "wave" => build_from_wav(media),
        "ogg" | "oga" => {
            let bytes: std::sync::Arc<[u8]> = std::fs::read(media)?.into();
            let mut peaks = build_from_ogg(bytes)?;
            peaks.source_stamp = media_stamp(media).unwrap_or(0);
            Ok(peaks)
        }
        _ => Err(BuildError::Unsupported(media.display().to_string())),
    }
}

/// `media`'s peaks, from the cache when one is current, else scanned and
/// written. Returns the peaks and whether they were built (rather than
/// read), which is what a CLI reports.
///
/// # Errors
///
/// The scan failed. A cache that cannot be *written* is not an error —
/// a read-only media folder just means the next run scans again.
#[cfg(all(feature = "peaks-wav", feature = "peaks-ogg"))]
pub fn ensure(media: &Path, force: bool) -> Result<(ReaPeaks, bool), BuildError> {
    if !force
        && let Some((_, peaks)) = read_any(media)
        && is_current(&peaks, media)
    {
        return Ok((peaks, false));
    }
    let peaks = build(media)?;
    if let Err(err) = write(media, &peaks) {
        tracing::warn!(
            peaks.media = %media.display(),
            peaks.write_error = %err,
            "sessionpeaks write failed; the cache stays in memory"
        );
    }
    Ok((peaks, true))
}

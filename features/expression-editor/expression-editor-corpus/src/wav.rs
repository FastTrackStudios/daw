//! Reading WAV headers, because the kit pages do not state them.
//!
//! None of the three DrumGizmo wiki pages says a sample rate or a bit
//! depth, so #158 could only record the question. The answer, read off
//! the actual headers:
//!
//! | Kit | Rate | Sample format | Channels per file |
//! |---|---|---|---|
//! | CrocellKit 1.1 | 48 000 Hz | 32-bit IEEE float | 15 |
//! | DRSKit 2.1 | 44 100 Hz | 32-bit IEEE float | 13 |
//!
//! Three consequences, each of which would have bitten later:
//!
//! 1. **The two kits are at different rates.** A harness that assumes
//!    one rate per corpus is wrong on its second kit — the same finding
//!    the material inventory (#159) made about the demo project.
//! 2. **The samples are float, not integer.** `format` here reports
//!    that, because a reader that assumes PCM silently produces noise.
//! 3. **One file per hit holds the whole array**, interleaved. So a
//!    "sample" is a 15-channel object and the bleed is inside it; there
//!    is no per-mic file to load in isolation.
//!
//! This module reads the header and nothing else — it never decodes
//! audio — so probing a whole kit is a few thousand short seeks rather
//! than gigabytes of I/O.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// How samples are encoded, as the `fmt ` chunk's format tag reports
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Tag 1 — integer PCM.
    Pcm,
    /// Tag 3 — IEEE float. What the DrumGizmo kits are.
    Float,
    /// Tag 0xFFFE — the real format hides in the extension's sub-format
    /// GUID, whose first two bytes are one of the tags above.
    Extensible(u16),
    /// Anything else, reported rather than rejected: the point of a
    /// probe is to say what is there.
    Other(u16),
}

impl Format {
    fn from_tag(tag: u16) -> Self {
        match tag {
            1 => Self::Pcm,
            3 => Self::Float,
            0xFFFE => Self::Extensible(0),
            other => Self::Other(other),
        }
    }

    /// The effective encoding, resolving an extensible header to the
    /// sub-format it actually carries.
    pub fn effective(self) -> Self {
        match self {
            Self::Extensible(sub) => Self::from_tag(sub),
            other => other,
        }
    }
}

/// What a WAV file's `fmt ` chunk says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WavHeader {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub format: Format,
}

/// Read one file's header.
///
/// Walks the RIFF chunk list rather than assuming `fmt ` sits at byte
/// 12. It usually does, but a file with a `JUNK` or `bext` chunk in
/// front is perfectly legal and a fixed offset reads garbage from it.
pub fn probe(path: &Path) -> io::Result<WavHeader> {
    let mut file = File::open(path)?;
    let mut riff = [0u8; 12];
    file.read_exact(&mut riff)?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: not a RIFF/WAVE file", path.display()),
        ));
    }

    let mut header = [0u8; 8];
    loop {
        if file.read_exact(&mut header).is_err() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: no fmt chunk", path.display()),
            ));
        }
        let id = [header[0], header[1], header[2], header[3]];
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        if &id == b"fmt " {
            let mut fmt = vec![0u8; size as usize];
            file.read_exact(&mut fmt)?;
            return parse_fmt(&fmt, path);
        }
        // Chunks are word-aligned: an odd size is followed by a pad
        // byte that is not counted in the size.
        let skip = size as i64 + (size as i64 & 1);
        file.seek(SeekFrom::Current(skip))?;
    }
}

fn parse_fmt(fmt: &[u8], path: &Path) -> io::Result<WavHeader> {
    if fmt.len() < 16 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: fmt chunk is {} bytes, want 16", path.display(), fmt.len()),
        ));
    }
    let tag = u16::from_le_bytes([fmt[0], fmt[1]]);
    let mut format = Format::from_tag(tag);
    // WAVE_FORMAT_EXTENSIBLE: the sub-format GUID starts at offset 24
    // of the chunk and its first two bytes are the real format tag.
    if let Format::Extensible(_) = format
        && fmt.len() >= 26
    {
        format = Format::Extensible(u16::from_le_bytes([fmt[24], fmt[25]]));
    }
    Ok(WavHeader {
        channels: u16::from_le_bytes([fmt[2], fmt[3]]),
        sample_rate: u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]),
        bits_per_sample: u16::from_le_bytes([fmt[14], fmt[15]]),
        format: format.effective(),
    })
}

/// Every `.wav` under a directory, with its header.
///
/// Sorted by path so two runs over the same kit report in the same
/// order — a probe whose output reshuffles cannot be diffed between
/// kit versions, which is half of what it is for.
pub fn probe_tree(root: &Path) -> io::Result<Vec<(PathBuf, WavHeader)>> {
    let mut paths = Vec::new();
    collect(root, &mut paths)?;
    paths.sort();
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        match probe(&path) {
            Ok(header) => out.push((path, header)),
            // A kit with one unreadable file should still report the
            // other seven hundred.
            Err(e) => eprintln!("  skipped {}: {e}", path.display()),
        }
    }
    Ok(out)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    if dir.is_file() {
        out.push(dir.to_path_buf());
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
            out.push(path);
        }
    }
    Ok(())
}

/// Read one channel of a file as `f64`, with its sample rate.
///
/// `channel` picks a mic out of an interleaved file; `None` sums to
/// mono. Summing is offered but is rarely what you want here — mixing
/// fifteen mics together is a way to *manufacture* the bleed the corpus
/// exists to measure honestly, so the detector should normally be run
/// per channel.
pub fn read_channel(path: &Path, channel: Option<usize>) -> io::Result<(Vec<f64>, f64)> {
    let reader = hound::WavReader::open(path).map_err(to_io)?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let wanted = channel.unwrap_or(0);
    if channel.is_some_and(|c| c >= channels) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{}: channel {wanted} of {channels}", path.display()),
        ));
    }

    let scale = match spec.sample_format {
        hound::SampleFormat::Float => 1.0,
        // `bits_per_sample` is the meaningful width; hound hands back
        // the full container, so the divisor is the width's full scale.
        hound::SampleFormat::Int => (1i64 << (spec.bits_per_sample - 1)) as f64,
    };
    let raw: Vec<f64> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .map(|s| s.map(|v| v as f64))
            .collect::<Result<_, _>>()
            .map_err(to_io)?,
        hound::SampleFormat::Int => reader
            .into_samples::<i32>()
            .map(|s| s.map(|v| v as f64 / scale))
            .collect::<Result<_, _>>()
            .map_err(to_io)?,
    };

    let frames = raw.len() / channels;
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let frame = &raw[f * channels..(f + 1) * channels];
        out.push(match channel {
            Some(c) => frame[c],
            None => frame.iter().sum::<f64>() / channels as f64,
        });
    }
    Ok((out, spec.sample_rate as f64))
}

/// Write a mono `f64` buffer as 32-bit float, the format the kits
/// themselves are in.
pub fn write_mono(path: &Path, samples: &[f64], sample_rate: f64) -> io::Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sample_rate as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(to_io)?;
    for &s in samples {
        writer.write_sample(s as f32).map_err(to_io)?;
    }
    writer.finalize().map_err(to_io)
}

fn to_io(e: hound::Error) -> io::Error {
    match e {
        hound::Error::IoError(e) => e,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

/// The distinct headers in a probe, with how many files carry each.
///
/// The useful form of a kit probe: seven hundred identical lines say
/// nothing, and "all 758 files are 15ch/48000/float32" is the whole
/// answer. A kit that comes back with two rows has a mixed-rate
/// problem worth knowing about before rendering.
pub fn summarize(probed: &[(PathBuf, WavHeader)]) -> Vec<(WavHeader, usize)> {
    let mut out: Vec<(WavHeader, usize)> = Vec::new();
    for (_, header) in probed {
        match out.iter_mut().find(|(h, _)| h == header) {
            Some((_, n)) => *n += 1,
            None => out.push((*header, 1)),
        }
    }
    out.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    out
}

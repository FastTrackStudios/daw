//! Header-only audio probing — reads the RIFF chunk list, never decodes
//! samples, so scanning a whole library (IR banks, DrumGizmo kits) costs a
//! few short reads per file rather than gigabytes of I/O.
//!
//! Walks the chunk list rather than assuming `fmt ` sits at byte 12. It
//! usually does, but a file with a `JUNK` or `bext` chunk in front is
//! perfectly legal and a fixed offset reads garbage from it.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::SamplerError;

/// How samples are encoded, as the `fmt ` chunk's format tag reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavFormat {
    /// Tag 1 — integer PCM.
    Pcm,
    /// Tag 3 — IEEE float.
    Float,
    /// Tag 0xFFFE — the real format hides in the extension's sub-format
    /// GUID, whose first two bytes are one of the tags above.
    Extensible(u16),
    /// Anything else, reported rather than rejected: the point of a
    /// probe is to say what is there.
    Other(u16),
}

impl WavFormat {
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

/// What a file's header says, without decoding any audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    /// Frames in the `data` chunk — 0 when the chunk is absent or the
    /// header gives no usable frame size.
    pub num_frames: u64,
    /// Byte offset of the `data` chunk's samples within the file — 0 when
    /// the chunk is absent (sample data can never start at byte 0). This
    /// is what a memory-mapper needs to address samples directly.
    pub data_offset: u64,
    /// The effective encoding (extensible headers resolved to their
    /// sub-format).
    pub format: WavFormat,
}

impl AudioInfo {
    /// Duration in seconds (0.0 when the header gives no rate or length).
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.num_frames as f64 / self.sample_rate as f64
        }
    }
}

/// Read one WAV file's header. Header-only: no sample decode.
pub fn probe(path: &Path) -> Result<AudioInfo, SamplerError> {
    let mut file = File::open(path)?;
    let mut riff = [0u8; 12];
    file.read_exact(&mut riff)?;
    if &riff[0..4] != b"RIFF" || &riff[8..12] != b"WAVE" {
        return Err(SamplerError::Probe(format!(
            "{}: not a RIFF/WAVE file",
            path.display()
        )));
    }

    let mut fmt: Option<Vec<u8>> = None;
    let mut data_size: Option<u64> = None;
    let mut data_offset: u64 = 0;
    let mut header = [0u8; 8];
    loop {
        if file.read_exact(&mut header).is_err() {
            break;
        }
        let id = [header[0], header[1], header[2], header[3]];
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        match &id {
            b"fmt " => {
                let mut buf = vec![0u8; size as usize];
                file.read_exact(&mut buf)?;
                // Chunks are word-aligned: an odd size is followed by a
                // pad byte that is not counted in the size.
                if size & 1 == 1 {
                    file.seek(SeekFrom::Current(1))?;
                }
                fmt = Some(buf);
            }
            b"data" => {
                data_size = Some(u64::from(size));
                data_offset = file.stream_position()?;
                file.seek(SeekFrom::Current(size as i64 + (size as i64 & 1)))?;
            }
            _ => {
                file.seek(SeekFrom::Current(size as i64 + (size as i64 & 1)))?;
            }
        }
        if fmt.is_some() && data_size.is_some() {
            break;
        }
    }

    let Some(fmt) = fmt else {
        return Err(SamplerError::Probe(format!(
            "{}: no fmt chunk",
            path.display()
        )));
    };
    parse_fmt(&fmt, data_size.unwrap_or(0), data_offset, path)
}

fn parse_fmt(
    fmt: &[u8],
    data_size: u64,
    data_offset: u64,
    path: &Path,
) -> Result<AudioInfo, SamplerError> {
    if fmt.len() < 16 {
        return Err(SamplerError::Probe(format!(
            "{}: fmt chunk is {} bytes, want 16",
            path.display(),
            fmt.len()
        )));
    }
    let tag = u16::from_le_bytes([fmt[0], fmt[1]]);
    let mut format = WavFormat::from_tag(tag);
    // WAVE_FORMAT_EXTENSIBLE: the sub-format GUID starts at offset 24
    // of the chunk and its first two bytes are the real format tag.
    if let WavFormat::Extensible(_) = format {
        if fmt.len() >= 26 {
            format = WavFormat::Extensible(u16::from_le_bytes([fmt[24], fmt[25]]));
        }
    }
    let channels = u16::from_le_bytes([fmt[2], fmt[3]]);
    let bits_per_sample = u16::from_le_bytes([fmt[14], fmt[15]]);
    // Frame size: trust `block_align` when it is sane, else compute it
    // from channels × sample width.
    let block_align = u16::from_le_bytes([fmt[12], fmt[13]]) as u64;
    let frame_bytes = if block_align > 0 {
        block_align
    } else {
        u64::from(channels) * u64::from(bits_per_sample / 8)
    };
    let num_frames = if frame_bytes > 0 {
        data_size / frame_bytes
    } else {
        0
    };
    Ok(AudioInfo {
        channels,
        sample_rate: u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]),
        bits_per_sample,
        num_frames,
        data_offset,
        format: format.effective(),
    })
}

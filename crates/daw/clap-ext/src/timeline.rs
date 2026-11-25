//! `com.fasttrackstudio.host-timeline/1`'s data: a track's whole MIDI
//! content and the tempo map in project quarter notes ([`Timeline`]), and
//! the byte stream `read` carries it in ([`Timeline::encode`] /
//! [`Timeline::decode`]). The contract is `docs/host-timeline.md`.

use crate::ExtError;
use crate::sys::{
    FTS_TIMELINE_CC, FTS_TIMELINE_CHANNEL_PRESSURE, FTS_TIMELINE_MAGIC, FTS_TIMELINE_PITCH_BEND,
    FTS_TIMELINE_POLY_PRESSURE, FTS_TIMELINE_PROGRAM, FTS_TIMELINE_TEMPO_RAMP, fts_timeline_ctrl,
    fts_timeline_header, fts_timeline_note, fts_timeline_tempo,
};

/// One note of the track, as the song plays it. Positions are project
/// quarter notes from project time zero (the document epoch).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub start_qn: f64,
    pub end_qn: f64,
    /// 0..15.
    pub channel: u8,
    pub key: u8,
    /// 1..127.
    pub velocity: u8,
    /// 0..127; 64 when the source has none.
    pub release_velocity: u8,
}

/// What a controller event is, with its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Cc {
        controller: u8,
        value: u8,
    },
    /// 0..16383, 8192 centre.
    PitchBend(u16),
    ChannelPressure(u8),
    PolyPressure {
        key: u8,
        value: u8,
    },
    Program(u8),
}

/// A controller event at `qn` on `channel`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ctrl {
    pub qn: f64,
    pub channel: u8,
    pub control: Control,
}

/// How the tempo goes from a point to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TempoShape {
    /// Constant until the next point, then it steps.
    #[default]
    Step,
    /// Linear in quarter notes to the next point's bpm.
    Ramp,
}

/// One tempo point. `seconds` is the host's own time of `qn`, so a reader
/// never integrates the map to find where the host is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub qn: f64,
    pub seconds: f64,
    pub bpm: f64,
    pub numerator: u16,
    pub denominator: u16,
    pub shape: TempoShape,
}

/// A track's timeline: everything a plugin needs to play the whole song's
/// part ahead of the playhead.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Timeline {
    /// The revision this snapshot is (`revision()` when it was read).
    pub revision: u64,
    /// Where the track's content ends (its last item's end), QN.
    pub end_qn: f64,
    /// Sorted by `start_qn`.
    pub notes: Vec<Note>,
    /// Sorted by `qn`.
    pub ctrls: Vec<Ctrl>,
    /// Sorted by `qn`; the first point is at QN 0 (empty: 120 bpm, 4/4).
    pub tempo: Vec<Tempo>,
}

impl Note {
    fn to_raw(self) -> fts_timeline_note {
        fts_timeline_note {
            start_qn: self.start_qn,
            end_qn: self.end_qn,
            channel: self.channel,
            key: self.key,
            velocity: self.velocity,
            release_velocity: self.release_velocity,
            port: 0,
            reserved: 0,
        }
    }

    fn from_raw(r: &fts_timeline_note) -> Self {
        Self {
            start_qn: r.start_qn,
            end_qn: r.end_qn,
            channel: r.channel & 0x0f,
            key: r.key & 0x7f,
            velocity: r.velocity.clamp(1, 127),
            release_velocity: r.release_velocity & 0x7f,
        }
    }
}

impl Ctrl {
    fn to_raw(self) -> fts_timeline_ctrl {
        let (kind, number, value) = match self.control {
            Control::Cc { controller, value } => (FTS_TIMELINE_CC, controller, u16::from(value)),
            Control::PitchBend(v) => (FTS_TIMELINE_PITCH_BEND, 0, v),
            Control::ChannelPressure(v) => (FTS_TIMELINE_CHANNEL_PRESSURE, 0, u16::from(v)),
            Control::PolyPressure { key, value } => {
                (FTS_TIMELINE_POLY_PRESSURE, key, u16::from(value))
            }
            Control::Program(p) => (FTS_TIMELINE_PROGRAM, 0, u16::from(p)),
        };
        fts_timeline_ctrl {
            qn: self.qn,
            kind,
            channel: self.channel,
            number,
            reserved: 0,
            value,
            port: 0,
        }
    }

    /// `None` for a kind this version doesn't know (a reader skips it).
    fn from_raw(r: &fts_timeline_ctrl) -> Option<Self> {
        let seven = (r.value.min(127)) as u8;
        let control = match r.kind {
            FTS_TIMELINE_CC => Control::Cc {
                controller: r.number & 0x7f,
                value: seven,
            },
            FTS_TIMELINE_PITCH_BEND => Control::PitchBend(r.value.min(16_383)),
            FTS_TIMELINE_CHANNEL_PRESSURE => Control::ChannelPressure(seven),
            FTS_TIMELINE_POLY_PRESSURE => Control::PolyPressure {
                key: r.number & 0x7f,
                value: seven,
            },
            FTS_TIMELINE_PROGRAM => Control::Program(seven),
            _ => return None,
        };
        Some(Self {
            qn: r.qn,
            channel: r.channel & 0x0f,
            control,
        })
    }
}

impl Tempo {
    fn to_raw(self) -> fts_timeline_tempo {
        fts_timeline_tempo {
            qn: self.qn,
            seconds: self.seconds,
            bpm: self.bpm,
            numerator: self.numerator,
            denominator: self.denominator,
            flags: match self.shape {
                TempoShape::Step => 0,
                TempoShape::Ramp => FTS_TIMELINE_TEMPO_RAMP,
            },
        }
    }

    fn from_raw(r: &fts_timeline_tempo) -> Self {
        Self {
            qn: r.qn,
            seconds: r.seconds,
            bpm: r.bpm,
            numerator: r.numerator,
            denominator: r.denominator,
            shape: if r.flags & FTS_TIMELINE_TEMPO_RAMP != 0 {
                TempoShape::Ramp
            } else {
                TempoShape::Step
            },
        }
    }
}

/// `T`'s bytes, as the C struct lays them out (native order).
fn put<T: Copy>(out: &mut Vec<u8>, value: &T) {
    // SAFETY: `T` is one of this module's `#[repr(C)]` plain-data structs
    // (no padding holes: their layouts are asserted in `sys`), read as
    // bytes for the length of the value.
    let bytes = unsafe {
        std::slice::from_raw_parts(std::ptr::from_ref(value).cast::<u8>(), size_of::<T>())
    };
    out.extend_from_slice(bytes);
}

/// A `T` from the first `size_of::<T>()` bytes of `bytes`; the rest of an
/// element a later version made longer is ignored. Zero-filled when the
/// element is shorter than `T` (an older writer).
fn take<T: Copy + Default>(bytes: &[u8]) -> T {
    let mut value = T::default();
    let n = bytes.len().min(size_of::<T>());
    // SAFETY: `T` is plain data (every bit pattern of its fields valid);
    // `n` bytes fit both the source and the value.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            std::ptr::from_mut(&mut value).cast::<u8>(),
            n,
        );
    }
    value
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A section of `count` elements of `size` bytes from `at`, advancing it.
fn section<'a>(
    bytes: &'a [u8],
    at: &mut usize,
    count: u32,
    size: u32,
) -> Result<impl Iterator<Item = &'a [u8]>, ExtError> {
    let size = size as usize;
    let len = (count as usize)
        .checked_mul(size)
        .ok_or_else(|| ExtError::Failed("timeline: section too long".into()))?;
    let end = at
        .checked_add(len)
        .filter(|e| *e <= bytes.len())
        .ok_or_else(|| ExtError::Failed("timeline: truncated".into()))?;
    let body = bytes.get(*at..end).unwrap_or_default();
    *at = end;
    Ok(body.chunks_exact(size.max(1)).take(count as usize))
}

impl Timeline {
    /// The stream `read` writes: `fts_timeline_header_t`, then the notes,
    /// controllers and tempo points.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let header = fts_timeline_header {
            magic: FTS_TIMELINE_MAGIC,
            header_size: count(size_of::<fts_timeline_header>()),
            revision: self.revision,
            note_count: count(self.notes.len()),
            ctrl_count: count(self.ctrls.len()),
            tempo_count: count(self.tempo.len()),
            note_size: count(size_of::<fts_timeline_note>()),
            ctrl_size: count(size_of::<fts_timeline_ctrl>()),
            tempo_size: count(size_of::<fts_timeline_tempo>()),
            end_qn: self.end_qn,
        };
        let mut out = Vec::with_capacity(
            size_of::<fts_timeline_header>()
                + self.notes.len() * size_of::<fts_timeline_note>()
                + self.ctrls.len() * size_of::<fts_timeline_ctrl>()
                + self.tempo.len() * size_of::<fts_timeline_tempo>(),
        );
        put(&mut out, &header);
        for n in &self.notes {
            put(&mut out, &n.to_raw());
        }
        for c in &self.ctrls {
            put(&mut out, &c.to_raw());
        }
        for t in &self.tempo {
            put(&mut out, &t.to_raw());
        }
        out
    }

    /// Read what [`encode`](Self::encode) (or a C host) wrote.
    ///
    /// # Errors
    ///
    /// When the stream isn't a timeline (wrong magic) or is cut short.
    pub fn decode(bytes: &[u8]) -> Result<Self, ExtError> {
        let header: fts_timeline_header = take(bytes);
        if bytes.len() < 8 || header.magic != FTS_TIMELINE_MAGIC {
            return Err(ExtError::Failed("timeline: not a timeline stream".into()));
        }
        let mut at = header.header_size as usize;
        if at < size_of::<fts_timeline_header>() || at > bytes.len() {
            return Err(ExtError::Failed("timeline: bad header".into()));
        }
        let notes = section(bytes, &mut at, header.note_count, header.note_size)?
            .map(|b| Note::from_raw(&take(b)))
            .collect();
        let ctrls = section(bytes, &mut at, header.ctrl_count, header.ctrl_size)?
            .filter_map(|b| Ctrl::from_raw(&take(b)))
            .collect();
        let tempo = section(bytes, &mut at, header.tempo_count, header.tempo_size)?
            .map(|b| Tempo::from_raw(&take(b)))
            .collect();
        Ok(Self {
            revision: header.revision,
            end_qn: header.end_qn,
            notes,
            ctrls,
            tempo,
        })
    }

    /// The channels that have notes, ascending: each is its own line (see
    /// the contract's "Channels").
    #[must_use]
    pub fn channels(&self) -> Vec<u8> {
        let mut used = [false; 16];
        for n in &self.notes {
            used[usize::from(n.channel & 0x0f)] = true;
        }
        (0u8..16).filter(|c| used[usize::from(*c)]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Timeline {
        Timeline {
            revision: 7,
            end_qn: 16.0,
            notes: vec![
                Note {
                    start_qn: 0.0,
                    end_qn: 2.1,
                    channel: 0,
                    key: 60,
                    velocity: 90,
                    release_velocity: 64,
                },
                Note {
                    start_qn: 2.0,
                    end_qn: 4.0,
                    channel: 3,
                    key: 62,
                    velocity: 30,
                    release_velocity: 12,
                },
            ],
            ctrls: vec![
                Ctrl {
                    qn: 0.0,
                    channel: 0,
                    control: Control::Cc {
                        controller: 1,
                        value: 96,
                    },
                },
                Ctrl {
                    qn: 1.5,
                    channel: 3,
                    control: Control::PitchBend(10_000),
                },
                Ctrl {
                    qn: 1.75,
                    channel: 3,
                    control: Control::PolyPressure { key: 62, value: 40 },
                },
                Ctrl {
                    qn: 2.0,
                    channel: 1,
                    control: Control::ChannelPressure(5),
                },
                Ctrl {
                    qn: 3.0,
                    channel: 1,
                    control: Control::Program(41),
                },
            ],
            tempo: vec![
                Tempo {
                    qn: 0.0,
                    seconds: 0.0,
                    bpm: 120.0,
                    numerator: 4,
                    denominator: 4,
                    shape: TempoShape::Step,
                },
                Tempo {
                    qn: 8.0,
                    seconds: 4.0,
                    bpm: 90.0,
                    numerator: 3,
                    denominator: 4,
                    shape: TempoShape::Ramp,
                },
            ],
        }
    }

    #[test]
    fn a_timeline_survives_its_stream() {
        let t = sample();
        let bytes = t.encode();
        assert_eq!(bytes.len(), 48 + 2 * 24 + 5 * 16 + 2 * 32);
        assert_eq!(Timeline::decode(&bytes).unwrap(), t);
        assert_eq!(t.channels(), vec![0, 3]);
    }

    #[test]
    fn a_cut_or_foreign_stream_is_refused() {
        let bytes = sample().encode();
        assert!(Timeline::decode(&bytes[..bytes.len() - 1]).is_err());
        assert!(Timeline::decode(b"not a timeline at all, no").is_err());
        assert!(Timeline::decode(&[]).is_err());
    }

    #[test]
    fn a_longer_element_from_a_later_version_is_stepped_over() {
        // A writer whose notes carry 8 more bytes each.
        let t = sample();
        let mut header: fts_timeline_header = take(&t.encode());
        header.note_size = 32;
        let mut out = Vec::new();
        put(&mut out, &header);
        for n in &t.notes {
            put(&mut out, &n.to_raw());
            out.extend_from_slice(&[0xAA; 8]);
        }
        for c in &t.ctrls {
            put(&mut out, &c.to_raw());
        }
        for p in &t.tempo {
            put(&mut out, &p.to_raw());
        }
        assert_eq!(Timeline::decode(&out).unwrap(), t);
    }
}

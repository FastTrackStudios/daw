//! Reading a Guitar Pro file through the dependency.
//!
//! Everything crate-specific lives here, so the rest of the importer
//! talks in [`crate::GpNote`] and knows nothing about `guitarpro`'s
//! model. That boundary is the whole reason the crate is pinned rather
//! than tracked: a 0.x model change should land in this file.

use crate::{BendPoint, GpNote};
use expression_editor_core::rows::StringTuning;
use guitarpro::model::legacy::song::Song;

/// Which format a file is, decided by extension.
///
/// The library needs telling; there is no sniffing entry point, and
/// guessing from magic bytes would still leave gp3 and gp4 ambiguous.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Gp3,
    Gp4,
    Gp5,
    /// GP6 `.gpx` and GP7/8 `.gp`, both wrapping GPIF.
    Gpif,
}

impl Format {
    pub fn of_path(path: &str) -> Option<Self> {
        let ext = path.rsplit('.').next()?.to_ascii_lowercase();
        match ext.as_str() {
            "gp3" => Some(Format::Gp3),
            "gp4" => Some(Format::Gp4),
            "gp5" => Some(Format::Gp5),
            "gpx" | "gp" => Some(Format::Gpif),
            _ => None,
        }
    }

    /// Whether bends from this format keep their shape.
    ///
    /// The GPIF path in the dependency reads only origin and
    /// destination, so a curve authored with a hold comes back as a
    /// straight line (#160).
    pub fn keeps_bend_shape(&self) -> bool {
        !matches!(self, Format::Gpif)
    }
}

/// Parse a file's bytes into a song.
pub fn read(bytes: &[u8], format: Format) -> Result<Song, String> {
    let mut song = Song::default();
    let result = match format {
        Format::Gp3 => song.read_gp3(bytes),
        Format::Gp4 => song.read_gp4(bytes),
        Format::Gp5 => song.read_gp5(bytes),
        Format::Gpif => song.read_gp(bytes),
    };
    result.map_err(|e| format!("{e:?}"))?;
    Ok(song)
}

/// The tuning of a track, as the file declares it.
///
/// `strings` is `(number, pitch)` with string 1 the *highest*, which is
/// the opposite of the editor's rows — row 0 is the lowest string,
/// because a string roll reads like tab with the low E at the bottom.
pub fn tuning_of(track: &guitarpro::model::legacy::track::Track) -> StringTuning {
    let mut pitches: Vec<i32> = track.strings.iter().map(|(_, p)| *p as i32).collect();
    pitches.reverse();
    StringTuning {
        name: "Guitar Pro",
        open_pitches: pitches,
        frets: track.fret_count.max(1),
        capo: track.offset.clamp(0, 24) as u8,
    }
}

/// A beat's length in ticks.
///
/// `Duration::time` is private in the dependency, so the arithmetic is
/// repeated here rather than reached for. It is the standard reading —
/// a whole note is four quarters, dots add half again — and the
/// library's own quarter is 960, the same as ours, so no scaling is
/// needed on top.
fn duration_ticks(d: &guitarpro::model::legacy::key_signature::Duration) -> f64 {
    let value = d.value.max(1) as f64;
    let mut ticks = crate::PPQ * 4.0 / value;
    if d.dotted {
        ticks += ticks / 2.0;
    }
    if d.double_dotted {
        ticks += ticks * 3.0 / 4.0;
    }
    // Tuplets divide the beat: three in the time of two, and so on.
    let enters = d.tuplet_enters.max(1) as f64;
    let times = d.tuplet_times.max(1) as f64;
    ticks * times / enters
}

/// Flatten a track's measures into notes on an absolute tick timeline.
///
/// Voices are merged: the editor's roll has one note per string per
/// moment, and a second voice on the same string at the same tick is a
/// notation convenience rather than two things to play.
pub fn notes_of(track: &guitarpro::model::legacy::track::Track, strings: usize) -> Vec<GpNote> {
    let mut out = Vec::new();

    for measure in &track.measures {
        for voice in &measure.voices {
            let mut tick = measure.start.max(0) as f64;
            for beat in &voice.beats {
                let length = duration_ticks(&beat.duration);
                let start = beat.start.map(|s| s as f64).unwrap_or(tick);

                for note in &beat.notes {
                    // GP numbers strings from 1, highest first; the roll
                    // puts the lowest at row 0.
                    let n = note.string.max(1) as usize;
                    let string = strings.saturating_sub(n);

                    let (bend, prebend) = match &note.effect.bend {
                        Some(b) => (
                            b.points
                                .iter()
                                .map(|p| BendPoint {
                                    position: p.position as f64,
                                    // The library pre-scales values to
                                    // quarter tones; #160 says convert
                                    // once at the boundary and keep
                                    // semitones inside.
                                    value: p.value as f64 * 25.0,
                                })
                                .collect(),
                            b.points.first().is_some_and(|p| p.position == 0 && p.value != 0),
                        ),
                        None => (Vec::new(), false),
                    };

                    out.push(GpNote {
                        string,
                        fret: note.value as i32,
                        start,
                        length,
                        bend,
                        prebend,
                    });
                }
                tick = start + length;
            }
        }
    }

    out.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then(a.string.cmp(&b.string))
    });
    out
}

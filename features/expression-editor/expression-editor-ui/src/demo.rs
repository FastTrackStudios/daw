//! Demo documents — the scenes the example app and the screenshot
//! harness both mount.
//!
//! Lives in the library, not in a test, so the runnable example and the
//! PNG harness show *the same thing*. A screenshot that drifted from
//! what the app actually launches would be worse than no screenshot.

use expression_editor_core::doc::{ExpressionDoc, Lane, Marker, Note, NoteId, TimeBase};
use expression_editor_core::{Editor, Viewport};

pub const PPQ: f64 = 960.0;

/// Which demo document to build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    /// A sung phrase: scoops into each note, vibrato that grows.
    Phrase,
    /// A long note divided into Q zones, each with its own effective
    /// pitch center.
    Zones,
    /// The same phrase read under Maqam Rast.
    Microtonal,
    /// Pressure lane active, showing the fixed two-semitone lane box.
    Pressure,
    /// All three MPE dimensions overlaid at once.
    AllLanes,
    /// Two sounding notes sharing a member channel — ownership is
    /// undecidable and the editor says so.
    Ambiguous,
    /// An empty document.
    Empty,
    /// A drum groove — triangle heads on named kit lanes.
    Drums,
    /// A guitar riff on a string roll: fret numbers, per-string colour,
    /// articulation badges and legato ties.
    Guitar,
    /// A sung line carrying lyric syllables.
    Lyrics,
    /// Orchestral: held notes with CC1 and CC11 riding behind them.
    Orchestral,
    /// Long held notes across several rows — material a razor can
    /// slice through.
    Held,
    /// A part whose note density changes across it — sixteenths at the
    /// start, held notes at the end. What contextual zoom is for.
    Density,
}

impl Scene {
    pub const ALL: [Scene; 13] = [
        Scene::Phrase,
        Scene::Zones,
        Scene::Microtonal,
        Scene::Pressure,
        Scene::AllLanes,
        Scene::Ambiguous,
        Scene::Empty,
        Scene::Density,
        Scene::Held,
        Scene::Orchestral,
        Scene::Drums,
        Scene::Guitar,
        Scene::Lyrics,
    ];

    /// Stable file-name stem for screenshots.
    pub fn slug(&self) -> &'static str {
        match self {
            Scene::Phrase => "01-phrase",
            Scene::Zones => "02-zones",
            Scene::Microtonal => "03-microtonal",
            Scene::Pressure => "04-pressure",
            Scene::AllLanes => "05-all-lanes",
            Scene::Ambiguous => "06-ambiguous",
            Scene::Empty => "07-empty",
            Scene::Density => "16-density",
            Scene::Held => "19-held",
            Scene::Orchestral => "21-orchestral",
            Scene::Drums => "25-drums",
            Scene::Guitar => "26-guitar",
            Scene::Lyrics => "27-lyrics",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Scene::Phrase => "Sung phrase",
            Scene::Zones => "Q zones",
            Scene::Microtonal => "Maqam Rast",
            Scene::Pressure => "Pressure lane",
            Scene::AllLanes => "All lanes",
            Scene::Ambiguous => "Channel conflict",
            Scene::Empty => "Empty",
            Scene::Density => "Mixed density",
            Scene::Held => "Held notes",
            Scene::Orchestral => "Orchestral CC",
            Scene::Drums => "Drum groove",
            Scene::Guitar => "Guitar riff",
            Scene::Lyrics => "Vocal lyrics",
        }
    }
}

/// A note whose pitch curve looks like something a person sang: a scoop
/// up into the target, then vibrato that opens up as the note is held.
fn sung_note(id: u64, start: f64, len: f64, row: i32, channel: u8, scoop: f64) -> Note {
    let mut n = Note::new(NoteId(id), start, start + len, row);
    n.channel = Some(channel);
    const STEPS: usize = 48;
    for k in 0..STEPS {
        let f = k as f64 / (STEPS - 1) as f64;
        let t = start + len * f;
        // Scoop decays fast; vibrato grows in after the attack.
        let approach = scoop * (1.0 - f).powi(4);
        let vibrato = 0.22 * (f * 22.0).sin() * (f * 1.6).min(1.0);
        // A little downward drift, the thing the Drift slider exists to
        // take back out.
        let drift = -0.18 * f;
        n.pitch.set(t, approach + vibrato + drift);
        n.pressure.set(t, (0.35 + 0.6 * (f * 3.1).sin().abs()).clamp(0.0, 1.0));
        n.timbre.set(t, (0.2 + 0.7 * f).clamp(0.0, 1.0));
    }
    n
}

/// Build the editor for a scene, sized to `viewport`.
pub fn editor(scene: Scene, viewport: Viewport) -> Editor {
    let end = match scene {
        Scene::Density => PPQ * 40.0,
        Scene::Drums => PPQ * 8.0,
        _ => PPQ * 8.0,
    };
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, end);

    match scene {
        Scene::Empty => {}
        Scene::Density => {
            // A sixteenth-note run, then held notes: one fixed zoom
            // cannot serve both, which is the whole argument for
            // density-aware zoom.
            let mut id = 1u64;
            for i in 0..32 {
                doc.push(sung_note(
                    id,
                    PPQ * 0.25 * i as f64,
                    PPQ * 0.2,
                    60 + (i % 7) as i32,
                    2 + (i % 14) as u8,
                    -0.5,
                ));
                id += 1;
            }
            for i in 0..5 {
                doc.push(sung_note(
                    id,
                    PPQ * (16.0 + 4.0 * i as f64),
                    PPQ * 3.5,
                    62 + (i * 2) as i32,
                    2 + (i % 14) as u8,
                    -1.5,
                ));
                id += 1;
            }
        }
        Scene::Zones => {
            // One long note that moves through three pitch centers —
            // the case zone scaling exists for.
            let mut n = Note::new(NoteId(1), PPQ * 0.5, PPQ * 6.5, 62);
            n.channel = Some(2);
            const STEPS: usize = 120;
            for k in 0..STEPS {
                let f = k as f64 / (STEPS - 1) as f64;
                let t = n.start + (n.end - n.start) * f;
                // Three plateaus with slides between them.
                let plateau = if f < 0.3 {
                    0.0
                } else if f < 0.4 {
                    (f - 0.3) / 0.1 * 3.0
                } else if f < 0.65 {
                    3.0
                } else if f < 0.75 {
                    3.0 - (f - 0.65) / 0.1 * 5.0
                } else {
                    -2.0
                };
                let vib = 0.18 * (f * 40.0).sin();
                n.pitch.set(t, plateau + vib);
                n.pressure.set(t, 0.5 + 0.4 * (f * 6.0).sin());
                n.timbre.set(t, 0.5);
            }
            n.add_split(PPQ * 2.6);
            n.add_split(PPQ * 4.9);
            n.target = expression_editor_core::Target::Zone(1);
            doc.push(n);
        }
        Scene::Drums => {
            use expression_editor_core::{DrumMap, RowSpace};
            let map = DrumMap::general_midi();
            let row = |name: &str| {
                map.lanes.iter().position(|l| l.name == name).unwrap_or(0) as i32
            };
            let mut id = 1u64;
            let mut hit = |doc: &mut ExpressionDoc, r: i32, beat: f64, vel: f64| {
                let t = PPQ * beat;
                let mut n = Note::new(NoteId(id), t, t + PPQ * 0.1, r);
                n.velocity = vel;
                doc.push(n);
                id += 1;
            };
            // Two bars of a plain backbeat, with ghost notes so the
            // velocity strip has something to show.
            for bar in 0..2 {
                let b = bar as f64 * 4.0;
                for eighth in 0..8 {
                    let t = b + eighth as f64 * 0.5;
                    hit(&mut doc, row("HH Closed"), t, if eighth % 2 == 0 { 0.8 } else { 0.5 });
                }
                hit(&mut doc, row("Kick"), b, 0.95);
                hit(&mut doc, row("Kick"), b + 2.5, 0.9);
                hit(&mut doc, row("Snare"), b + 1.0, 0.92);
                hit(&mut doc, row("Snare"), b + 1.75, 0.35);
                hit(&mut doc, row("Snare"), b + 3.0, 0.92);
                if bar == 1 {
                    hit(&mut doc, row("Crash"), b, 0.85);
                    hit(&mut doc, row("Tom Low"), b + 3.5, 0.7);
                    hit(&mut doc, row("Tom High"), b + 3.75, 0.75);
                }
            }
            doc.row_space = RowSpace::Drums(map);
        }
        Scene::Guitar => {
            use expression_editor_core::{Articulation, RowSpace, StringTuning};
            let tuning = StringTuning::guitar_standard();
            // A riff on the low strings: fret numbers, techniques, and
            // a hammer-on/pull-off pair.
            let riff: [(usize, u8, f64, f64, Option<Articulation>); 9] = [
                (0, 0, 0.0, 0.45, Some(Articulation::PalmMute)),
                (0, 0, 0.5, 0.45, Some(Articulation::PalmMute)),
                (0, 3, 1.0, 0.5, Some(Articulation::HammerOn)),
                (0, 5, 1.5, 0.5, None),
                (1, 5, 2.0, 0.9, None),
                (2, 7, 3.0, 0.9, Some(Articulation::Vibrato)),
                (2, 5, 4.0, 0.5, Some(Articulation::PullOff)),
                (2, 3, 4.5, 0.5, None),
                (5, 12, 5.0, 1.6, Some(Articulation::NaturalHarmonic)),
            ];
            for (i, &(string, fret, start, len, art)) in riff.iter().enumerate() {
                let mut n = Note::new(
                    NoteId(i as u64 + 1),
                    PPQ * start,
                    PPQ * (start + len),
                    string as i32,
                );
                n.fret = Some(fret);
                n.velocity = 0.8;
                n.articulation = art;
                n.legato = art.is_some_and(|a| a.is_legato());
                doc.push(n);
            }
            doc.row_space = RowSpace::Strings(tuning);
        }
        Scene::Lyrics => {
            let words = [
                ("A", 62, 0.0, 0.75),
                ("ma", 64, 0.75, 0.75),
                ("zing", 67, 1.5, 1.5),
                ("grace", 64, 3.0, 1.5),
                ("how", 62, 4.5, 0.75),
                ("sweet", 59, 5.25, 0.75),
                ("the", 60, 6.0, 0.5),
                ("sound", 62, 6.5, 1.5),
            ];
            for (i, &(text, row, start, len)) in words.iter().enumerate() {
                let mut n = sung_note(
                    i as u64 + 1,
                    PPQ * start,
                    PPQ * len,
                    row,
                    2 + (i % 14) as u8,
                    -0.8,
                );
                n.text = Some(text.to_string());
                doc.push(n);
            }
        }
        Scene::Orchestral => {
            for (i, row) in [55, 62, 67, 71].iter().enumerate() {
                doc.push(sung_note(
                    i as u64 + 1,
                    PPQ * 0.5,
                    PPQ * 7.0,
                    *row,
                    2 + i as u8,
                    -0.8,
                ));
            }
            doc.cc = expression_editor_core::CcSet::orchestral();
            // A swell and release on expression, with modulation
            // arriving late — the shape an orchestral phrase actually
            // rides.
            for k in 0..=48 {
                let f = k as f64 / 48.0;
                let t = PPQ * 0.5 + PPQ * 7.0 * f;
                let swell = (f * core::f64::consts::PI).sin();
                if let Some(l) = doc.cc.get_mut(11) {
                    l.curve.set(t, 0.25 + 0.7 * swell);
                }
                if let Some(l) = doc.cc.get_mut(1) {
                    l.curve.set(t, (f * 1.6 - 0.35).clamp(0.0, 1.0) * 0.85);
                }
            }
        }
        Scene::Held => {
            for (i, row) in [60, 62, 64, 65, 67].iter().enumerate() {
                doc.push(sung_note(
                    i as u64 + 1,
                    PPQ * 0.5,
                    PPQ * 7.0,
                    *row,
                    2 + i as u8,
                    -1.0,
                ));
            }
        }
        Scene::Ambiguous => {
            // Deliberately both on channel 2 while sounding together.
            doc.push(sung_note(1, PPQ * 0.5, PPQ * 3.0, 60, 2, -2.0));
            doc.push(sung_note(2, PPQ * 2.0, PPQ * 3.0, 67, 2, 1.5));
            doc.push(sung_note(3, PPQ * 5.5, PPQ * 2.0, 64, 3, -1.0));
        }
        _ => {
            let phrase: [(f64, f64, i32, f64); 5] = [
                (0.4, 1.1, 60, -2.4),
                (1.6, 0.9, 64, -1.2),
                (2.7, 1.4, 67, 1.8),
                (4.3, 1.0, 65, -1.6),
                (5.5, 2.0, 62, -0.9),
            ];
            for (i, &(start, len, row, scoop)) in phrase.iter().enumerate() {
                doc.push(sung_note(
                    i as u64 + 1,
                    PPQ * start,
                    PPQ * len,
                    row,
                    2 + i as u8,
                    scoop,
                ));
            }
        }
    }
    // Section markers the host would normally supply.
    if scene != Scene::Empty {
        doc.markers = vec![
            Marker { t: 0.0, label: Some("Verse".into()) },
            Marker { t: PPQ * 4.0, label: Some("Chorus".into()) },
        ];
    }
    doc.mark_ambiguity();

    let mut ed = Editor::new(doc, viewport);
    // The editor renders in the document's row space; keeping two
    // copies in sync by hand is how a drum roll ends up drawing piano
    // keys.
    ed.row_space = ed.doc.row_space.clone();
    ed.reset_view();
    ed.playhead = (scene != Scene::Empty).then_some(PPQ * 2.35);
    match scene {
        Scene::Zones => {
            ed.selection.set_single(NoteId(1));
        }
        Scene::Microtonal => {
            ed.tuning.temperament = expression_editor_core::tuning::RAST.clone();
            ed.tuning.key_pc = 2; // D
            ed.selection.set_single(NoteId(3));
        }
        Scene::Pressure => {
            ed.lane = Lane::Pressure;
            ed.selection.set_single(NoteId(3));
        }
        Scene::AllLanes => {
            ed.overlays = vec![Lane::Pitch, Lane::Pressure, Lane::Timbre];
            ed.selection.set_single(NoteId(3));
        }
        Scene::Ambiguous => {
            ed.selection.set_single(NoteId(1));
        }
        Scene::Empty | Scene::Density | Scene::Held => {}
        Scene::Orchestral => {
            ed.set_mode(expression_editor_core::Mode::Mpe);
            ed.selection.set_single(NoteId(2));
        }
        Scene::Drums => ed.set_mode(expression_editor_core::Mode::Drums),
        Scene::Guitar => {
            ed.set_mode(expression_editor_core::Mode::Guitar);
            ed.selection.set_single(NoteId(6));
        }
        Scene::Lyrics => {
            ed.set_mode(expression_editor_core::Mode::Vocals);
            ed.selection.set_single(NoteId(3));
        }
        Scene::Phrase => {
            ed.selection.set_single(NoteId(3));
        }
    }
    ed
}

/// Default demo canvas size — roughly a plugin editor window.
pub fn default_viewport() -> Viewport {
    Viewport::new(1100.0, 560.0)
}

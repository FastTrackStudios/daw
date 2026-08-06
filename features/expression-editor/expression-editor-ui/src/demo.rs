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
}

impl Scene {
    pub const ALL: [Scene; 7] = [
        Scene::Phrase,
        Scene::Zones,
        Scene::Microtonal,
        Scene::Pressure,
        Scene::AllLanes,
        Scene::Ambiguous,
        Scene::Empty,
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
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);

    match scene {
        Scene::Empty => {}
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
        Scene::Empty => {}
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

//! Flams and two-handed drum rows.

use expression_editor_core::doc::{ExpressionDoc, Note, NoteId, TimeBase};
use expression_editor_core::flam::{DEFAULT_FLAM_MS, FlamError, FlamSide, FlamStep, flam, next_step};
use expression_editor_core::rows::{DrumMap, Hand, RowSpace};
use expression_editor_core::{Editor, Mode, Viewport};

const PPQ: f64 = 960.0;
const BPM: f64 = 120.0;

fn map() -> DrumMap {
    DrumMap::fts()
}

fn row_of(map: &DrumMap, name: &str) -> usize {
    map.lanes.iter().position(|l| l.name == name).unwrap()
}

fn doc_with_hit(row: usize) -> ExpressionDoc {
    let mut d = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 4.0);
    d.row_space = RowSpace::Drums(map());
    let mut n = Note::new(NoteId(1), PPQ, PPQ + PPQ / 8.0, row as i32);
    n.velocity = 1.0;
    d.push(n);
    d
}

// ── The map ──────────────────────────────────────────────────────────

#[test]
fn kick_snare_and_every_tom_are_two_handed() {
    let m = map();
    for name in ["K", "S", "T1", "T2", "T3", "T4"] {
        let r = row_of(&m, name);
        assert!(m.is_two_handed(r), "{name} should have both hands");
        assert!(m.other_hand_row(r).is_some(), "{name} has no counterpart");
    }
}

#[test]
fn cymbals_and_hats_are_not() {
    // Nothing about a hi-hat depends on which stick got there.
    let m = map();
    for name in ["H-Clsd Tip", "C-L", "R-Bell", "Stack"] {
        let r = row_of(&m, name);
        assert!(!m.is_two_handed(r), "{name} should be a single row");
    }
}

#[test]
fn the_pairing_is_symmetric() {
    let m = map();
    for r in 0..m.lanes.len() {
        if let Some(other) = m.other_hand_row(r) {
            assert_eq!(
                m.other_hand_row(other),
                Some(r),
                "{} points at {} but not back",
                m.lanes[r].name,
                m.lanes[other].name
            );
        }
    }
}

#[test]
fn the_unmarked_half_of_a_pair_is_the_right_hand() {
    // The map leaves the lead hand unlabelled — `K`, `S`, `T1`.
    let m = map();
    assert_eq!(m.hand_of(row_of(&m, "K")), Some(Hand::Right));
    assert_eq!(m.hand_of(row_of(&m, "K L")), Some(Hand::Left));
    assert_eq!(m.hand_of(row_of(&m, "S")), Some(Hand::Right));
    assert_eq!(m.hand_of(row_of(&m, "S R")), Some(Hand::Right));
}

#[test]
fn a_piece_is_one_row_until_it_is_split() {
    let m = map();
    let visible = m.visible_rows(&[]);
    // Both halves of a pair are never both visible while collapsed.
    let k = row_of(&m, "K");
    let kl = row_of(&m, "K L");
    assert!(visible.contains(&k));
    assert!(!visible.contains(&kl), "the left kick showed unasked");

    let split = m.visible_rows(&[k, kl]);
    assert!(split.contains(&k) && split.contains(&kl));
    assert!(split.len() > visible.len());
}

// ── The flam ─────────────────────────────────────────────────────────

#[test]
fn a_flam_puts_the_grace_note_on_the_other_hand() {
    // A flam played with one hand twice is a drag; the roll has to show
    // the difference.
    let m = map();
    let snare = row_of(&m, "S");
    let doc = doc_with_hit(snare);
    let edit = flam(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, BPM).unwrap();

    let expression_editor_core::Edit::AddNote(grace) = edit else {
        panic!("expected an added note");
    };
    assert_eq!(grace.row as usize, m.other_hand_row(snare).unwrap());
}

#[test]
fn flam_before_lands_early_and_flam_after_lands_late() {
    let m = map();
    let doc = doc_with_hit(row_of(&m, "S"));
    // First press: before.
    let expression_editor_core::Edit::AddNote(g) =
        flam(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, BPM).unwrap()
    else {
        unreachable!()
    };
    assert!(g.start < PPQ, "the first press goes before the hit");

    // With that grace note present, the next press moves it across.
    let mut with_grace = doc.clone();
    let before_start = g.start;
    with_grace.push(*g);
    let expression_editor_core::Edit::MoveTime { delta, .. } =
        flam(&with_grace, &m, NoteId(1), DEFAULT_FLAM_MS, BPM).unwrap()
    else {
        panic!("expected the grace note to move");
    };
    assert!(
        before_start + delta > PPQ,
        "moving it should land it after the hit"
    );
}

#[test]
fn the_offset_is_the_measured_one() {
    // #176 swept real flams: 75% sit at 25-35 ms, nothing below 15.
    // 30 ms at 120bpm is 0.03 * 2 quarters/sec * 960 = 57.6 ticks.
    let m = map();
    let doc = doc_with_hit(row_of(&m, "S"));
    let expression_editor_core::Edit::AddNote(g) =
        flam(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, BPM).unwrap()
    else {
        unreachable!()
    };
    let offset = PPQ - g.start;
    assert!(
        (offset - 57.6).abs() < 0.5,
        "30ms at 120bpm should be ~57.6 ticks, got {offset}"
    );
}

#[test]
fn the_offset_is_wall_clock_not_musical() {
    // A flam is two sticks and one wrist; it does not scale with tempo
    // the way a subdivision does.
    let m = map();
    let doc = doc_with_hit(row_of(&m, "S"));
    let ticks_at = |bpm| {
        let expression_editor_core::Edit::AddNote(g) =
            flam(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, bpm).unwrap()
        else {
            unreachable!()
        };
        PPQ - g.start
    };
    assert!(
        ticks_at(60.0) < ticks_at(240.0),
        "at a faster tempo the same milliseconds are more ticks"
    );
}

#[test]
fn the_grace_note_is_quieter() {
    // Which is what makes a flam read as one gesture rather than two
    // hits — but not silent, or it disappears.
    let m = map();
    let doc = doc_with_hit(row_of(&m, "S"));
    let expression_editor_core::Edit::AddNote(g) =
        flam(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, BPM).unwrap()
    else {
        unreachable!()
    };
    assert!(g.velocity < 1.0 && g.velocity > 0.2, "got {}", g.velocity);
}

#[test]
fn a_single_handed_piece_cannot_flam() {
    let m = map();
    let doc = doc_with_hit(row_of(&m, "H-Clsd Tip"));
    assert_eq!(
        flam(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, BPM).err(),
        Some(FlamError::NotTwoHanded)
    );
}

#[test]
fn the_cycle_is_none_then_before_then_after_then_none() {
    // The third press removing the flam is what makes it a cycle rather
    // than a trap: changing your mind should not mean reaching for undo.
    let m = map();
    let mut doc = doc_with_hit(row_of(&m, "S"));

    // 1: nothing there yet.
    assert_eq!(
        next_step(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, BPM),
        Ok(FlamStep::Add(FlamSide::Before))
    );
    let expression_editor_core::Edit::AddNote(g) =
        flam(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, BPM).unwrap()
    else {
        unreachable!()
    };
    let grace_id = g.id;
    doc.push(*g);

    // 2: it is before, so move it across.
    match next_step(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, BPM).unwrap() {
        FlamStep::Move { grace, delta } => {
            assert_eq!(grace, grace_id);
            if let Some(n) = doc.note_mut(grace_id) {
                let len = n.end - n.start;
                n.start += delta;
                n.end = n.start + len;
            }
        }
        other => panic!("expected a move, got {other:?}"),
    }
    assert!(doc.note(grace_id).unwrap().start > PPQ, "now after the hit");

    // 3: it is after, so the cycle closes.
    assert_eq!(
        next_step(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, BPM),
        Ok(FlamStep::Remove(grace_id))
    );

    // 4: and having removed it, we are back at the start.
    doc.notes.retain(|n| n.id != grace_id);
    assert_eq!(
        next_step(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, BPM),
        Ok(FlamStep::Add(FlamSide::Before)),
        "the cycle came round"
    );
}

#[test]
fn a_grace_note_nudged_by_hand_is_still_that_hits_flam() {
    // Otherwise the next press adds a second one, silently, and the
    // part has two grace notes where the player wanted one.
    let m = map();
    let mut doc = doc_with_hit(row_of(&m, "S"));
    let expression_editor_core::Edit::AddNote(g) =
        flam(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, BPM).unwrap()
    else {
        unreachable!()
    };
    let id = g.id;
    let mut g = *g;
    g.start -= 12.0; // dragged a little earlier
    g.end -= 12.0;
    doc.push(g);

    match next_step(&doc, &m, NoteId(1), DEFAULT_FLAM_MS, BPM).unwrap() {
        FlamStep::Move { grace, .. } => assert_eq!(grace, id),
        other => panic!("expected it to be recognised, got {other:?}"),
    }
}

// ── The key ──────────────────────────────────────────────────────────

fn drum_editor() -> Editor {
    let m = map();
    let snare = row_of(&m, "S");
    let mut ed = Editor::new(doc_with_hit(snare), Viewport::new(900.0, 500.0));
    ed.set_mode(Mode::Drums);
    ed.row_space = RowSpace::Drums(m);
    ed.selection.set_single(NoteId(1));
    ed
}

#[test]
fn pressing_the_key_walks_the_hit_through_the_cycle() {
    // Press, look, press again — and a third press if you did not want
    // it after all.
    let mut ed = drum_editor();
    let hit = NoteId(1);

    assert_eq!(ed.flam_selection(), 1);
    let grace = ed.doc.notes.iter().find(|n| n.id != hit).unwrap();
    assert!(grace.start < PPQ, "first press: before the hit");

    ed.selection.set_single(hit);
    assert_eq!(ed.flam_selection(), 1);
    let grace = ed.doc.notes.iter().find(|n| n.id != hit).unwrap();
    assert!(grace.start > PPQ, "second press: after it");

    ed.selection.set_single(hit);
    assert_eq!(ed.flam_selection(), 1);
    assert_eq!(
        ed.doc.notes.len(),
        1,
        "third press: the flam is gone and only the hit remains"
    );

    ed.selection.set_single(hit);
    assert_eq!(ed.flam_selection(), 1);
    let grace = ed.doc.notes.iter().find(|n| n.id != hit).unwrap();
    assert!(grace.start < PPQ, "and round again");
}

#[test]
fn the_next_step_is_readable_before_the_key_is_pressed() {
    // So a UI can say what the key will do rather than the user finding
    // out by pressing it.
    let mut ed = drum_editor();
    assert_eq!(
        ed.flam_step(NoteId(1)),
        Some(FlamStep::Add(FlamSide::Before))
    );
    ed.flam_selection();
    assert!(matches!(
        ed.flam_step(NoteId(1)),
        Some(FlamStep::Move { .. })
    ));
}

#[test]
fn flamming_opens_the_piece_so_the_grace_note_is_visible() {
    // A grace note on a hidden row is one you can neither see nor
    // select.
    let mut ed = drum_editor();
    assert!(ed.split_pieces.is_empty());
    ed.flam_selection();
    assert!(
        ed.split_pieces.len() >= 2,
        "both hands of the piece should be showing"
    );
}

#[test]
fn a_piece_can_be_split_and_collapsed_by_hand() {
    let mut ed = drum_editor();
    let m = map();
    let k = row_of(&m, "K");
    ed.toggle_piece_split(k);
    assert!(ed.split_pieces.contains(&k));
    ed.toggle_piece_split(k);
    assert!(!ed.split_pieces.contains(&k));
}

#[test]
fn the_key_does_nothing_outside_drum_mode() {
    let mut ed = drum_editor();
    ed.set_mode(Mode::Midi);
    ed.row_space = RowSpace::Pitch;
    assert_eq!(ed.flam_selection(), 0);
}

// ── Family banding ───────────────────────────────────────────────────
//
// The FTS map is 39 rows. Evenly-striped lanes give the eye nothing to
// steer by, so the roll bands by part of the kit.

use expression_editor_core::rows::{DrumFamily, drum_family};

#[test]
fn the_fts_abbreviations_land_in_the_right_families() {
    // Matched exactly on the head, not by substring: a single-letter
    // `contains` would put half the kit in the wrong band.
    assert_eq!(drum_family("K"), DrumFamily::Kick);
    assert_eq!(drum_family("K L"), DrumFamily::Kick);
    assert_eq!(drum_family("S"), DrumFamily::Snare);
    assert_eq!(drum_family("S-Cross"), DrumFamily::Snare);
    assert_eq!(drum_family("T3 L"), DrumFamily::Tom);
    assert_eq!(drum_family("H-Clsd Tip"), DrumFamily::HiHat);
    assert_eq!(drum_family("C-L Choke"), DrumFamily::Cymbal);
    assert_eq!(drum_family("R-Bell"), DrumFamily::Ride);
    assert_eq!(drum_family("Stack"), DrumFamily::Other);
}

#[test]
fn general_midi_names_still_work() {
    assert_eq!(drum_family("Kick"), DrumFamily::Kick);
    assert_eq!(drum_family("HH Closed"), DrumFamily::HiHat);
    assert_eq!(drum_family("Tom Floor L"), DrumFamily::Tom);
    assert_eq!(drum_family("Crash"), DrumFamily::Cymbal);
    assert_eq!(drum_family("Ride Bell"), DrumFamily::Ride);
}

#[test]
fn ride_is_its_own_band_not_a_cymbal() {
    // It is played like a hat — a continuous part — not struck like a
    // crash, so it reads better on its own.
    assert_ne!(drum_family("R-Bow Lo"), DrumFamily::Cymbal);
    assert_ne!(drum_family("Ride"), DrumFamily::Cymbal);
}

#[test]
fn every_family_has_a_distinct_band() {
    let all = [
        DrumFamily::Kick,
        DrumFamily::Snare,
        DrumFamily::Tom,
        DrumFamily::HiHat,
        DrumFamily::Cymbal,
        DrumFamily::Ride,
        DrumFamily::Other,
    ];
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!(a.band(), b.band(), "{a:?} and {b:?} look the same");
        }
    }
}

#[test]
fn a_divider_falls_once_per_group_not_once_per_row() {
    // The line that makes a band read as a band.
    let m = map();
    let starts: Vec<usize> = (0..m.lanes.len()).filter(|&r| m.starts_family(r)).collect();
    assert!(
        starts.len() < m.lanes.len() / 3,
        "{} dividers for {} rows is per-row, not per-group",
        starts.len(),
        m.lanes.len()
    );
    assert!(m.starts_family(0), "the first row opens its family");
}

#[test]
fn only_drum_rolls_are_banded() {
    // A piano roll has black and white keys to steer by; a string roll
    // has six rows. Neither needs it.
    use expression_editor_core::rows::{RowSpace, StringTuning};
    assert!(RowSpace::Pitch.row_background(60).is_none());
    assert!(
        RowSpace::Strings(StringTuning::guitar_standard())
            .row_background(2)
            .is_none()
    );
    assert!(RowSpace::Drums(map()).row_background(0).is_some());
}

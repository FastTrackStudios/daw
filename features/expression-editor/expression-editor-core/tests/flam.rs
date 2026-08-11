//! Flams and two-handed drum rows.

use expression_editor_core::doc::{ExpressionDoc, Note, NoteId, TimeBase};
use expression_editor_core::flam::{DEFAULT_FLAM_MS, FlamError, FlamSide, flam};
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
    let edit = flam(&doc, &m, NoteId(1), FlamSide::Before, DEFAULT_FLAM_MS, BPM).unwrap();

    let expression_editor_core::Edit::AddNote(grace) = edit else {
        panic!("expected an added note");
    };
    assert_eq!(grace.row as usize, m.other_hand_row(snare).unwrap());
}

#[test]
fn flam_before_lands_early_and_flam_after_lands_late() {
    let m = map();
    let doc = doc_with_hit(row_of(&m, "S"));
    let at = |side| {
        let expression_editor_core::Edit::AddNote(g) =
            flam(&doc, &m, NoteId(1), side, DEFAULT_FLAM_MS, BPM).unwrap()
        else {
            unreachable!()
        };
        g.start
    };
    assert!(at(FlamSide::Before) < PPQ);
    assert!(at(FlamSide::After) > PPQ);
}

#[test]
fn the_offset_is_the_measured_one() {
    // #176 swept real flams: 75% sit at 25-35 ms, nothing below 15.
    // 30 ms at 120bpm is 0.03 * 2 quarters/sec * 960 = 57.6 ticks.
    let m = map();
    let doc = doc_with_hit(row_of(&m, "S"));
    let expression_editor_core::Edit::AddNote(g) =
        flam(&doc, &m, NoteId(1), FlamSide::Before, DEFAULT_FLAM_MS, BPM).unwrap()
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
            flam(&doc, &m, NoteId(1), FlamSide::Before, DEFAULT_FLAM_MS, bpm).unwrap()
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
        flam(&doc, &m, NoteId(1), FlamSide::Before, DEFAULT_FLAM_MS, BPM).unwrap()
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
        flam(&doc, &m, NoteId(1), FlamSide::Before, DEFAULT_FLAM_MS, BPM),
        Err(FlamError::NotTwoHanded)
    );
}

#[test]
fn flamming_twice_is_refused_rather_than_stacked() {
    // Two notes on one row at one tick is not a flam, it is a bug you
    // find later by ear.
    let m = map();
    let snare = row_of(&m, "S");
    let mut doc = doc_with_hit(snare);
    let expression_editor_core::Edit::AddNote(g) =
        flam(&doc, &m, NoteId(1), FlamSide::Before, DEFAULT_FLAM_MS, BPM).unwrap()
    else {
        unreachable!()
    };
    doc.push(*g);

    assert_eq!(
        flam(&doc, &m, NoteId(1), FlamSide::Before, DEFAULT_FLAM_MS, BPM),
        Err(FlamError::AlreadyFlammed)
    );
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
fn pressing_the_key_flams_and_pressing_it_again_flips_the_side() {
    // One key, because you press it, look, and press again if you
    // wanted the grace note on the other side.
    let mut ed = drum_editor();
    assert_eq!(ed.flam_side, FlamSide::Before);

    assert_eq!(ed.flam_selection(), 1);
    assert_eq!(ed.flam_side, FlamSide::After, "the next one flips");
    let first = ed.doc.notes.iter().find(|n| n.id != NoteId(1)).unwrap().start;
    assert!(first < PPQ, "the first went before");

    ed.selection.set_single(NoteId(1));
    assert_eq!(ed.flam_selection(), 1);
    assert_eq!(ed.flam_side, FlamSide::Before, "and flips back");
    let late = ed.doc.notes.iter().any(|n| n.start > PPQ);
    assert!(late, "the second went after");
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

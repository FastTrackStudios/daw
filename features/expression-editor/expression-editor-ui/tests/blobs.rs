//! Sung notes draw as amplitude blobs, not rectangles.

use expression_editor_core::doc::{ExpressionDoc, Note, NoteId, TimeBase};
use expression_editor_core::{Editor, Mode, Viewport};
use expression_editor_ui::canvas;
use expression_editor_ui::theme;

const PPQ: f64 = 960.0;

/// One note whose contour and amplitude both vary, so a body that
/// ignored either would be visibly wrong.
fn editor(mode: Mode, offset: f64) -> Editor {
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    let mut n = Note::new(NoteId(1), 0.0, PPQ * 4.0, 60);
    n.weight = 0.8;
    for k in 0..64 {
        let f = k as f64 / 63.0;
        let t = n.start + (n.end - n.start) * f;
        n.pitch.set(t, offset + 0.6 * (f * core::f64::consts::TAU * 4.0).sin());
        // Swells in, fades out.
        n.pressure.set(t, (f * core::f64::consts::PI).sin());
    }
    doc.push(n);
    let mut ed = Editor::new(doc, Viewport::new(900.0, 500.0));
    ed.set_mode(mode);
    ed
}

fn points(s: &str) -> Vec<(f64, f64)> {
    s.split_whitespace()
        .filter_map(|p| {
            let (a, b) = p.split_once(',')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .collect()
}

#[test]
fn audio_notes_draw_as_blobs_and_midi_notes_do_not() {
    let audio = canvas::note_rects(&editor(Mode::Audio, 0.0));
    assert!(audio[0].blob.is_some(), "a sung note is not a rectangle");

    for mode in [Mode::Midi, Mode::Mpe, Mode::Drums, Mode::Guitar] {
        let rects = canvas::note_rects(&editor(mode, 0.0));
        assert!(
            rects[0].blob.is_none(),
            "{mode:?} has no envelope to draw a body from"
        );
    }
    // Vocals is the manual's own product, so it draws them too.
    assert!(canvas::note_rects(&editor(Mode::Vocals, 0.0))[0]
        .blob
        .is_some());
}

#[test]
fn the_body_thickness_follows_the_amplitude() {
    let rects = canvas::note_rects(&editor(Mode::Audio, 0.0));
    let pts = points(rects[0].blob.as_ref().unwrap());
    // The polygon is the top edge then the bottom edge reversed, so the
    // pair straddling each x is `i` and `len - 1 - i`.
    let n = pts.len();
    assert!(n >= 8 && n.is_multiple_of(2));
    let thickness = |i: usize| (pts[n - 1 - i].1 - pts[i].1).abs();

    // Pressure is a sine swell: thin at the ends, fattest in the middle.
    let mid = thickness(n / 4);
    let start = thickness(0);
    let end = thickness(n / 2 - 1);
    assert!(
        mid > start * 1.5 && mid > end * 1.5,
        "the swell is visible in the body: {start} .. {mid} .. {end}"
    );
}

#[test]
fn the_body_is_flat_and_the_pitch_track_moves_against_it() {
    let ed = editor(Mode::Audio, 0.0);
    let rects = canvas::note_rects(&ed);
    let pts = points(rects[0].blob.as_ref().unwrap());
    let n = pts.len();
    // Centre line of the top/bottom pair, per sample.
    let centers: Vec<f64> = (0..n / 2).map(|i| (pts[i].1 + pts[n - 1 - i].1) * 0.5).collect();

    let lo = centers.iter().cloned().fold(f64::MAX, f64::min);
    let hi = centers.iter().cloned().fold(f64::MIN, f64::max);
    assert!(
        hi - lo < 0.5,
        "the body sits flat, the way a MIDI note sits on its row — the \
         pitch track is what moves, and the gap between them is the \
         thing being edited. Wandered {}px",
        hi - lo
    );
}

#[test]
fn the_body_sits_at_the_notes_own_pitch_not_its_row() {
    // A note sung a semitone and a half sharp draws where it sounds, so
    // the row it is nearest is still readable underneath it.
    let base = editor(Mode::Audio, 0.0);
    let mut sharp_ed = editor(Mode::Audio, 1.5);
    // `Editor::new` frames its own content, so the two would otherwise
    // be measured through different cameras and the comparison would be
    // about zoom rather than pitch.
    sharp_ed.camera = base.camera;

    let flat = canvas::note_rects(&base)[0].blob_center.unwrap();
    let sharp = canvas::note_rects(&sharp_ed)[0].blob_center.unwrap();
    let expected = 1.5 * base.camera.px_per_semitone;
    // Up the screen is a smaller y.
    let moved = flat - sharp;
    assert!(
        (moved - expected).abs() < expected * 0.15,
        "moved {moved}px for a {expected}px detune"
    );
}

#[test]
fn the_recorded_envelope_outranks_an_authored_curve() {
    // A note with both: the waveform is what was recorded and carries
    // detail no control-point curve could, so it wins.
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    let mut n = Note::new(NoteId(1), 0.0, PPQ * 4.0, 60);
    n.weight = 0.5;
    // Pressure says "flat and quiet".
    n.pressure.set(0.0, 0.2);
    n.pressure.set(PPQ * 4.0, 0.2);
    // The recording says "loud, with grain".
    n.envelope = (0..200)
        .map(|k| if k % 2 == 0 { 1.0 } else { 0.85 })
        .collect();
    doc.push(n);
    let mut ed = Editor::new(doc, Viewport::new(900.0, 500.0));
    ed.set_mode(Mode::Audio);

    let rects = canvas::note_rects(&ed);
    let pts = points(rects[0].blob.as_ref().unwrap());
    let n_pts = pts.len();
    let thickness = (pts[n_pts - 1].1 - pts[0].1).abs();
    let quiet = 0.2 * ed.camera.px_per_semitone;
    assert!(
        thickness > quiet * 2.0,
        "the recording won, not the trim: {thickness}px"
    );
}

#[test]
fn a_note_too_narrow_to_shape_gets_no_blob() {
    let mut ed = editor(Mode::Audio, 0.0);
    // Zoom out until the note is a sliver.
    ed.camera.units_per_px = 1e6;
    let rects = canvas::note_rects(&ed);
    assert!(rects.is_empty() || rects[0].blob.is_none());
}

#[test]
fn colour_reports_how_far_out_of_tune_the_note_is() {
    // Dead on, and badly flat.
    let in_tune = canvas::note_rects(&editor(Mode::Audio, 0.0))[0].fill;
    let out = canvas::note_rects(&editor(Mode::Audio, -0.35))[0].fill;

    assert_eq!(in_tune, theme::TUNE_RAMP[0]);
    assert_eq!(out, theme::TUNE_RAMP[4]);
    assert_ne!(in_tune, out);
}

#[test]
fn vibrato_does_not_make_a_good_note_flash_red() {
    // The note is centred on pitch but swings ±60 cents four times
    // across its length. Colouring from an instant would strobe; the
    // colour comes from the contour's centre, so it stays in tune.
    let rects = canvas::note_rects(&editor(Mode::Audio, 0.0));
    assert_eq!(
        rects[0].fill,
        theme::TUNE_RAMP[0],
        "a vibrato around the target is in tune, not out"
    );
}

#[test]
fn the_ribbon_is_dropped_where_the_blob_already_shows_it() {
    let audio = canvas::note_rects(&editor(Mode::Audio, 0.0));
    assert!(
        audio[0].ribbon.is_none(),
        "the blob is the envelope; a ribbon would draw it twice, and in \
         the row rather than on the pitch"
    );
    // MPE still gets the ribbon — it has no blob to carry the reading.
    let mpe = canvas::note_rects(&editor(Mode::Mpe, 0.0));
    assert!(mpe[0].ribbon.is_some());
}

#[test]
fn an_unedited_note_still_has_a_body() {
    // No authored Pressure at all: the analysis weight has to carry the
    // shape, or a freshly-analyzed take renders as hairlines.
    let mut doc = ExpressionDoc::new(TimeBase::Ppq { ppq: PPQ }, 0.0, PPQ * 8.0);
    let mut n = Note::new(NoteId(1), 0.0, PPQ * 4.0, 60);
    n.weight = 0.9;
    doc.push(n);
    let mut ed = Editor::new(doc, Viewport::new(900.0, 500.0));
    ed.set_mode(Mode::Audio);

    let rects = canvas::note_rects(&ed);
    let pts = points(rects[0].blob.as_ref().unwrap());
    let n_pts = pts.len();
    let thickness = (pts[n_pts - 1].1 - pts[0].1).abs();
    assert!(thickness > 2.0, "got {thickness}px of body");
}

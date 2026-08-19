//! The slip drag on a role lane's hit, driven on the real surface.
//!
//! The claim: pressing on a hit line in a role lane and dragging
//! sideways emits `on_slip(hit_secs, next_secs, delta_secs)` on
//! release — and a press away from any hit still switches lanes rather
//! than slipping. The daw write is the host's; the gesture's contract
//! is these three numbers.

use std::cell::RefCell;

use dioxus::prelude::*;
use dioxus_test::{by_testid, render};
use expression_editor_core::kit::LaneRole;
use expression_editor_core::{Editor, ExpressionDoc, Mode, Note, NoteId, TimeBase, Viewport};
use expression_editor_ui::{canvas, stack};

const VP_W: f64 = 1000.0;
const VP_H: f64 = 400.0;
const RATE: f64 = 100.0;

thread_local! {
    static STAGED: RefCell<Option<Editor>> = const { RefCell::new(None) };
    static GOT: RefCell<Option<(f64, f64, f64)>> = const { RefCell::new(None) };
}

/// A one-lane kit: a kick with hits at 1.0 s and 2.0 s in a 4 s doc.
fn kit_editor() -> Editor {
    let mut doc = ExpressionDoc::new(TimeBase::Frames { frame_rate: RATE }, 0.0, RATE * 4.0);
    doc.push(Note::new(NoteId(1), RATE, RATE * 2.0, 1));
    doc.push(Note::new(NoteId(2), RATE * 2.0, RATE * 3.0, 1));
    let mut ed = Editor::new(doc, Viewport::new(VP_W, VP_H));
    ed.set_mode(Mode::UnpitchedAudio);
    ed.tracks.rename(0, "Kick In");
    let guid = ed.tracks.track(0).unwrap().guid.clone();
    ed.tracks.fold_roles(&[(guid, LaneRole::Kick)]);
    ed.stacked = true;
    ed.reset_view();
    ed
}

#[component]
fn Surface() -> Element {
    let editor = use_signal(|| {
        STAGED
            .with(|s| s.borrow_mut().take())
            .expect("an editor was staged")
    });
    rsx! {
        div {
            style: "width: {VP_W + canvas::GUTTER_W}px; height: {VP_H + canvas::RULER_H}px;",
            "data-testid": "stack",
            stack::StackView {
                editor,
                on_slip: move |v: (f64, f64, f64)| {
                    GOT.with(|g| *g.borrow_mut() = Some(v));
                },
            }
        }
    }
}

// r[verify drums.manual.slip]
#[tokio::test]
async fn dragging_a_hit_emits_the_slip() -> dioxus_test::Result<()> {
    let ed = kit_editor();
    // Where the first hit draws, in element coordinates — read from the
    // same layout the view renders, so the press lands on the line by
    // construction.
    let views = stack::lanes(&ed, 1.15, 24.0);
    let first = views
        .iter()
        .flat_map(|l| l.notes.iter())
        .min_by(|a, b| a.at_secs.total_cmp(&b.at_secs))
        .expect("a hit to drag");
    let hit_x = canvas::GUTTER_W + first.x;
    assert!((first.at_secs - 1.0).abs() < 1e-6);
    STAGED.with(|s| *s.borrow_mut() = Some(ed));
    GOT.with(|g| *g.borrow_mut() = None);

    let tester = render(Surface)
        .with_window_size(
            (VP_W + canvas::GUTTER_W) as u32,
            (VP_H + canvas::RULER_H) as u32,
        )
        .build();
    tester.drain();
    tester.relayout();
    // The div is not at the window origin (body margin); pointer
    // coordinates are window-space, the handler's are element-space.
    let (ox, oy) = tester
        .query(by_testid("stack"))
        .immediately()?
        .document_origin();
    let hit_x = hit_x + ox;

    let y = oy + canvas::RULER_H + VP_H / 2.0;
    let to_x = hit_x + 40.0;
    tester.pointer_down(hit_x + 1.0, y);
    tester.drain();
    for i in 1..=10 {
        let t = i as f64 / 10.0;
        tester.pointer_move(hit_x + 1.0 + (to_x - hit_x - 1.0) * t, y, true);
        tester.drain();
    }
    tester.pointer_up(to_x, y);
    let _ = tester.pump().await;

    let got = GOT.with(|g| *g.borrow());
    let (hit, next, delta) = got.expect("the drag emitted a slip");
    assert!((hit - 1.0).abs() < 0.02, "hit at 1.0 s, got {hit}");
    assert!((next - 2.0).abs() < 0.02, "next at 2.0 s, got {next}");
    // 40 px over a 4 s view in 1000 px ≈ 0.16 s, dragged later.
    assert!(
        (delta - 0.16).abs() < 0.02,
        "delta ≈ 0.16 s later, got {delta}"
    );
    Ok(())
}

// r[verify drums.manual.slip]
#[tokio::test]
async fn a_press_away_from_any_hit_does_not_slip() -> dioxus_test::Result<()> {
    let ed = kit_editor();
    let far_x = canvas::GUTTER_W + ed.camera.x(RATE * 0.5) - 30.0;
    STAGED.with(|s| *s.borrow_mut() = Some(ed));
    GOT.with(|g| *g.borrow_mut() = None);

    let tester = render(Surface)
        .with_window_size(
            (VP_W + canvas::GUTTER_W) as u32,
            (VP_H + canvas::RULER_H) as u32,
        )
        .build();
    tester.drain();
    tester.relayout();
    let (ox, oy) = tester
        .query(by_testid("stack"))
        .immediately()?
        .document_origin();
    let far_x = far_x + ox;

    let y = oy + canvas::RULER_H + VP_H / 2.0;
    tester.pointer_down(far_x, y);
    tester.drain();
    tester.pointer_move(far_x + 40.0, y, true);
    tester.drain();
    tester.pointer_up(far_x + 40.0, y);
    let _ = tester.pump().await;

    assert!(
        GOT.with(|g| g.borrow().is_none()),
        "no hit under the press, no slip"
    );
    Ok(())
}

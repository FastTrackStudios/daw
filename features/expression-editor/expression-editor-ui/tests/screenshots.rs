//! Visual-inspection harness: rasterize the real editor to PNGs.
//!
//! Mounts `ExpressionEditor` on the headless Blitz DOM and paints it
//! through `DocumentTester::render_png` — CPU rasterizer, no GPU, no
//! DAW, no browser. Nothing asserts about looks; a wrong-looking canvas
//! is a picture you have to open:
//!
//! ```sh
//! cargo test -p expression-editor-ui --test screenshots
//! ```
//!
//! Output lands in `target/gui-shots/expression-editor/` (override with
//! `FTS_SHOTS_DIR`).
//!
//! Scenes come from `expression_editor_ui::demo`, the same module the
//! runnable example mounts, so these pictures cannot drift from what
//! the app actually launches.

use std::path::PathBuf;

use dioxus::prelude::*;
use dioxus_test::DocumentTester;
use expression_editor_core::{Editor, Viewport};
use expression_editor_ui::demo::{self, Scene};
use expression_editor_ui::{ExpressionEditor, ModDrawer};

const W: u32 = 1100;
/// The roll's own height. The window has to hold this plus the top bar,
/// the chord row, the lane strip and the status bar — the canvas sizes
/// itself from its intrinsic aspect ratio, so overshooting here pushes
/// the status bar out of frame rather than shrinking the roll.
const CANVAS_H: f64 = 400.0;
const H: u32 = 700;

#[component]
fn Harness(seed: Editor, drawer: Option<ModDrawer>) -> Element {
    let editor = use_signal(|| seed.clone());
    rsx! {
        style {
            "html, body {{ width: 100%; height: 100%; margin: 0; padding: 0; \
              overflow: hidden; background: #0d0d11; }}"
        }
        div {
            style: "width: 100%; height: 100%;",
            ExpressionEditor { editor, initial_drawer: drawer.clone() }
        }
    }
}

fn shots_dir() -> PathBuf {
    let dir = std::env::var("FTS_SHOTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../target/gui-shots/expression-editor")
        });
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create {}: {e}", dir.display()));
    dir
}

async fn shoot(ed: Editor, name: &str) {
    shoot_with(ed, None, name).await
}

async fn shoot_with(ed: Editor, drawer: Option<ModDrawer>, name: &str) {
    // The canvas measures itself from the mounted element; headless
    // there is no resize event, so state the viewport the shot uses.
    let mut ed = ed;
    ed.resize(Viewport::new(W as f64, CANVAS_H));

    let dom = VirtualDom::new_with_props(
        Harness,
        HarnessProps { seed: ed, drawer },
    );
    let tester = DocumentTester::from_virtual_dom(dom)
        .with_window_size(W, H)
        .build();
    for _ in 0..4 {
        let _ = tester.pump().await;
    }
    let path = shots_dir().join(format!("{name}.png"));
    tester.render_png(&path);
    println!("shot: {}", path.display());
}

#[tokio::test(flavor = "current_thread")]
async fn shoot_every_scene() {
    for scene in Scene::ALL {
        let ed = demo::editor(scene, Viewport::new(W as f64, CANVAS_H));
        shoot(ed, scene.slug()).await;
    }
}

/// The same phrase at three zoom depths — the camera is most of the
/// feel, and it is the part a still picture can still show.
#[tokio::test(flavor = "current_thread")]
async fn shoot_zoom_levels() {
    let base = || demo::editor(Scene::Phrase, Viewport::new(W as f64, CANVAS_H));

    let mut ed = base();
    for _ in 0..6 {
        ed.zoom_in_at(W as f64 * 0.45, CANVAS_H * 0.5, 1.25);
    }
    shoot(ed, "08-zoom-in").await;

    // Near an item edge, where the edge magnet frames the boundary.
    let mut ed = base();
    for _ in 0..8 {
        ed.zoom_in_at(W as f64 * 0.95, CANVAS_H * 0.5, 1.25);
    }
    shoot(ed, "09-edge-magnet").await;
}

/// Before and after the Robot button, on the same note.
#[tokio::test(flavor = "current_thread")]
async fn shoot_flatten() {
    let vp = Viewport::new(W as f64, CANVAS_H);
    let mut ed = demo::editor(Scene::Phrase, vp);
    let id = ed.selection.notes[0];
    let (t0, t1) = {
        let n = ed.doc.note(id).unwrap();
        (n.start, n.end)
    };
    for _ in 0..3 {
        ed.zoom_in_at(W as f64 * 0.5, CANVAS_H * 0.5, 1.3);
    }
    shoot(ed.clone(), "10-as-sung").await;

    ed.apply(&expression_editor_core::Edit::ReblendPitch {
        note: id,
        t0,
        t1,
        drift_amount: 0.0,
        modulation_amount: 0.0,
    });
    shoot(ed, "11-robot").await;
}

/// The modulation drawer, opened through its real capture path.
#[tokio::test(flavor = "current_thread")]
async fn shoot_modulation_drawer() {
    let mut ed = demo::editor(Scene::Phrase, Viewport::new(W as f64, CANVAS_H));
    for _ in 0..3 {
        ed.zoom_in_at(W as f64 * 0.5, CANVAS_H * 0.5, 1.3);
    }
    let mut drawer = ModDrawer::default();
    assert!(drawer.open_on(&ed), "a note is selected, so it must open");
    drawer.preview(&mut ed);
    shoot_with(ed, Some(drawer), "12-modulation").await;
}

/// Contextual zoom on a part whose density changes: the same gesture at
/// two positions must produce two different zoom levels.
#[tokio::test(flavor = "current_thread")]
async fn shoot_smart_zoom() {
    use expression_editor_core::zoom::ZoomModes;

    let vp = Viewport::new(W as f64, CANVAS_H);
    let ed = demo::editor(Scene::Density, vp);

    let mut dense = ed.clone();
    dense.smart_zoom(ZoomModes::NOTE_AREA, demo::PPQ * 1.0, 62.0);
    shoot(dense, "13-smart-zoom-dense").await;

    let mut sparse = ed.clone();
    sparse.smart_zoom(ZoomModes::NOTE_AREA, demo::PPQ * 22.0, 62.0);
    shoot(sparse, "14-smart-zoom-sparse").await;

    let mut whole = ed;
    whole.smart_zoom(ZoomModes::KEYS, demo::PPQ * 1.0, 62.0);
    shoot(whole, "15-smart-zoom-item").await;
}

/// Razor edits: an area over held notes, and the result of moving it.
#[tokio::test(flavor = "current_thread")]
async fn shoot_razor() {
    use expression_editor_core::razor::{self, RazorArea};

    let vp = Viewport::new(W as f64, CANVAS_H);
    let mut ed = demo::editor(Scene::Held, vp);
    let area = RazorArea::new(demo::PPQ * 2.0, demo::PPQ * 4.0, 60, 67);

    ed.razor.add(area);
    shoot(ed.clone(), "17-razor-area").await;

    // Moving it slices the held notes at both edges and carries the
    // middle — the thing a marquee cannot do.
    razor::move_contents(&mut ed.doc, area, demo::PPQ * 4.0, 0, false);
    ed.razor.clear();
    ed.razor.add(area.translated(demo::PPQ * 4.0, 0));
    shoot(ed, "18-razor-moved").await;
}

/// The chord box on a real chord, with the velocity strip populated.
#[tokio::test(flavor = "current_thread")]
async fn shoot_chord_box() {
    let vp = Viewport::new(W as f64, CANVAS_H);
    let mut ed = demo::editor(Scene::Held, vp);
    // Five stacked notes: the box should name the chord, not shrug.
    ed.selection.notes = ed.doc.notes.iter().map(|n| n.id).collect();
    // Spread the velocities so the strip shows shape rather than a wall.
    for (i, n) in ed.doc.notes.iter_mut().enumerate() {
        n.velocity = 0.35 + 0.13 * i as f64;
    }
    shoot(ed, "20-chord-box").await;
}

/// Pinned controller lanes behind the roll, and CC edit mode.
#[tokio::test(flavor = "current_thread")]
async fn shoot_cc_lanes() {
    let vp = Viewport::new(W as f64, CANVAS_H);
    let ed = demo::editor(Scene::Orchestral, vp);
    shoot(ed.clone(), "22-cc-pinned").await;

    // Editing one brings it forward and pushes the notes back.
    let mut editing = ed;
    editing.edit_cc(11);
    shoot(editing.clone(), "23-cc-edit").await;

    // And drawing on the roll writes into that lane.
    let mut drag = expression_editor_ui::Drag::default();
    let vph = editing.viewport.h;
    let x0 = editing.camera.x(demo::PPQ * 1.0);
    drag = expression_editor_ui::interaction::pointer_down(
        &mut editing,
        x0,
        vph * 0.8,
        Default::default(),
        0,
    );
    for i in 1..=40 {
        let f = i as f64 / 40.0;
        let x = editing.camera.x(demo::PPQ * (1.0 + 6.0 * f));
        // A jagged shape, so the result is obviously hand-drawn rather
        // than the smooth curve the scene started with.
        let y = vph * (0.15 + 0.55 * (f * 9.0).sin().abs());
        expression_editor_ui::interaction::pointer_move(
            &mut editing,
            &mut drag,
            x,
            y,
            Default::default(),
        );
    }
    shoot(editing, "24-cc-drawn").await;
}

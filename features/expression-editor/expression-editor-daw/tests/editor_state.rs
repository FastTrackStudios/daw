//! Editor state that survives a save, against the standalone backend.
//!
//! The whole persistence design is exercised here with no DAW running,
//! because `daw-standalone` implements `project_ext_state` with the same
//! semantics REAPER has — verified against live REAPER in #189.

use daw::service::project::ProjectInfo;
use daw::service::{ExtState, ProjectContext};
use daw::standalone::Standalone;
use expression_editor_core::Mode;
use expression_editor_daw::state::{self, EditorState, NAMESPACE};

fn daw() -> (Standalone, ProjectContext) {
    let daw = Standalone::new();
    daw.seed_project(ProjectInfo {
        guid: "test-proj".into(),
        name: "Test".into(),
        path: String::new(),
    });
    (daw, ProjectContext::Current)
}

const TRACK: &str = "{TRACK-GUID}";
const TAKE: &str = "{TAKE-GUID}";

// ── The tracer bullet ────────────────────────────────────────────────

#[test]
fn a_correction_survives_a_save_and_reload() {
    let (d, project) = daw();

    let mut state = EditorState::default();
    state.correct_take(TAKE, Mode::Drums);
    state::save(&d, project.clone(), &state).expect("save");

    // Reload from nothing but what is stored.
    let reloaded = state::load(&d, project);
    assert_eq!(
        reloaded.mode_for(TRACK, TAKE),
        Some(Mode::Drums),
        "the correction is still there next session"
    );
}

#[test]
fn nothing_stored_means_nobody_corrected_anything() {
    let (d, project) = daw();
    let state = state::load(&d, project);
    assert!(state.is_empty());
    assert_eq!(
        state.mode_for(TRACK, TAKE),
        None,
        "None means infer, not 'no mode'"
    );
}

// ── The cascade ──────────────────────────────────────────────────────

#[test]
fn resolution_is_take_then_track_then_infer() {
    let mut state = EditorState::default();
    assert_eq!(state.mode_for(TRACK, TAKE), None);

    state.correct_track(TRACK, Mode::Vocals);
    assert_eq!(
        state.mode_for(TRACK, TAKE),
        Some(Mode::Vocals),
        "the track's correction applies to its takes"
    );

    state.correct_take(TAKE, Mode::Drums);
    assert_eq!(
        state.mode_for(TRACK, TAKE),
        Some(Mode::Drums),
        "and one odd comp can disagree"
    );

    state.clear_take(TAKE);
    assert_eq!(
        state.mode_for(TRACK, TAKE),
        Some(Mode::Vocals),
        "clearing the take falls back to the track"
    );
}

#[test]
fn a_track_correction_reaches_a_take_recorded_later() {
    let (d, project) = daw();
    let mut state = EditorState::default();
    state.correct_track(TRACK, Mode::UnpitchedAudio);
    state::save(&d, project.clone(), &state).unwrap();

    // A take that did not exist when the correction was made.
    let later = "{TAKE-RECORDED-LATER}";
    let reloaded = state::load(&d, project);
    assert_eq!(reloaded.mode_for(TRACK, later), Some(Mode::UnpitchedAudio));
}

#[test]
fn correcting_the_same_guid_twice_replaces_rather_than_appends() {
    let mut state = EditorState::default();
    state.correct_take(TAKE, Mode::Midi);
    state.correct_take(TAKE, Mode::Guitar);
    assert_eq!(state.take_modes.len(), 1);
    assert_eq!(state.mode_for(TRACK, TAKE), Some(Mode::Guitar));
}

// ── Only corrections are written ─────────────────────────────────────

#[test]
fn inference_is_absent_from_what_is_written() {
    let (d, project) = daw();
    // An editor that inferred Drums for a hundred takes and was never
    // corrected writes nothing about them.
    let state = EditorState::default();
    state::save(&d, project.clone(), &state).unwrap();

    let raw = d
        .get_project(project, "fts.side", NAMESPACE)
        .unwrap_or_default();
    assert!(
        !raw.contains("Drums"),
        "no inferred mode reached the project: {raw}"
    );
}

#[test]
fn an_affirmed_correction_is_written_even_when_it_matches_inference() {
    // "The user affirmed this" is worth pinning against a future
    // heuristic change, so the store does not second-guess it.
    let (d, project) = daw();
    let mut state = EditorState::default();
    state.correct_take(TAKE, Mode::Midi);
    state::save(&d, project.clone(), &state).unwrap();

    assert_eq!(
        state::load(&d, project).mode_for(TRACK, TAKE),
        Some(Mode::Midi)
    );
}

// ── Versioning ───────────────────────────────────────────────────────

#[test]
fn a_version_from_the_future_falls_back_to_defaults() {
    let (d, project) = daw();
    let mut state = EditorState::default();
    state.version = state::CURRENT_VERSION + 1;
    state.correct_take(TAKE, Mode::Drums);
    state::save(&d, project.clone(), &state).unwrap();

    let reloaded = state::load(&d, project);
    assert!(
        reloaded.is_empty(),
        "discarded whole rather than half-read — a field whose meaning \
         changed would otherwise be misread as if it had not"
    );
}

#[test]
fn an_unparseable_blob_falls_back_to_defaults_rather_than_erroring() {
    let (d, project) = daw();
    d.set_project(project.clone(), "fts.side", NAMESPACE, "not styx {{{")
        .unwrap();

    let reloaded = state::load(&d, project);
    assert!(reloaded.is_empty(), "no error reaches the caller");
}

#[test]
fn an_empty_blob_is_the_same_as_nothing_stored() {
    let (d, project) = daw();
    d.set_project(project.clone(), "fts.side", NAMESPACE, "")
        .unwrap();
    assert!(state::load(&d, project).is_empty());
}

// ── The namespace ────────────────────────────────────────────────────

#[test]
fn another_feature_cannot_collide_with_the_editors_state() {
    use daw::service::ext_state::side_store;

    let (d, project) = daw();
    let mut state = EditorState::default();
    state.correct_take(TAKE, Mode::Guitar);
    state::save(&d, project.clone(), &state).unwrap();

    // A different feature writing under its own namespace.
    side_store::store(&d, project.clone(), "some-other-feature", "whatever").unwrap();

    assert_eq!(
        state::load(&d, project.clone()).mode_for(TRACK, TAKE),
        Some(Mode::Guitar),
        "the editor's namespace is untouched"
    );
    assert_eq!(
        side_store::load(&d, project, "some-other-feature").as_deref(),
        Some("whatever")
    );
}

// ── Improving the heuristic ──────────────────────────────────────────
//
// The reason inference is never persisted: a better heuristic should
// reach every take *except* the ones somebody explicitly overrode. These
// stand in for that by changing the inference and watching who moves.

#[test]
fn a_better_heuristic_reaches_uncorrected_takes() {
    let (d, project) = daw();
    let before = state::resolve_mode(&d, project.clone(), TRACK, TAKE, || Mode::Midi);
    assert_eq!(before, Mode::Midi);

    // Same take, same stored state, smarter guess.
    let after = state::resolve_mode(&d, project, TRACK, TAKE, || Mode::PitchedAudio);
    assert_eq!(
        after,
        Mode::PitchedAudio,
        "nobody corrected this, so the improvement lands"
    );
}

#[test]
fn a_better_heuristic_does_not_override_a_correction() {
    let (d, project) = daw();
    state::correct_take_mode(&d, project.clone(), TAKE, Mode::Drums).unwrap();

    let resolved = state::resolve_mode(&d, project, TRACK, TAKE, || Mode::PitchedAudio);
    assert_eq!(
        resolved,
        Mode::Drums,
        "a human said Drums; a cleverer guess does not get to disagree"
    );
}

#[test]
fn correcting_a_track_reaches_its_takes_through_the_resolver() {
    let (d, project) = daw();
    state::correct_track_mode(&d, project.clone(), TRACK, Mode::Vocals).unwrap();

    let resolved = state::resolve_mode(&d, project, TRACK, "{ANY-TAKE}", || Mode::Midi);
    assert_eq!(resolved, Mode::Vocals);
}

#[test]
fn a_take_correction_still_beats_its_tracks_through_the_resolver() {
    let (d, project) = daw();
    state::correct_track_mode(&d, project.clone(), TRACK, Mode::Vocals).unwrap();
    state::correct_take_mode(&d, project.clone(), TAKE, Mode::Guitar).unwrap();

    assert_eq!(
        state::resolve_mode(&d, project.clone(), TRACK, TAKE, || Mode::Midi),
        Mode::Guitar
    );
    // ...and a sibling take on that track still follows the track.
    assert_eq!(
        state::resolve_mode(&d, project, TRACK, "{SIBLING}", || Mode::Midi),
        Mode::Vocals
    );
}

#[test]
fn corrections_accumulate_across_separate_writes() {
    // Each correction is a read-modify-write of one blob, so this is the
    // case where a careless implementation loses the earlier one.
    let (d, project) = daw();
    state::correct_track_mode(&d, project.clone(), TRACK, Mode::Vocals).unwrap();
    state::correct_take_mode(&d, project.clone(), TAKE, Mode::Drums).unwrap();
    state::correct_take_mode(&d, project.clone(), "{OTHER}", Mode::Guitar).unwrap();

    let s = state::load(&d, project);
    assert_eq!(s.track_modes.len(), 1);
    assert_eq!(s.take_modes.len(), 2);
    assert_eq!(s.mode_for(TRACK, TAKE), Some(Mode::Drums));
    assert_eq!(s.mode_for(TRACK, "{OTHER}"), Some(Mode::Guitar));
    assert_eq!(s.mode_for(TRACK, "{THIRD}"), Some(Mode::Vocals));
}

// ── The rest of the per-take bucket (#192) ───────────────────────────

use expression_editor_core::{Dimension, Editor, StripLane, Viewport, tuning};
use expression_editor_daw::state::{TakeView, TuningRef};

fn editor() -> Editor {
    Editor::new(
        expression_editor_core::ExpressionDoc::new(
            expression_editor_core::TimeBase::Ppq { ppq: 960.0 },
            0.0,
            3840.0,
        ),
        Viewport::new(800.0, 600.0),
    )
}

#[test]
fn the_view_a_take_was_left_in_comes_back() {
    let (d, project) = daw();

    let mut ed = editor();
    ed.dimension = Dimension::Pressure;
    ed.overlays = vec![Dimension::Pitch, Dimension::Timbre];
    ed.strip_lane = StripLane::OffVelocity;
    ed.lane_strip_h = 87.0;
    ed.cc_display.background_opacity = 0.42;
    ed.tuning.key_pc = 7;
    ed.tuning.snap_12tet = false;

    let mut state = EditorState::default();
    state.set_view(TakeView::capture(TAKE, &ed));
    state::save(&d, project.clone(), &state).unwrap();

    let mut fresh = editor();
    state::load(&d, project)
        .view_for(TAKE)
        .expect("stored")
        .apply(&mut fresh);

    assert_eq!(fresh.dimension, Dimension::Pressure);
    assert_eq!(fresh.overlays, vec![Dimension::Pitch, Dimension::Timbre]);
    assert_eq!(fresh.strip_lane, StripLane::OffVelocity);
    assert_eq!(fresh.lane_strip_h, 87.0);
    assert_eq!(fresh.cc_display.background_opacity, 0.42);
    assert_eq!(fresh.tuning.key_pc, 7);
    assert!(!fresh.tuning.snap_12tet);
}

#[test]
fn the_camera_is_not_part_of_it() {
    // Vertical zoom and scroll position are deliberately not saved: the
    // project should open fitted to *your* screen, not the sender's.
    let (d, project) = daw();
    let mut ed = editor();
    ed.camera.t0 = 12345.0;
    ed.camera.vertical.center = 99.0;

    let mut state = EditorState::default();
    state.set_view(TakeView::capture(TAKE, &ed));
    state::save(&d, project.clone(), &state).unwrap();

    let raw = d
        .get_project(project.clone(), "fts.side", NAMESPACE)
        .unwrap_or_default();
    assert!(!raw.contains("12345"), "no camera reached the project: {raw}");

    let mut fresh = editor();
    let before = fresh.camera;
    state::load(&d, project)
        .view_for(TAKE)
        .unwrap()
        .apply(&mut fresh);
    assert_eq!(fresh.camera, before, "applying a view does not move the camera");
}

#[test]
fn applying_a_view_leaves_ephemeral_state_alone() {
    // Restoring a selection the user did not make is a hazard, not a
    // convenience — same for the razor and the clipboard.
    let mut ed = editor();
    let view = TakeView::capture(TAKE, &ed);

    ed.stacked = true;
    ed.timing_mode = true;
    ed.sibilant_scope = true;
    view.apply(&mut ed);

    assert!(ed.stacked, "a view flag is not a stored view");
    assert!(ed.timing_mode);
    assert!(ed.sibilant_scope);
}

#[test]
fn a_tuning_is_stored_by_name_not_by_value() {
    let mut t = expression_editor_core::Tuning::default();
    t.temperament = tuning::RAST.clone();
    t.key_pc = 2;

    let stored = TuningRef::of(&t);
    assert_eq!(stored.temperament, tuning::RAST.name);

    let back = stored.resolve();
    assert_eq!(back.temperament.name, tuning::RAST.name);
    assert_eq!(back.key_pc, 2);
}

#[test]
fn an_unknown_temperament_falls_back_rather_than_guessing() {
    let stored = TuningRef {
        temperament: "Temperament From A Later Build".into(),
        key_pc: 5,
        snap_12tet: true,
    };
    let back = stored.resolve();
    assert_eq!(
        back.temperament.name,
        expression_editor_core::Tuning::default().temperament.name,
        "the default tuning, not a guessed one"
    );
    assert_eq!(back.key_pc, 5, "the rest of the setting still applies");
}

#[test]
fn each_take_keeps_its_own_view() {
    let mut state = EditorState::default();
    let mut a = editor();
    a.dimension = Dimension::Pressure;
    let mut b = editor();
    b.dimension = Dimension::Timbre;

    state.set_view(TakeView::capture("take-a", &a));
    state.set_view(TakeView::capture("take-b", &b));

    assert_eq!(state.view_for("take-a").unwrap().dimension, Dimension::Pressure);
    assert_eq!(state.view_for("take-b").unwrap().dimension, Dimension::Timbre);
    assert_eq!(state.take_views.len(), 2);
}

#[test]
fn re_capturing_a_take_replaces_its_view() {
    let mut state = EditorState::default();
    let mut ed = editor();
    ed.dimension = Dimension::Pressure;
    state.set_view(TakeView::capture(TAKE, &ed));
    ed.dimension = Dimension::Timbre;
    state.set_view(TakeView::capture(TAKE, &ed));

    assert_eq!(state.take_views.len(), 1);
    assert_eq!(state.view_for(TAKE).unwrap().dimension, Dimension::Timbre);
}

// ── The lane layout (#201) ───────────────────────────────────────────

use expression_editor_core::{ExpressionDoc as Doc, TimeBase, Track, Workspace};
use expression_editor_daw::state::StoredLayout;

fn doc_for(_n: &str) -> Doc {
    Doc::new(TimeBase::Ppq { ppq: 960.0 }, 0.0, 3840.0)
}

fn workspace(names: &[&str]) -> Workspace {
    let mut ws = Workspace::single(names[0], doc_for(names[0]));
    for n in &names[1..] {
        ws.push(Track::new(*n, doc_for(n)));
    }
    ws.auto_pair();
    ws
}

/// The same tracks with the *same guids* — what reopening a project
/// looks like. `Track::new` mints a fresh guid each time, so a plain
/// rebuild is a different project as far as a stored layout is
/// concerned, and correctly refuses to apply.
fn reopened(names: &[&str]) -> Workspace {
    let mut ws = Workspace::single("", doc_for(""));
    {
        let t = ws.track_mut(0).unwrap();
        t.guid = format!("g-{}", names[0]);
        t.name = names[0].to_string();
    }
    for n in &names[1..] {
        ws.push(Track::with_guid(format!("g-{n}"), *n, doc_for(n)));
    }
    // Rebuild the inferred layout against the corrected guids.
    ws.layout_mut().mark_arranged();
    *ws.layout_mut() = Default::default();
    ws.auto_pair();
    ws
}

#[test]
fn an_inferred_layout_is_not_stored() {
    let ws = workspace(&["Lead Vox", "Lead Vox MIDI", "Kit"]);
    assert!(!ws.layout().is_arranged());
    assert!(
        !StoredLayout::capture(&ws).is_arranged(),
        "the matcher recomputes it on load; storing it would just go stale"
    );
}

#[test]
fn a_hand_arranged_layout_survives_a_save() {
    let (d, project) = daw();
    let mut ws = reopened(&["A", "B", "C"]);
    ws.merge_lanes(0, 1);
    assert_eq!(ws.layout().len(), 2);

    let mut state = EditorState::default();
    state.layout = StoredLayout::capture(&ws);
    state::save(&d, project.clone(), &state).unwrap();

    let mut fresh = reopened(&["A", "B", "C"]);
    // The matcher would leave three lanes; the stored arrangement wins.
    assert_eq!(fresh.layout().len(), 3);
    state::load(&d, project).layout.apply(&mut fresh);

    assert_eq!(fresh.layout().len(), 2);
    assert_eq!(fresh.lane_tracks(0).len(), 2);
    assert!(fresh.layout().is_arranged());
}

#[test]
fn the_layout_is_stored_once_per_project_not_per_take() {
    // Twenty tracks, one layout. Keyed by take it would be twenty
    // conflicting copies of the same thing.
    let mut ws = workspace(&["A", "B"]);
    ws.merge_lanes(0, 1);
    let state = EditorState {
        layout: StoredLayout::capture(&ws),
        ..Default::default()
    };
    assert_eq!(state.layout.lanes.len(), 1);
    assert!(state.take_views.is_empty(), "no per-take copy of it");
}

#[test]
fn a_lane_weight_survives_with_the_layout() {
    let (d, project) = daw();
    let mut ws = reopened(&["A", "B"]);
    ws.merge_lanes(0, 1);
    ws.layout_mut().lane_mut(0).unwrap().weight = 6.5;

    let state = EditorState {
        layout: StoredLayout::capture(&ws),
        ..Default::default()
    };
    state::save(&d, project.clone(), &state).unwrap();

    let mut fresh = reopened(&["A", "B"]);
    state::load(&d, project).layout.apply(&mut fresh);
    assert_eq!(fresh.layout().lane(0).unwrap().weight, 6.5);
}

#[test]
fn a_track_added_since_the_layout_was_written_is_matched_in() {
    let mut ws = reopened(&["Lead Vox", "Kit"]);
    ws.merge_lanes(0, 1);
    let stored = StoredLayout::capture(&ws);

    // Reopen with an extra comp on the vocal.
    let mut fresh = reopened(&["Lead Vox", "Kit", "Lead Vox Audio"]);
    stored.apply(&mut fresh);

    let vox = fresh.index_of("Lead Vox").unwrap();
    let comp = fresh.index_of("Lead Vox Audio").unwrap();
    let vox_lane = fresh.layout().lane_of(&fresh.track(vox).unwrap().guid.clone()).unwrap();
    assert!(
        fresh.lane_tracks(vox_lane).contains(&comp),
        "the new comp joined the vocal's lane rather than being appended"
    );
}

#[test]
fn a_stale_entry_is_dropped_and_the_rest_survives() {
    let mut ws = reopened(&["A", "B", "C"]);
    ws.merge_lanes(0, 1);
    let mut stored = StoredLayout::capture(&ws);
    // A guid nobody has: what a deleted track leaves behind.
    stored.lanes[0].tracks.push("{GONE}".into());

    let mut fresh = reopened(&["A", "B", "C"]);
    stored.apply(&mut fresh);

    let all: Vec<usize> = (0..fresh.layout().len())
        .flat_map(|i| fresh.lane_tracks(i))
        .collect();
    assert_eq!(all.len(), 3, "every real track is still placed");
}

#[test]
fn a_layout_whose_tracks_are_all_gone_does_not_leave_empty_lanes() {
    let mut ws = reopened(&["A", "B"]);
    ws.merge_lanes(0, 1);
    let stored = StoredLayout::capture(&ws);

    // Reopened against a completely different project.
    let mut other = reopened(&["X", "Y"]);
    stored.apply(&mut other);

    assert_eq!(other.layout().len(), 2, "X and Y each got a lane");
    for i in 0..other.layout().len() {
        assert!(!other.lane_tracks(i).is_empty());
    }
}

#[test]
fn a_layout_from_a_different_project_is_ignored_rather_than_applied() {
    // Guids are how a layout knows it is looking at the same tracks. A
    // layout carried into a project whose tracks it has never seen
    // applies to nothing — which is right, and is also the trap that
    // makes a test built from freshly-minted tracks look broken.
    let mut ws = reopened(&["A", "B"]);
    ws.merge_lanes(0, 1);
    let stored = StoredLayout::capture(&ws);

    let mut elsewhere = workspace(&["A", "B"]); // same names, new guids
    stored.apply(&mut elsewhere);

    assert_eq!(
        elsewhere.layout().len(),
        2,
        "the matcher placed them; the foreign layout did not"
    );
}

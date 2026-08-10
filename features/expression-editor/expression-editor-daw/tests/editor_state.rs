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

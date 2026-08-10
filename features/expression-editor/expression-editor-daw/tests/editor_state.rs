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

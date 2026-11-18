//! A project on disk: the directory's two entries, the old extension, the
//! modified flag, a missing object, and a project opened from its parts.
//!
//! These build their project from REAPER text, so they live here rather
//! than beside `DawProject`: the `.rpp` reader is `dawfile-reaper`'s
//! (`dawfile_reaper::session`), which depends on this crate.

use std::path::PathBuf;

use dawfile_reaper::session::RppProject;
use session_format::{DawError, DawProject, OBJECTS_DIR, ObjectStore, choose_manifest};

const TINY: &str = r#"<REAPER_PROJECT 0.1 "7.0/test" 1700000000
  SAMPLERATE 48000 0 0
  TEMPO 120 4 4
  <TRACK {AAAAAAAA-0001-0000-0000-000000000000}
    NAME Kick
    VOLPAN 1 0 -1 -1 1
    TRACKID {AAAAAAAA-0001-0000-0000-000000000000}
  >
>
"#;

fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join(format!("daw-project-{}", uuid::Uuid::new_v4()))
}

#[test]
fn a_project_directory_holds_exactly_two_entries() {
    let dir = scratch_dir();
    let (mut project, _) = DawProject::import_rpp(TINY, "Tiny").expect("import");
    project.save(&dir).expect("save");

    let mut entries: Vec<String> = std::fs::read_dir(&dir)
        .expect("read")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    entries.sort();
    assert_eq!(
        entries,
        vec!["Tiny.session".to_string(), "objects".to_string()]
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_project_saved_under_the_old_extension_still_opens_and_is_renamed() {
    // `.daw` was the extension before `.session`. The bytes never
    // changed, so an older project must open as it is — and saving it
    // must leave one manifest behind, not two.
    let dir = scratch_dir();
    let (mut project, _) = DawProject::import_rpp(TINY, "Tiny").expect("import");
    project.save(&dir).expect("save");

    std::fs::rename(dir.join("Tiny.session"), dir.join("Tiny.daw")).expect("rename");
    let mut reopened = DawProject::load(&dir).expect("an older project still opens");
    assert_eq!(reopened.document().tracks.len(), 1);

    reopened.save(&dir).expect("save");
    let mut entries: Vec<String> = std::fs::read_dir(&dir)
        .expect("read")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    entries.sort();
    assert_eq!(
        entries,
        vec!["Tiny.session".to_string(), "objects".to_string()],
        "saving must retire the old manifest rather than keep both"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_saved_project_reloads_with_the_same_entities() {
    let dir = scratch_dir();
    let (mut project, _) = DawProject::import_rpp(TINY, "Tiny").expect("import");
    project.save(&dir).expect("save");

    let reloaded = DawProject::load(&dir).expect("load");
    assert_eq!(reloaded.document().tracks.len(), 1);
    assert_eq!(reloaded.document().tracks[0].track.name, "Kick");
    assert_eq!(reloaded.document().sample_rate, Some(48_000));
    assert!(reloaded.document().check_invariants().is_empty());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn saving_clears_the_modified_flag_and_editing_sets_it() {
    let dir = scratch_dir();
    let (mut project, _) = DawProject::import_rpp(TINY, "Tiny").expect("import");
    assert!(!project.is_modified());
    project.edit(|document| document.tracks[0].track.volume = 0.5);
    assert!(project.is_modified());
    project.save(&dir).expect("save");
    assert!(!project.is_modified());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_manifest_referencing_a_missing_object_fails_loudly() {
    let dir = scratch_dir();
    let (mut project, _) = DawProject::import_rpp(TINY, "Tiny").expect("import");
    project.save(&dir).expect("save");

    // Simulate the object half not having finished syncing.
    std::fs::remove_dir_all(dir.join(OBJECTS_DIR)).expect("remove objects");

    assert!(matches!(
        DawProject::load(&dir),
        Err(DawError::MissingObject { .. })
    ));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_directory_without_a_manifest_is_not_a_project() {
    let dir = scratch_dir();
    std::fs::create_dir_all(&dir).expect("mkdir");
    assert!(matches!(
        DawProject::load(&dir),
        Err(DawError::NotAProject { .. })
    ));
    std::fs::remove_dir_all(&dir).ok();
}
#[test]
fn a_project_opens_from_its_parts_as_from_its_directory() {
    let dir = std::env::temp_dir().join(format!("dawfile-parts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (mut project, _) = DawProject::import_rpp(TINY, "Tiny").expect("import");
    project.save(&dir).expect("save");
    let from_disk = DawProject::load(&dir).expect("load");

    // The same folder, as files that arrived from elsewhere.
    let mut names = Vec::new();
    let mut objects = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        names.push(entry.unwrap().file_name().to_string_lossy().into_owned());
    }
    for entry in std::fs::read_dir(dir.join(OBJECTS_DIR)).unwrap() {
        let entry = entry.unwrap();
        objects.push((
            entry.file_name().to_string_lossy().into_owned(),
            std::fs::read(entry.path()).unwrap(),
        ));
    }
    let manifest = choose_manifest(&names).expect("one manifest");
    let text = std::fs::read_to_string(dir.join(manifest)).unwrap();
    let from_parts =
        DawProject::from_parts(&text, manifest, ObjectStore::from_named(objects).unwrap())
            .expect("from parts");
    assert_eq!(from_parts.to_rpp().unwrap(), from_disk.to_rpp().unwrap());

    // A blob that is not what its name says is refused, as on disk.
    assert!(ObjectStore::from_named([("sha256-00".to_owned(), b"x".to_vec())]).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

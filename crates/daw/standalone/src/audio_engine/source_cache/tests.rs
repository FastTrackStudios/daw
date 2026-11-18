use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;

use super::{MediaKey, SourceCache};
use crate::audio_engine::{AudioSource, DecodedAudio};

fn key(name: &str) -> MediaKey {
    MediaKey::File {
        path: PathBuf::from(name),
        len: 1,
        modified: None,
    }
}

fn tone() -> AudioSource {
    AudioSource::Memory(DecodedAudio::new(vec![0.5; 64], 1, 48_000))
}

#[test]
fn a_file_loads_once_while_it_is_played() {
    let cache = SourceCache::default();
    let loads = Cell::new(0);
    let load = || {
        loads.set(loads.get() + 1);
        Ok::<_, ()>(tone())
    };

    let a = cache.get_or_load(key("kick.wav"), load).unwrap();
    let b = cache.get_or_load(key("kick.wav"), load).unwrap();

    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!((loads.get(), cache.live()), (1, 1));
}

#[test]
fn a_file_nothing_plays_is_unloaded() {
    let cache = SourceCache::default();
    let a = cache
        .get_or_load(key("kick.wav"), || Ok::<_, ()>(tone()))
        .unwrap();
    drop(a);
    assert_eq!(cache.live(), 0);

    let loads = Cell::new(0);
    let _b = cache
        .get_or_load(key("kick.wav"), || {
            loads.set(loads.get() + 1);
            Ok::<_, ()>(tone())
        })
        .unwrap();

    assert_eq!(loads.get(), 1, "loaded again, not handed a dropped source");
}

#[test]
fn a_failed_load_is_tried_again() {
    let cache = SourceCache::default();
    assert_eq!(
        cache
            .get_or_load(key("gone.wav"), || Err::<AudioSource, _>("missing"))
            .err(),
        Some("missing")
    );

    let got = cache.get_or_load(key("gone.wav"), || Ok::<_, &str>(tone()));

    assert!(got.is_ok());
}

#[test]
fn different_files_are_different_sources() {
    let cache = SourceCache::default();
    let a = cache
        .get_or_load(key("kick.wav"), || Ok::<_, ()>(tone()))
        .unwrap();
    let b = cache
        .get_or_load(key("snare.wav"), || Ok::<_, ()>(tone()))
        .unwrap();

    assert!(!Arc::ptr_eq(&a, &b));
    assert_eq!(cache.live(), 2);
}

#[test]
fn the_same_bytes_are_the_same_media_and_other_bytes_are_not() {
    let a = MediaKey::of_bytes("Kick.wav", b"RIFF....WAVE one");
    assert_eq!(a, MediaKey::of_bytes("Kick.wav", b"RIFF....WAVE one"));
    assert_ne!(a, MediaKey::of_bytes("Kick.wav", b"RIFF....WAVE two"));
}

// Lab copies of a song link its recordings from a folder of their own.
#[cfg(unix)]
#[test]
fn a_file_and_a_link_to_it_are_the_same_media() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Kick.wav");
    std::fs::write(&file, b"RIFF").unwrap();
    let link = dir.path().join("Kick link.wav");
    std::os::unix::fs::symlink(&file, &link).unwrap();

    assert_eq!(MediaKey::of_file(&file), MediaKey::of_file(&link));
}

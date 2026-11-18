//! Every take of one file plays one source (#311).

use std::sync::Arc;

use daw_proto::{ItemRef, ProjectContext, ProjectInfo, TakeRef};

use super::detach_audio_source;
use crate::audio_engine::AudioSource;
use crate::media_seed::{StemSpec, seed_media_tracks};
use crate::sync::Standalone;
use crate::take_reader::TakeReader;

/// A mono WAV of `frames` samples, `bits` wide (8 or 16), as bytes.
fn wav(frames: usize, bits: u16) -> Vec<u8> {
    let width = usize::from(bits / 8);
    let data: Vec<u8> = (0..frames)
        .flat_map(|i| {
            let v = ((i % 200) as f32 / 100.0 - 1.0) * 0.5;
            if bits == 8 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let s = (v * 127.0 + 128.0) as u8;
                vec![s]
            } else {
                #[allow(clippy::cast_possible_truncation)]
                let s = (v * 32767.0) as i16;
                s.to_le_bytes().to_vec()
            }
        })
        .collect();
    let rate = 48_000u32;
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&u32::try_from(36 + data.len()).unwrap().to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(bits / 8)).to_le_bytes());
    out.extend_from_slice(&u16::try_from(width).unwrap().to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// A song of two tracks whose items are both cut from the one file
/// `bytes` is written to, materialized as a project opens; and its takes.
fn two_takes_of_one_file(
    dir: &tempfile::TempDir,
    bytes: &[u8],
) -> (Standalone, &'static str, Vec<String>) {
    let path = dir.path().join("Kick In.wav");
    std::fs::write(&path, bytes).unwrap();
    let path = path.to_string_lossy().into_owned();
    let daw = Standalone::new();
    let guid = "song";
    daw.seed_project(ProjectInfo {
        guid: guid.into(),
        name: guid.into(),
        path: String::new(),
    });
    let stems = [
        StemSpec::new("Kick In", path.clone(), None),
        StemSpec::new("Kick In 2", path, None),
    ];
    let report = seed_media_tracks(&daw, guid, &stems, 0.0, 1.0);
    assert_eq!(report.materialize.failed, Vec::new());
    let takes = daw
        .read_project(guid, |p| {
            let mut takes: Vec<String> = p
                .takes
                .values()
                .flat_map(|tl| tl.takes.iter().map(|t| t.guid.clone()))
                .collect();
            takes.sort();
            takes
        })
        .unwrap_or_default();
    assert_eq!(takes.len(), 2);
    (daw, guid, takes)
}

fn sources(daw: &Standalone, guid: &str, takes: &[String]) -> Vec<Arc<AudioSource>> {
    takes
        .iter()
        .map(|t| daw.audio_source(guid, t).expect("the take has a source"))
        .collect()
}

#[tokio::test]
async fn two_takes_of_one_wav_play_one_mapping() {
    let dir = tempfile::tempdir().unwrap();
    let (daw, guid, takes) = two_takes_of_one_file(&dir, &wav(4_800, 16));

    let got = sources(&daw, guid, &takes);

    assert!(Arc::ptr_eq(&got[0], &got[1]), "one source for one file");
    assert!(
        matches!(*got[0], AudioSource::PcmFile(_)),
        "mapped, not decoded"
    );
}

// A file the mapper refuses (8-bit PCM) decodes whole: once, charged once.
// nextest runs each test in a process of its own, so the budget's counter
// is this test's alone.
#[tokio::test]
async fn two_takes_of_one_decoded_file_hold_one_decode() {
    let dir = tempfile::tempdir().unwrap();
    let frames = 48_000;
    let before = fts_sample::budget::used_bytes();
    let (daw, guid, takes) = two_takes_of_one_file(&dir, &wav(frames, 8));

    let got = sources(&daw, guid, &takes);

    assert!(Arc::ptr_eq(&got[0], &got[1]), "one decode for one file");
    assert!(matches!(*got[0], AudioSource::Memory(_)));
    assert_eq!(
        fts_sample::budget::used_bytes() - before,
        (frames * std::mem::size_of::<f32>()) as u64,
        "one decode's worth charged"
    );
}

// A take whose source was never materialized (made by an edit, pointed at
// its file afterwards) loads when it is first read — mapped, and shared
// with the file's other takes, not decoded whole for each one.
#[tokio::test]
async fn takes_loaded_on_first_read_share_one_mapping() {
    let dir = tempfile::tempdir().unwrap();
    let (daw, guid, takes) = two_takes_of_one_file(&dir, &wav(4_800, 16));
    for take in &takes {
        detach_audio_source(&daw, guid, take);
    }
    let items: Vec<String> = daw
        .read_project(guid, |p| p.takes.keys().cloned().collect())
        .unwrap_or_default();

    for item in items {
        TakeReader::open_or_decode(
            &daw,
            ProjectContext::Project(guid.into()),
            ItemRef::Guid(item),
            TakeRef::Active,
        )
        .expect("the take reads");
    }
    let got = sources(&daw, guid, &takes);

    assert!(Arc::ptr_eq(&got[0], &got[1]), "one source for one file");
    assert!(
        matches!(*got[0], AudioSource::PcmFile(_)),
        "mapped, not decoded"
    );
}

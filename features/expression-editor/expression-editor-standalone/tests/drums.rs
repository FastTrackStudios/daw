//! The drum workspace load, headless.
//!
//! `--drums` opens a project's kit folder as one workspace: a track per
//! mic, each analysed as percussion, folded into role lanes. This is the
//! loading half of `r[drums.open.runner]` — no window, same reason as
//! `tests/load.rs`.

use std::path::{Path, PathBuf};

use dawfile_reaper::RppSerialize;
use dawfile_reaper::builder::ReaperProjectBuilder;
use dawfile_reaper::types::item::SourceType;
use expression_editor_core::kit::LaneRole;
use expression_editor_core::{Mode, Viewport};
use expression_editor_standalone::{Args, LoadError, Runner, Source, Target};

fn viewport() -> Viewport {
    Viewport::new(1100.0, 500.0)
}

/// A mono 16-bit PCM second of silence with a few sharp decaying clicks
/// in it — the least audio the envelope gate will call hits.
fn write_click_wav(path: &Path, rate: u32) {
    let frames = rate; // one second
    let mut samples = vec![0.0f64; frames as usize];
    for &at_secs in &[0.1, 0.4, 0.7] {
        let start = (at_secs * rate as f64) as usize;
        for i in 0..((rate / 100) as usize) {
            // Sharp attack, ~10 ms decay: struck, not swelled, which is
            // exactly what the gate's crest condition tests for.
            let t = i as f64 / rate as f64;
            if let Some(s) = samples.get_mut(start + i) {
                *s = 0.9 * (-t / 0.003).exp();
            }
        }
    }

    let data_len = frames * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &v in &samples {
        out.extend_from_slice(&((v * i16::MAX as f64) as i16).to_le_bytes());
    }
    std::fs::write(path, out).unwrap();
}

/// Three mics under a `Drums` folder: `Kick In` inside a `Kick`
/// sub-folder, `Snare Top` inside `Snare`, and `OH` directly under the
/// kit — the smallest project that exercises all three ways a track gets
/// its role (folder, folder, own name).
fn fixture(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let wavs: Vec<PathBuf> = ["kick.wav", "snare.wav", "oh.wav"]
        .iter()
        .map(|n| {
            let p = dir.join(n);
            write_click_wav(&p, 44_100);
            p
        })
        .collect();

    let rpp = ReaperProjectBuilder::new()
        .tempo(120.0)
        .track("Drums", |t| t.folder_start())
        .track("Kick", |t| t.folder_start())
        .track("Kick In", |t| {
            t.item(0.0, 1.0, |i| {
                i.take(wavs[0].to_string_lossy().into_owned(), SourceType::Wave)
            })
            .folder_end(1)
        })
        .track("Snare", |t| t.folder_start())
        .track("Snare Top", |t| {
            t.item(0.0, 1.0, |i| {
                i.take(wavs[1].to_string_lossy().into_owned(), SourceType::Wave)
            })
            .folder_end(1)
        })
        .track("OH", |t| {
            t.item(0.0, 1.0, |i| {
                i.take(wavs[2].to_string_lossy().into_owned(), SourceType::Wave)
            })
            .folder_end(1)
        })
        .build()
        .to_rpp_string();

    let path = dir.join("kit.rpp");
    std::fs::write(&path, rpp).unwrap();
    path
}

// r[verify drums.open.runner]
#[test]
fn a_kit_folder_opens_as_a_drum_workspace() {
    let dir = std::env::temp_dir().join("fts-ee-standalone-drums");
    let path = fixture(&dir);

    let runner = Runner::open(
        &Source::Rpp(path),
        &Target {
            drums: Some(None),
            ..Target::default()
        },
        viewport(),
        None,
    )
    .expect("the kit opens");

    assert_eq!(runner.loaded.kind(), "drums");
    assert!(runner.daw.is_some(), "the backend outlives the load");
    assert!(
        runner.label.contains("drums: Drums (3 mics)"),
        "got {:?}",
        runner.label
    );

    let editor = runner.loaded.editor();
    assert!(editor.stacked, "a drum workspace opens on the stack");
    assert_eq!(editor.tracks.len(), 3, "one track per mic, no folders");
    assert_eq!(editor.mode, Mode::UnpitchedAudio);

    // Role lanes, top to bottom Other / Snare / Kick — kick at the
    // bottom, and no Toms lane because the kit has no toms.
    let roles: Vec<Option<LaneRole>> = editor
        .tracks
        .layout()
        .lanes()
        .iter()
        .map(|l| l.role)
        .collect();
    assert_eq!(
        roles,
        vec![
            Some(LaneRole::Other),
            Some(LaneRole::Snare),
            Some(LaneRole::Kick)
        ]
    );

    // Every mic analysed: unpitched, hits found, waveform behind them.
    for (i, track) in editor.tracks.tracks().iter().enumerate() {
        assert_eq!(track.mode, Mode::UnpitchedAudio, "{}", track.name);
        let doc = if i == editor.tracks.active() {
            &editor.doc
        } else {
            editor.tracks.doc_of(i).expect("parked doc")
        };
        assert!(!doc.notes.is_empty(), "no hits detected on {}", track.name);
        assert!(!doc.peaks.is_empty(), "no peaks on {}", track.name);
    }
}

// r[verify drums.open.runner]
#[test]
fn a_named_kit_folder_is_found_case_insensitively() {
    let dir = std::env::temp_dir().join("fts-ee-standalone-drums-named");
    let path = fixture(&dir);
    let runner = Runner::open(
        &Source::Rpp(path),
        &Target {
            drums: Some(Some("drums".into())),
            ..Target::default()
        },
        viewport(),
        None,
    )
    .expect("the named folder opens");
    assert_eq!(runner.loaded.editor().tracks.len(), 3);
}

// r[verify drums.open.runner]
#[test]
fn a_project_with_no_kit_folder_says_so() {
    let rpp = ReaperProjectBuilder::new()
        .track("Vox", |t| {
            t.item(0.0, 1.0, |i| i.take("vox", SourceType::Midi))
        })
        .build()
        .to_rpp_string();
    let dir = std::env::temp_dir().join("fts-ee-standalone-drums-none");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("no-kit.rpp");
    std::fs::write(&path, rpp).unwrap();

    let err = Runner::open(
        &Source::Rpp(path),
        &Target {
            drums: Some(None),
            ..Target::default()
        },
        viewport(),
        None,
    )
    .expect_err("nothing to open as a kit");
    assert!(matches!(err, LoadError::NoKitFolder { .. }), "got {err:?}");
}

// r[verify drums.open.runner]
#[test]
fn the_drums_flag_parses_with_and_without_a_folder() {
    let bare = Args::parse(["song.rpp", "--drums"].map(String::from)).unwrap();
    assert_eq!(bare.target.drums, Some(None));

    let named = Args::parse(["song.rpp", "--drums", "The Kit"].map(String::from)).unwrap();
    assert_eq!(named.target.drums, Some(Some("The Kit".into())));

    // Before the source, a following token is NOT eaten as the folder —
    // it is the source.
    let ordered = Args::parse(["--drums", "song.rpp"].map(String::from)).unwrap();
    assert_eq!(ordered.target.drums, Some(None));
    assert!(matches!(ordered.source, Source::Rpp(_)));
}

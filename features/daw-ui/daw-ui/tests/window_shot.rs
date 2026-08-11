//! The main window, rasterised — REAPER's whole shape in one picture.
//!
//! `cargo test -p daw-ui --test window_shot -- --nocapture` writes
//! `target/theme-shots/main-window.png`: transport across the top, TCP
//! rows beside the arrangement with its ruler, grid and items, mixer
//! strips docked underneath. The same blitz-dom pipeline as `strip_shot`,
//! so the picture is what REAPER's renderer would draw.

use std::collections::HashMap;

use daw_proto::{Item, Track};
use daw_ui::components::arrangement_view::{ItemPreview, NotePreview};
use daw_ui::components::main_window::MainWindowPreview;
use daw_ui::components::media_browser::{MediaEntry, MediaKind};
use daw_ui::controls::TrackStore;
use dioxus::prelude::*;

const BODY_MARGIN: u32 = 8;
const W: f32 = 1024.0;
const H: f32 = 768.0;

fn tracks() -> Vec<Track> {
    let t = |guid: &str, name: &str, colour: u32, index: u32| Track {
        guid: guid.into(),
        name: name.into(),
        color: Some(colour),
        index,
        volume: 1.0,
        record_input: daw_proto::track::RecordInput::Audio { channel: index },
        ..Default::default()
    };
    vec![
        Track { armed: true, ..t("kick", "Kick", 0xe0_56_7a, 0) },
        Track { soloed: true, ..t("snare", "Snare", 0xe0_56_7a, 1) },
        t("oh", "OH", 0xe0_56_7a, 2),
        Track { muted: true, ..t("bass", "Bass", 0x55_88_e0, 3) },
        t("gtr", "Gtr", 0x42_c8_e0, 4),
        Track { fx_count: 2, ..t("keys", "Keys", 0xe0_a8_42, 5) },
    ]
}

/// Items in bars at 120 BPM 4/4 — one bar is two seconds.
fn items() -> Vec<Item> {
    let item = |guid: &str, track: &str, bar_from: f64, bars: f64, label: &str| Item {
        guid: guid.into(),
        track_guid: track.into(),
        position: daw_proto::primitives::PositionInSeconds::from_seconds(bar_from * 2.0),
        length: daw_proto::primitives::Duration::from_seconds(bars * 2.0),
        label: (!label.is_empty()).then(|| label.to_string()),
        ..Default::default()
    };
    vec![
        item("i1", "kick", 0.0, 8.0, "Kick"),
        item("i2", "snare", 1.0, 7.0, "Snare"),
        item("i3", "oh", 0.0, 8.0, "OH"),
        item("i4", "bass", 0.0, 4.0, "Bass A"),
        Item { muted: true, ..item("i5", "bass", 4.0, 4.0, "Bass B") },
        item("i6", "gtr", 2.0, 3.0, "Gtr"),
        Item { selected: true, ..item("i7", "gtr", 5.0, 3.0, "Gtr dbl") },
        item("i8", "keys", 4.0, 4.0, "Keys"),
    ]
}

/// Synthetic content previews: a few beats of drum-like transients, a
/// sustained pad, and a MIDI riff — enough to show the waveform and note
/// renderers doing their jobs in one picture.
fn previews() -> HashMap<String, ItemPreview> {
    // A transient every half bar, decaying — reads as drums.
    let hits = |blocks: usize, period: usize| -> Vec<f32> {
        (0..blocks)
            .map(|i| {
                let phase = (i % period) as f32 / period as f32;
                (1.0 - phase).powi(3) * 0.9 + 0.05
            })
            .collect()
    };
    // A slow swell — reads as a pad or bass.
    let swell = |blocks: usize| -> Vec<f32> {
        (0..blocks)
            .map(|i| {
                let x = i as f32 / blocks as f32 * std::f32::consts::PI;
                x.sin() * 0.7 + 0.1
            })
            .collect()
    };
    let riff = vec![
        NotePreview { pitch: 60, start: 0.0, length: 0.9 },
        NotePreview { pitch: 64, start: 1.0, length: 0.9 },
        NotePreview { pitch: 67, start: 2.0, length: 0.9 },
        NotePreview { pitch: 72, start: 3.0, length: 0.9 },
        NotePreview { pitch: 67, start: 4.0, length: 1.9 },
        NotePreview { pitch: 64, start: 6.0, length: 1.9 },
    ];

    HashMap::from([
        ("i1".to_string(), ItemPreview::Waveform(hits(512, 32))),
        ("i2".to_string(), ItemPreview::Waveform(hits(448, 64))),
        ("i3".to_string(), ItemPreview::Waveform(swell(512))),
        ("i4".to_string(), ItemPreview::Waveform(swell(256))),
        ("i5".to_string(), ItemPreview::Waveform(swell(256))),
        ("i6".to_string(), ItemPreview::Waveform(hits(192, 48))),
        ("i7".to_string(), ItemPreview::Waveform(hits(192, 48))),
        ("i8".to_string(), ItemPreview::Notes(riff)),
    ])
}

/// The browser's shelf: pack-style names carrying their BPM and key, so
/// the chips exercise the filename parser, with a MIDI file among the
/// audio and one entry still waiting on its preview.
fn media() -> Vec<MediaEntry> {
    let wave = |n: usize, seed: u32| -> Vec<f32> {
        (0..n)
            .map(|i| {
                let x = i as f32 * 0.13 + seed as f32;
                (x.sin() * (x * 0.31).cos()).abs() * 0.8 + 0.08
            })
            .collect()
    };
    let audio = |name: &str, folder: &str, dur: f32, seed: u32| MediaEntry {
        name: name.into(),
        folder: folder.into(),
        kind: MediaKind::Audio,
        duration: dur,
        preview: Some(ItemPreview::Waveform(wave(160, seed))),
    };
    let riff = vec![
        NotePreview { pitch: 57, start: 0.0, length: 0.4 },
        NotePreview { pitch: 60, start: 0.5, length: 0.4 },
        NotePreview { pitch: 64, start: 1.0, length: 0.4 },
        NotePreview { pitch: 65, start: 1.5, length: 0.4 },
        NotePreview { pitch: 64, start: 2.0, length: 0.9 },
        NotePreview { pitch: 60, start: 3.0, length: 0.9 },
    ];
    vec![
        audio("Am_140_groove.wav", "Loops", 6.9, 3),
        audio("kick_punchy.wav", "Drums", 0.8, 7),
        MediaEntry {
            name: "F#MIN 92bpm [SHARK].mid".into(),
            folder: "MIDI".into(),
            kind: MediaKind::Midi,
            duration: 4.0,
            preview: Some(ItemPreview::Notes(riff)),
        },
        audio("D 175bpm break.wav", "Loops", 5.5, 11),
        audio("vocal_chop_Em_128.wav", "Vocals", 3.7, 17),
        MediaEntry {
            name: "chords_Cmaj_pad.mid".into(),
            folder: "MIDI".into(),
            kind: MediaKind::Midi,
            duration: 8.0,
            preview: None,
        },
        audio("snare_tight.wav", "Drums", 0.5, 23),
    ]
}

#[test]
fn paint_the_main_window() {
    fn app() -> Element {
        let mut store = use_hook(TrackStore::new);
        use_hook(|| {
            store.seed(tracks());
            provide_context(store);
        });
        rsx! {
            div {
                style: "position:absolute; left:0; top:0;",
                MainWindowPreview {
                    tracks: tracks(),
                    items: items(),
                    previews: previews(),
                    media: media(),
                    media_selected: Some("F#MIN 92bpm [SHARK].mid".to_string()),
                }
            }
        }
    }

    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/theme-shots");
    std::fs::create_dir_all(&out).unwrap();
    let path = out.join("main-window.png");
    dioxus_test::render(app)
        .with_window_size(W as u32 + BODY_MARGIN * 2, H as u32 + BODY_MARGIN * 2)
        .build()
        .render_png(&path);
    crop(&path, W as u32, H as u32);
    println!("wrote {}", path.display());
}

fn crop(path: &std::path::Path, width: u32, height: u32) {
    let status = std::process::Command::new("magick")
        .arg(path)
        .arg("-crop")
        .arg(format!("{width}x{height}+{BODY_MARGIN}+{BODY_MARGIN}"))
        .arg("+repage")
        .arg(path)
        .status();
    match status {
        Ok(s) if s.success() => {}
        _ => eprintln!("note: `magick` unavailable, {} keeps its margin", path.display()),
    }
}

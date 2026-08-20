//! The workstation window: the whole project, not just the editor.
//!
//! ```text
//! +--------------------+-------+
//! | TCP | Arrangement  | Mixer |
//! +--------------------+ (MCP) |
//! | Expression editor  |       |
//! +--------------------+-------+
//! ```
//!
//! Left column: daw-ui's `ArrangeView` (its `tcp_width` is the TCP) over
//! the expression editor; right column: the `MixerControlPanel`, full
//! height. Everything under one `ThemeProvider`.
//!
//! The panels are daw-ui's **view-model family** (`daw_ui::panels`):
//! Blitz-safe inline styles, fed `TrackView`s this module builds from
//! the in-process daw facade — the same `Standalone` the drum host
//! edits, served over a vox memory link by
//! [`daw::standalone::bootstrap::build_in_process_daw`]. One backend,
//! three faces: the arrange view shows the items the quantizer cuts,
//! the mixer moves the faders the renderer reads, and the editor writes
//! the edits — all visibly the same project.

use std::sync::{Arc, Mutex, OnceLock};

use dioxus::prelude::*;

use daw::service::Track;
use daw::standalone::Standalone;
use daw_ui::panels::{
    ArrangeEdit, ArrangeView, ClipView, MarkerView, MixerControlPanel, RegionView, TrackView,
};
use daw_ui::theming::{ThemeContext, ThemeProvider};
use expression_editor_core::Editor;
use expression_editor_ui::ExpressionEditor;

use crate::app::{HostCallbacks, host_callbacks};
use crate::drum_host::SharedDrumHost;

/// The mixer column's width. A constant for now; a draggable divider is
/// chrome the window can grow later.
pub const MIXER_W: f64 = 360.0;
/// The arrange pane's share of the left column.
pub const ARRANGE_FRACTION: f64 = 0.45;
/// The arrange view's TCP width.
pub const TCP_W: u32 = 300;

/// What the window mounts, staged like [`crate::app::stage`] and for
/// the same reason: `dioxus_native::launch_cfg` takes a bare
/// `fn() -> Element`.
struct StagedWorkstation {
    editor: Editor,
    host: Option<SharedDrumHost>,
    /// Window size, so the editor's cell can be reported before the
    /// first layout (dioxus-native fires no resize on mount).
    size: (f64, f64),
}

static STAGED: Mutex<Option<StagedWorkstation>> = Mutex::new(None);

/// Keeps the in-process daw link's acceptor alive for the window's
/// lifetime. Dropping it would silently disconnect every panel.
static DAW_BUNDLE: OnceLock<daw::standalone::bootstrap::InProcessDaw> = OnceLock::new();

/// Hand the workstation its document, host and window size.
pub fn stage_workstation(editor: Editor, host: Option<SharedDrumHost>, size: (f64, f64)) {
    *STAGED.lock().unwrap() = Some(StagedWorkstation { editor, host, size });
}

/// Serve `standalone` over an in-process memory link and install the
/// global daw facade the panels read. Call once, before the window.
///
/// The runtime is leaked multi-thread with 16 MiB worker stacks —
/// vox 0.10's debug-build channel encode recurses deeply and overflows
/// tokio's default 2 MiB (the same story as the app's session engine).
pub fn bootstrap_daw_blocking(standalone: &Standalone) -> eyre::Result<()> {
    if DAW_BUNDLE.get().is_some() {
        return Ok(());
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("ee-workstation-daw")
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()?;
    let rt: &'static tokio::runtime::Runtime = Box::leak(Box::new(rt));
    let bundle = rt.block_on(daw::standalone::bootstrap::build_in_process_daw(
        standalone.clone(),
    ))?;
    // A separate current-thread runtime for `daw::block_on` in sync
    // contexts, so it can never be entered from inside the engine
    // runtime's own workers.
    let block_on_rt = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?,
    );
    daw::init_from_parts(bundle.daw.clone(), block_on_rt);
    DAW_BUNDLE
        .set(bundle)
        .map_err(|_| eyre::eyre!("workstation daw bootstrapped twice"))?;
    Ok(())
}

/// One themed `TrackView` from a daw `Track` + its clips.
fn track_view(id: usize, t: &Track, clips: Vec<ClipView>) -> TrackView {
    let hex = t.color.map(|c| format!("#{:06x}", c & 0x00FF_FFFF));
    let mut tv = TrackView::new(id, t.name.clone(), hex.as_deref())
        .fader(t.volume as f32)
        .depth(t.folder_depth.max(0) as u32)
        .clips(clips);
    if t.is_folder {
        tv = tv.folder();
    }
    *tv.mute.write() = t.muted;
    *tv.solo.write() = t.soloed;
    *tv.pan.write() = (0.5 + t.pan as f32 / 2.0).clamp(0.0, 1.0);
    tv
}

/// `(max, min)` pairs for a clip, from the take's first channel.
///
/// The standalone backend serves real peaks now (r[drums.open.peaks]),
/// interleaved `[ch0_min, ch0_max, ch1_min, …]` per block.
fn clip_peaks(data: &daw::service::TakePeakData) -> Vec<(f32, f32)> {
    let stride = (data.num_channels.max(1) as usize) * 2;
    data.peaks
        .chunks(stride)
        .filter(|b| b.len() >= 2)
        .map(|b| (b[1] as f32, b[0] as f32))
        .collect()
}

/// Everything the arrange + mixer need, fetched in one pass.
#[derive(Default, Clone)]
struct ProjectShape {
    guid: String,
    entries: Vec<(String, TrackView)>,
    regions: Vec<RegionView>,
    markers: Vec<MarkerView>,
    bpm: f64,
    /// End of the last item, so the timeline spans the material.
    length_secs: f64,
}

async fn fetch_project() -> Option<ProjectShape> {
    let daw = daw::get()?;
    let project = daw.current_project().await.ok()?;
    let tracks = project.tracks().all().await.ok()?;
    let items = project.items().all().await.ok()?;

    let mut length = 0.0f64;
    let mut entries = Vec::with_capacity(tracks.len());
    for (i, t) in tracks.iter().enumerate() {
        let mut clips = Vec::new();
        for item in items.iter().filter(|i| i.track_guid == t.guid) {
            let start = item.position.as_seconds();
            let len = item.length.as_seconds();
            length = length.max(start + len);
            // ~12 peaks a second at 48k: the arrange draws whole songs,
            // not sample detail; the editor below has the close view.
            let peaks = match project.items().by_guid(&item.guid).await {
                Ok(Some(h)) => h
                    .active_take()
                    .peaks(4096)
                    .await
                    .map(|d| clip_peaks(&d))
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            clips.push(ClipView {
                start,
                length: len,
                name: String::new(),
                color: item.color.map(|c| format!("#{:06x}", c & 0x00FF_FFFF)),
                peaks,
                peaks_right: Vec::new(),
                fade_in: item.fade_in_length.as_seconds(),
                fade_out: item.fade_out_length.as_seconds(),
                selected: false,
                muted: item.muted,
                lane: item.fixed_lane,
            });
        }
        entries.push((t.guid.clone(), track_view(i, t, clips)));
    }

    let regions = project
        .regions()
        .all()
        .await
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, r)| RegionView {
            start: r.time_range.start_seconds(),
            end: r.time_range.end_seconds(),
            name: r.name.clone(),
            color: r.color.map(|c| format!("#{c:06x}")),
            idx: r.id.unwrap_or(i as u32),
        })
        .collect();
    let markers = project
        .markers()
        .all()
        .await
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, m)| MarkerView {
            time: m.position.seconds().unwrap_or_default(),
            name: m.name.clone(),
            color: m.color.map(|c| format!("#{c:06x}")),
            idx: m.id.unwrap_or(i as u32),
        })
        .collect();
    let bpm = project.transport().get_tempo().await.unwrap_or(120.0);

    Some(ProjectShape {
        guid: project.guid().to_string(),
        entries,
        regions,
        markers,
        bpm,
        length_secs: length.max(60.0),
    })
}

/// The workstation. Takes no props — see [`stage_workstation`].
#[component]
pub fn WorkstationApp() -> Element {
    let staged = use_hook(|| {
        Arc::new(
            STAGED
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| StagedWorkstation {
                    editor: crate::app::fallback(),
                    host: None,
                    size: (1920.0, 1080.0),
                }),
        )
    });
    let (win_w, win_h) = staged.size;
    let arrange_h = ((win_h) * ARRANGE_FRACTION).round();
    let left_w = (win_w - MIXER_W).max(200.0);
    // The editor's cell is everything under the arrange pane. It
    // subtracts its own chrome from what we report here.
    expression_editor_ui::available_space(left_w, win_h - arrange_h);

    let editor = use_signal(|| {
        let mut ed = staged.editor.clone();
        ed.viewport = expression_editor_ui::viewport_in(left_w, win_h - arrange_h);
        ed
    });
    let host = use_signal(|| staged.host.clone());
    let bins = use_signal(Vec::new);
    let previews = use_signal(Vec::new);
    let HostCallbacks {
        on_change,
        on_apply,
        on_save,
        on_hit,
    } = host_callbacks(editor, host.read().clone(), bins, previews);

    // The project, fetched once through the facade. A refetch key can
    // arrive later (an item-stream subscription); the drum host already
    // refreshes the editor's own lanes after every write.
    let mut shape = use_signal(ProjectShape::default);
    use_effect(move || {
        spawn(async move {
            if let Some(s) = fetch_project().await {
                shape.set(s);
            }
        });
    });

    // The playhead, driven by the backend's ~30 Hz position ticks.
    let mut playhead = use_signal(|| 0.0f64);
    use_future(move || async move {
        loop {
            let Some(daw) = daw::get() else {
                futures_timer::Delay::new(std::time::Duration::from_millis(250)).await;
                continue;
            };
            let Ok(project) = daw.current_project().await else {
                futures_timer::Delay::new(std::time::Duration::from_millis(250)).await;
                continue;
            };
            let guid = project.guid().to_string();
            let mut stream = project.transport().events();
            while let Ok(Some(ev)) = stream.recv().await {
                if let daw::service::transport::TransportStreamEvent::Position(tick) = ev.get()
                    && tick.project_guid == guid
                {
                    if let Some(s) = tick.playhead.seconds() {
                        playhead.set(s);
                    }
                }
            }
            futures_timer::Delay::new(std::time::Duration::from_millis(500)).await;
        }
    });

    // Meters for the mixer: one subscription for every strip.
    {
        let entries_sig = shape;
        use_future(move || async move {
            loop {
                let Some(daw) = daw::get() else {
                    futures_timer::Delay::new(std::time::Duration::from_millis(250)).await;
                    continue;
                };
                let mut stream = daw.meter_events();
                while let Ok(Some(frame)) = stream.recv().await {
                    let frame = frame.get();
                    let s = entries_sig.peek();
                    if frame.project_guid != s.guid {
                        continue;
                    }
                    type MeterSigs = Vec<(usize, Signal<f32>, Signal<f32>, Signal<f32>)>;
                    let sigs: MeterSigs = s
                        .entries
                        .iter()
                        .enumerate()
                        .map(|(i, (_, tv))| (i, tv.level, tv.level_right, tv.peak))
                        .collect();
                    drop(s);
                    for (i, mut level, mut right, mut peak) in sigs {
                        if let Some(t) = frame.tracks.get(i) {
                            level.set(t.peak_left.clamp(0.0, 1.0));
                            right.set(t.peak_right.clamp(0.0, 1.0));
                            peak.set(t.hold_left.max(t.hold_right).clamp(0.0, 1.0));
                        }
                    }
                }
                futures_timer::Delay::new(std::time::Duration::from_millis(500)).await;
            }
        });
    }

    let on_edit = EventHandler::new(move |edit: ArrangeEdit| {
        if let ArrangeEdit::Seek(t) = edit {
            playhead.set(t);
            spawn(async move {
                if let Some(daw) = daw::get()
                    && let Ok(project) = daw.current_project().await
                {
                    let _ = project.transport().set_position(t).await;
                }
            });
        }
    });

    let s = shape.read();
    let tracks: Vec<TrackView> = s.entries.iter().map(|(_, tv)| tv.clone()).collect();
    let entries_now = s.entries.clone();
    let regions = s.regions.clone();
    let markers = s.markers.clone();
    let bpm = s.bpm;
    let seconds = s.length_secs;
    drop(s);
    // Fit the whole song into the arrange pane's width.
    let pps = ((left_w - TCP_W as f64) / seconds.max(1.0)).max(1.0);

    rsx! {
        style {
            "html, body {{ width: 100%; height: 100%; margin: 0; padding: 0; \
              overflow: hidden; background: #101016; }}"
        }
        ThemeProvider {
            theme: ThemeContext::new(),
            div {
                style: "display: flex; flex-direction: row; width: 100vw; height: 100vh; \
                        min-height: 0; min-width: 0;",
                // Left column: arrange over the editor.
                div {
                    style: "flex: 1 1 auto; min-width: 0; display: flex; \
                            flex-direction: column; min-height: 0;",
                    div {
                        style: "flex: 0 0 {arrange_h}px; min-height: 0; overflow: hidden;",
                        "data-testid": "workstation-arrange",
                        if tracks.is_empty() {
                            div {
                                style: "padding: 16px; color: #7b7b7b; font-size: 12px;",
                                "Loading project…"
                            }
                        } else {
                            ArrangeView {
                                tracks: tracks.clone(),
                                pps,
                                tcp_width: TCP_W,
                                seconds,
                                playhead,
                                markers,
                                regions,
                                bpm: Some(bpm),
                                on_edit,
                            }
                        }
                    }
                    div {
                        style: "flex: 1 1 auto; min-height: 0;",
                        "data-testid": "workstation-editor",
                        ExpressionEditor {
                            editor,
                            quantize_bins: bins(),
                            quantize_previews: previews(),
                            on_quantize_change: on_change,
                            on_quantize_apply: on_apply,
                            on_hit,
                            on_save,
                        }
                    }
                }
                // Right column: the mixer, full height. Width twice —
                // Blitz collapses a flex-basis column whose content is
                // still loading, and a mixer that appears out of
                // nowhere would reflow both panes to its left.
                div {
                    style: "flex: 0 0 {MIXER_W}px; width: {MIXER_W}px; min-height: 0; \
                            overflow: hidden; border-left: 1px solid #323232; \
                            background: #131319;",
                    "data-testid": "workstation-mixer",
                    if tracks.is_empty() {
                        div {
                            style: "padding: 16px; color: #7b7b7b; font-size: 12px;",
                            "Mixer — loading…"
                        }
                    } else {
                        MixerControlPanel { tracks }
                    }
                }
                // Invisible engine-sync siblings, one per strip.
                div {
                    style: "display: none;",
                    for (guid, tv) in entries_now.iter() {
                        TrackSync { key: "{guid}", guid: guid.clone(), tv: tv.clone() }
                    }
                }
            }
        }
    }
}

/// Push a strip's mute/solo/fader/pan intents to the engine, by GUID.
/// The MCP stays a pure view; this sibling is where UI becomes state.
#[component]
fn TrackSync(guid: String, tv: TrackView) -> Element {
    let mute = tv.mute;
    let solo = tv.solo;
    let fader = tv.fader;
    let pan = tv.pan;

    async fn with_track<F, Fut>(guid: String, op: F)
    where
        F: FnOnce(daw::rpc::TrackHandle) -> Fut,
        Fut: std::future::Future<Output = Result<(), daw::rpc::Error>>,
    {
        if let Some(daw) = daw::get()
            && let Ok(project) = daw.current_project().await
            && let Ok(Some(handle)) = project.tracks().by_guid(&guid).await
        {
            let _ = op(handle).await;
        }
    }

    {
        let guid = guid.clone();
        use_effect(move || {
            let muted = *mute.read();
            let guid = guid.clone();
            spawn(with_track(guid, move |h| async move {
                if muted {
                    h.mute().await
                } else {
                    h.unmute().await
                }
            }));
        });
    }
    {
        let guid = guid.clone();
        use_effect(move || {
            let soloed = *solo.read();
            let guid = guid.clone();
            spawn(with_track(guid, move |h| async move {
                if soloed {
                    h.solo().await
                } else {
                    h.unsolo().await
                }
            }));
        });
    }
    {
        let guid = guid.clone();
        use_effect(move || {
            let vol = *fader.read() as f64;
            let guid = guid.clone();
            spawn(with_track(
                guid,
                move |h| async move { h.set_volume(vol).await },
            ));
        });
    }
    {
        let guid = guid.clone();
        use_effect(move || {
            let pan_val = (*pan.read() as f64) * 2.0 - 1.0;
            let guid = guid.clone();
            spawn(with_track(guid, move |h| async move {
                h.set_pan(pan_val).await
            }));
        });
    }
    rsx! {}
}

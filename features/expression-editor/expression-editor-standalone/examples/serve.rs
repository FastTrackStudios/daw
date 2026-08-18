//! The expression editor as a desktop app, served.
//!
//! ```sh
//! just ee-serve
//! # = dx serve -p expression-editor-standalone --example serve \
//! #       --platform desktop --renderer native
//! ```
//!
//! The window comes from `dioxus_native::launch_cfg` — Blitz → Vello →
//! winit, which is dioxus's own documented desktop path and what
//! `eq-standalone` already uses. Not `nice-plug-dioxus`: that opens a
//! *plugin editor* window through baseview, which is the right host for
//! a VST3/CLAP editor and the wrong one for an application that is not a
//! plugin. The document model and the components are identical either
//! way — only the windowing differs — so this keeps rendering parity
//! with the plugin without pretending to be one.
//!
//! Not `dioxus::desktop` either: that is WebKit/WRY, which would render
//! the surface through a completely different engine from the one the
//! plugin and the REAPER panel use.
//!
//! What makes it *served* rather than one-shot is that `dx` watches the
//! tree and hot-reloads `rsx!` edits into the running window, and the
//! file being edited is chosen in the app rather than on the command
//! line. Point it at material with `EXPRESSION_EDITOR_LIBRARY`; it scans
//! the working directory otherwise.

use daw::standalone::Standalone;
use dioxus::prelude::*;
use dioxus_native::{Config, LogicalSize, WindowAttributes, launch_cfg};
use expression_editor_core::Viewport;
use expression_editor_standalone::cli::ArgsError;
use expression_editor_standalone::library::Entry;
use expression_editor_standalone::{Args, Runner, Target, library, stage};
use expression_editor_ui::{ExpressionEditor, theme};

fn main() {
    // Arguments are optional here, unlike `--example editor`: the whole
    // point is that the window can open on nothing and be told what to
    // load afterwards. A bad argument is still worth reporting rather
    // than silently ignoring.
    let args = match Args::from_env() {
        Ok(a) => Some(a),
        Err(e @ (ArgsError::Help | ArgsError::List)) => {
            print!("{e}");
            return;
        }
        Err(e) => {
            eprintln!("{e}\n\nopening empty — pick a file in the window");
            None
        }
    };

    let (width, height) = args
        .as_ref()
        .map(|a| (a.width, a.height))
        .unwrap_or((1200, 760));

    // Staging is the same hand-off `App` uses, so an argument opens the
    // window on that document and the chooser takes over from there.
    let mut title = String::from("Expression editor");
    let mut daw = None;
    if let Some(args) = &args {
        match Runner::open(&args.source, &args.target, args.viewport(), args.mode) {
            Ok(runner) => {
                println!(
                    "{} — {} notes, mode {}",
                    runner.label,
                    runner.loaded.editor().doc.notes.len(),
                    runner.loaded.editor().mode.label()
                );
                title = format!("Expression editor — {}", runner.label);
                daw = runner.daw;
                stage(runner.loaded.into_editor());
            }
            Err(e) => eprintln!("{e}\n\nopening empty — pick a file in the window"),
        }
    }

    let root = library::root();
    println!(
        "library: {} ({} openable)",
        root.display(),
        library::scan(&root).len()
    );

    // Outlives the window for the same reason `--example editor` holds
    // it: the session's location is only meaningful against the backend
    // it was read from.
    let _daw = daw;

    let window = WindowAttributes::default()
        .with_title(title)
        .with_surface_size(LogicalSize::new(width as f64, height as f64));
    launch_cfg(
        DevApp,
        vec![],
        vec![Box::new(Config::new().with_window_attributes(window))],
    );
}


// ── the runner's root component ─────────────────────────────────────


/// Height of the chooser strip. Fixed, so the editor below it gets a
/// stable viewport — the canvas fits its content to the space it is
/// given, and a bar that changed height would re-fit on every load.
const BAR: f64 = 34.0;

/// Height of the status line under the chooser.
const STATUS: f64 = 18.0;


/// Hand the editor the room left under the chooser and status line.
///
/// The runner subtracts only its *own* chrome. What the editor draws
/// around the roll is the editor's business, and `sizing::Chrome`
/// accounts for it — a host that tried to subtract the toolbar too would
/// be the second source of truth all over again.
fn report_space(window_w: f64, window_h: f64) {
    expression_editor_ui::available_space(window_w, (window_h - BAR - STATUS).max(1.0));
}

/// The runner's window: a chooser, and the editor under it.
#[component]
pub fn DevApp() -> Element {
    // The staged document, exactly as `App` takes it, so `dx serve` with
    // a file argument still opens on that file.
    let editor = use_signal(|| {
        expression_editor_standalone::app::take_staged().unwrap_or_else(|| {
            expression_editor_ui::demo::editor(expression_editor_ui::demo::Scene::Phrase, viewport())
        })
    });

    // The backend behind the current document. Held for as long as the
    // document is: a session's location is only meaningful against the
    // `Standalone` it was read from, so dropping this on the next load
    // would strand any write-back. Replaced, not accumulated.
    let mut backend = use_signal(|| None::<Standalone>);

    let entries = use_signal(|| {
        let mut all = library::scenes();
        all.extend(library::scan(&library::root()));
        all
    });
    let mut status = use_signal(|| String::from("ready"));
    let mut current = use_signal(|| String::new());

    // The editor measures its canvas when told to, because
    // dioxus-native delivers no element resize event. winit does report
    // the *window*, so the runner is what turns a drag of the window
    // edge into "your space changed" — the desktop equivalent of the
    // REAPER panel's dock callback.
    let window = dioxus_native::use_window();

    // The size we already have, at mount. `SurfaceResized` fires when
    // the size *changes*, so a window that opens at its final size and
    // is never dragged may never report one — and the editor would keep
    // the viewport its document was built with forever.
    {
        let window = window.clone();
        use_hook(move || {
            let scale = window.scale_factor();
            let size = window.surface_size();
            report_space(size.width as f64 / scale, size.height as f64 / scale);
        });
    }

    dioxus_native::use_window_event(move |event, _| {
        if let dioxus_native::winit::event::WindowEvent::SurfaceResized(size) = event {
            // Physical pixels from winit, CSS pixels for the editor.
            let scale = window.scale_factor();
            report_space(size.width as f64 / scale, size.height as f64 / scale);
        }
    });

    let mut open = move |entry: Entry| {
        let source = match entry.source() {
            Ok(s) => s,
            Err(e) => {
                status.set(format!("{}: {e}", entry.label));
                return;
            }
        };
        match Runner::open(&source, &Target::default(), viewport(), None) {
            Ok(runner) => {
                let notes = runner.loaded.editor().doc.notes.len();
                let mode = runner.loaded.editor().mode.label();
                status.set(format!("{} — {notes} notes, {mode}", runner.label));
                current.set(entry.arg.clone());
                backend.set(runner.daw);
                editor.clone().set(runner.loaded.into_editor());
            }
            // A file that will not open is the single most likely thing
            // to happen here, and it must not take the window with it —
            // the point of the runner is to still be up afterwards.
            Err(e) => status.set(format!("{}: {e}", entry.label)),
        }
    };

    rsx! {
        style {
            "html, body {{ width: 100%; height: 100%; margin: 0; padding: 0; \
              overflow: hidden; background: {theme::BG}; }}"
        }
        div {
            style: "width: 100vw; height: 100vh; display: flex; flex-direction: column;",

            // ── the chooser ──────────────────────────────────────────
            div {
                style: format!(
                    "height: {BAR}px; flex: none; display: flex; align-items: center; \
                     gap: 6px; padding: 0 8px; overflow-x: auto; \
                     background: {}; border-bottom: 1px solid {};",
                    theme::SURFACE_INSET,
                    theme::PANEL_BORDER,
                ),
                for entry in entries().into_iter() {
                    button {
                        key: "{entry.arg}",
                        "data-testid": "open-{entry.arg}",
                        title: "{entry.arg}",
                        style: format!(
                            "flex: none; height: 22px; padding: 0 8px; font-size: 10px; \
                             border-radius: 4px; cursor: pointer; white-space: nowrap; \
                             border: 1px solid {}; background: {}; color: {};",
                            if current() == entry.arg { theme::ACCENT } else { theme::PANEL_BORDER },
                            if current() == entry.arg { theme::CONTROL_ACTIVE } else { theme::SURFACE_INSET },
                            theme::TEXT,
                        ),
                        onclick: {
                            let entry = entry.clone();
                            move |_| open(entry.clone())
                        },
                        "{entry.kind.label()} · {entry.label}"
                    }
                }
            }

            // ── what just happened ───────────────────────────────────
            // The runner's stdout is not visible under `dx serve`, so
            // the line the `--example editor` path prints has to be on
            // screen instead.
            div {
                "data-testid": "status",
                style: format!(
                    "height: 18px; flex: none; padding: 0 10px; font-size: 10px; \
                     line-height: 18px; color: {}; background: {};",
                    theme::TEXT_DIM,
                    theme::SURFACE_INSET,
                ),
                "{status()}"
            }

            div {
                style: "flex: 1; min-height: 0;",
                ExpressionEditor { editor }
            }
        }
    }
}

/// The viewport a freshly loaded document starts at.
///
/// The *current* window, not the opening one. This used to be a constant,
/// so every scene or file opened reset the surface to 1200x760 however
/// wide the window had been dragged — the roll snapped back to its
/// opening aspect on each load.
fn viewport() -> Viewport {
    expression_editor_ui::current_viewport(expression_editor_ui::viewport_in(
        1200.0,
        760.0 - BAR - STATUS,
    ))
}

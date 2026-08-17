//! The runner you leave open — `dx serve`'s root component.
//!
//! [`app::App`](crate::app::App) is deliberately chrome-free: it is the
//! component the screenshot harness paints and the shape the REAPER
//! panel and a plugin editor mount, so anything added around the editor
//! there would show up in all three. This is the other root — the one
//! for a window you keep up for an hour while you change the UI under
//! it — and the only thing it adds is the ability to choose what is
//! loaded without restarting the process.
//!
//! It renders through the same pipeline regardless: `nice-plug-dioxus` →
//! Blitz → Vello → baseview, with `dioxus-devtools` connected, so an
//! `rsx!` edit hot-reloads into the very renderer the VST3/CLAP editor
//! and the REAPER panel use. That parity is the reason this is not a
//! `dioxus::desktop` window.

use daw::standalone::Standalone;
use dioxus::prelude::*;
use expression_editor_core::Viewport;
use expression_editor_ui::{ExpressionEditor, theme};

use crate::library::{self, Entry};
use crate::{Runner, Target};

/// Height of the chooser strip. Fixed, so the editor below it gets a
/// stable viewport — the canvas fits its content to the space it is
/// given, and a bar that changed height would re-fit on every load.
const BAR: f64 = 34.0;

/// The runner's window: a chooser, and the editor under it.
#[component]
pub fn DevApp() -> Element {
    // The staged document, exactly as `App` takes it, so `dx serve` with
    // a file argument still opens on that file.
    let editor = use_signal(|| {
        crate::app::take_staged().unwrap_or_else(|| {
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

/// The editor's viewport, less the chrome above it.
fn viewport() -> Viewport {
    Viewport::new(1200.0, 760.0 - BAR - 18.0)
}

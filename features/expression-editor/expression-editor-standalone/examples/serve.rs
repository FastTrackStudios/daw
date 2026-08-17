//! The expression editor, served — a window you keep open.
//!
//! ```sh
//! dx serve -p expression-editor-standalone --example serve --renderer native
//! # or, from the repo root:
//! just ee-serve
//! ```
//!
//! The difference from `--example editor` is not the renderer — both go
//! through `nice-plug-dioxus` → Blitz → Vello → baseview, which is what
//! the VST3/CLAP editor and the REAPER panel render through, and is why
//! neither uses `dioxus::desktop`. The difference is that this one is
//! built to be *left up*: `dx` watches the tree and hot-reloads `rsx!`
//! edits into the running window, and the file you are editing is
//! chosen in the app rather than on the command line.
//!
//! Point it at material with `EXPRESSION_EDITOR_LIBRARY`; it scans the
//! working directory otherwise. A source on the command line still
//! works, and becomes what the window opens on:
//!
//! ```sh
//! EXPRESSION_EDITOR_LIBRARY=~/Music/stems just ee-serve
//! ```

use expression_editor_standalone::cli::ArgsError;
use expression_editor_standalone::{Args, DevApp, Runner, library, stage};

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
    nice_plug_dioxus::open_standalone_with_state(DevApp, width, height, None);
}

//! The expression editor in a real window.
//!
//! ```sh
//! cargo run -p expression-editor-standalone --example editor
//! cargo run -p expression-editor-standalone --example editor -- guitar
//! cargo run -p expression-editor-standalone --example editor -- song.rpp --track Vox
//! cargo run -p expression-editor-standalone --example editor -- part.mid --mode mpe
//! ```
//!
//! The window comes from `nice_plug_dioxus::open_standalone_with_state`
//! — Blitz → Vello → baseview, the same pipeline a VST3/CLAP editor and
//! the REAPER panel render through. `dioxus::desktop::LaunchBuilder`
//! would put WebKit/WRY behind the same component and show something
//! the plugin will never show, which is why it is never used here.

use expression_editor_standalone::cli::ArgsError;
use expression_editor_standalone::{App, Args, Runner, stage};

fn main() {
    let args = match Args::from_env() {
        Ok(a) => a,
        // Help and the scene list are successful runs that happen not
        // to open a window.
        Err(e @ (ArgsError::Help | ArgsError::List)) => {
            print!("{e}");
            return;
        }
        Err(e) => {
            eprintln!("{e}\n\n{}", expression_editor_standalone::cli::USAGE);
            std::process::exit(2);
        }
    };

    let runner = match Runner::open(&args.source, &args.target, args.viewport(), args.mode) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    println!(
        "{} — {} notes, mode {}",
        runner.label,
        runner.loaded.editor().doc.notes.len(),
        runner.loaded.editor().mode.label()
    );

    // The backend has to outlive the window: the session's location is
    // only meaningful against the `Standalone` it was read from, and
    // dropping it here would leave a document with nowhere to go back
    // to the moment write-back lands.
    let _daw = runner.daw;
    stage(runner.loaded.into_editor());
    nice_plug_dioxus::open_standalone_with_state(App, args.width, args.height, None);
}

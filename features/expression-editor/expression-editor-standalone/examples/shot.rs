//! The same window, rasterized to a PNG instead of opened.
//!
//! ```sh
//! cargo run -p expression-editor-standalone --example shot -- \
//!     guitar --mode guitar --out target/gui-shots/guitar.png
//! ```
//!
//! This is where a mode's committed screenshot comes from. It paints
//! [`App`] — the *same* root the window mounts, not a lookalike — on a
//! headless Blitz DOM through `dioxus-test`'s CPU rasterizer. CPU
//! rather than the wgpu offscreen path because a screenshot that needs
//! a GPU cannot run on a CI box, and the point of committing pictures
//! is that they get regenerated.
//!
//! It takes the same arguments as `--example editor`, so the picture
//! and the window are the same load. A shot that came from a different
//! code path would be worse than no shot.

use std::path::PathBuf;

use dioxus::prelude::VirtualDom;
use dioxus_test::DocumentTester;
use expression_editor_standalone::cli::ArgsError;
use expression_editor_standalone::{App, Args, Runner, stage};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = match Args::from_env() {
        Ok(a) => a,
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

    let out = args
        .out
        .clone()
        .unwrap_or_else(|| default_out(&runner.label));
    if let Some(dir) = out.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!("create {}: {e}", dir.display());
        std::process::exit(1);
    }

    let _daw = runner.daw;
    stage(runner.loaded.into_editor());

    let dom = VirtualDom::new(App);
    let tester = DocumentTester::from_virtual_dom(dom)
        .with_window_size(args.width, args.height)
        .build();
    // Four pumps: mount, the canvas's own measure, and the two the
    // resulting layout change costs. Fewer and the roll paints at its
    // pre-measure size.
    for _ in 0..4 {
        let _ = tester.pump().await;
    }
    tester.render_png(&out);
    println!("shot: {}", out.display());
}

fn default_out(label: &str) -> PathBuf {
    let slug: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../target/gui-shots/expression-editor-standalone")
        .join(format!("{}.png", slug.trim_matches('-')))
}

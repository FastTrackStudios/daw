//! `task-pdf-render` — read a render request on stdin,
//! write PDF bytes on stdout.
//!
//! Run as a subprocess from any other crate that needs PDF
//! generation. Decouples the fulgur compile tree from the
//! workspace's giant interdependency graph (stylo's
//! recursion-limit issue surfaces in feature-unified
//! builds; isolating fulgur to one binary sidesteps it).
//!
//! Request shape (JSON):
//!
//! ```json
//! {
//!   "mode": "invoice",
//!   "data": { ... pdf::InvoiceData ... }
//! }
//! ```
//!
//! Or, for arbitrary HTML/CSS:
//!
//! ```json
//! { "mode": "html", "html": "<h1>…</h1>" }
//! ```
//!
//! Or, for a custom template:
//!
//! ```json
//! {
//!   "mode": "template",
//!   "name": "weekly.html",
//!   "template": "<html>…</html>",
//!   "data": { ... }
//! }
//! ```

use std::io::{Read, Write};

use clap::Parser;
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "task-pdf-render", about = "Stdin JSON → stdout PDF")]
struct Cli {
    /// Optional output path. Default: stdout.
    #[arg(short, long)]
    out: Option<std::path::PathBuf>,
}

#[derive(Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
enum Request {
    Invoice { data: pdf::InvoiceData },
    Html { html: String },
    Template {
        name: String,
        template: String,
        data: serde_json::Value,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        eprintln!("read stdin failed");
        return std::process::ExitCode::FAILURE;
    }
    let req: Request = match serde_json::from_str(&buf) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("parse request: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let bytes = match req {
        Request::Invoice { data } => pdf::render_invoice(&data),
        Request::Html { html } => pdf::render_html(&html),
        Request::Template {
            name,
            template,
            data,
        } => pdf::render_template(&name, &template, &data),
    };
    let bytes = match bytes {
        Ok(b) => b,
        Err(e) => {
            eprintln!("render: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    match cli.out {
        Some(path) => {
            if let Err(e) = std::fs::write(&path, &bytes) {
                eprintln!("write {}: {e}", path.display());
                return std::process::ExitCode::FAILURE;
            }
            eprintln!("wrote {} ({} bytes)", path.display(), bytes.len());
        }
        None => {
            let mut out = std::io::stdout().lock();
            if let Err(e) = out.write_all(&bytes) {
                eprintln!("write stdout: {e}");
                return std::process::ExitCode::FAILURE;
            }
        }
    }
    std::process::ExitCode::SUCCESS
}

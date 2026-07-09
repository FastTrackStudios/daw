//! `architect` — scaffolding CLI for architect projects.
//!
//! Today: `architect feature new <name>` drops a complete feature crate
//! family (`<name>-proto`, `<name>-memory`, `<name>` facade, native
//! tests, spec stub) into `features/<name>/` and wires it into the
//! workspace + tracey config.
//!
//! Future: `architect backend new <feature> <impl>`, `architect app new`,
//! `architect new <project>` for fresh repos.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use facet::Facet;
use figue as args;
use owo_colors::OwoColorize;

mod scaffold;

#[derive(Facet, Debug)]
struct FeatureNewArgs {
    /// kebab-case name of the new feature (e.g. `mixing`).
    #[facet(args::positional)]
    name: String,

    /// Overwrite existing files. Off by default — the CLI refuses to
    /// touch a non-empty target dir without this.
    #[facet(args::named)]
    force: bool,
}

fn print_usage() {
    eprintln!(
        "{}

  architect feature new <name> [--force]
       Scaffold a new feature at features/<name>/ — proto + memory
       backend + facade + native tests + spec stub. Wires it into the
       workspace Cargo.toml and .config/tracey/config.styx.

  architect --help
       This message.

{}: invoke from the repo root (where Cargo.toml has [workspace]).
",
        "USAGE".bold(),
        "NOTE".dimmed()
    );
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    let argv_strs: Vec<&str> = argv.iter().map(String::as_str).collect();

    let result: eyre::Result<()> = match argv_strs.split_first() {
        None | Some((&"--help" | &"-h" | &"help", _)) => {
            print_usage();
            return ExitCode::from(if argv.is_empty() { 2 } else { 0 });
        }
        Some((&"feature", rest)) => match rest.split_first() {
            Some((&"new", inner)) => {
                let args: FeatureNewArgs = figue::from_slice(inner).unwrap();
                repo_root().and_then(|root| scaffold::feature_new(&root, &args.name, args.force))
            }
            _ => {
                eprintln!("{}: architect feature <new>", "ERROR".red().bold());
                print_usage();
                return ExitCode::from(2);
            }
        },
        Some((cmd, _)) => {
            eprintln!("{}: unknown command `{cmd}`", "ERROR".red().bold());
            print_usage();
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}: {e:#}", "ERROR".red().bold());
            ExitCode::FAILURE
        }
    }
}

/// Walk up from the current dir until we find a `Cargo.toml` containing
/// a `[workspace]` section. That's the architect repo root.
fn repo_root() -> eyre::Result<PathBuf> {
    let mut dir = env::current_dir()?;
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            let contents = std::fs::read_to_string(&candidate)?;
            if contents.contains("[workspace]") {
                return Ok(dir);
            }
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => eyre::bail!(
                "no workspace Cargo.toml found walking up from {:?}",
                env::current_dir()?
            ),
        }
    }
}

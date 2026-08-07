//! `fts-themer` — edit the FastTrackStudio REAPER theme from the shell.
//!
//! The same operations the web GUI drives, minus the preview.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use fts_themer::{add_accent, color::Rgb, groups, ThemeDir};

/// Default theme location, relative to the repo root.
const DEFAULT_THEME: &str = "features/reaper/fts-theme";

#[derive(Parser)]
#[command(name = "fts-themer", about = "Edit a REAPER theme's colors and artwork")]
struct Cli {
    /// Theme directory (holding <name>.ReaperTheme) or the .ReaperTheme itself.
    #[arg(long, short, global = true, default_value = DEFAULT_THEME)]
    theme: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show what the theme is and where its parts live.
    Info,
    /// List color keys and their values, grouped by the area they paint.
    Colors {
        /// Only keys whose name contains this substring.
        #[arg(long, short)]
        filter: Option<String>,
        /// Only this group (e.g. `mcp`, `arrange`).
        #[arg(long, short)]
        group: Option<String>,
    },
    /// Set one or more colors: `col_cursor=#00b0f9`.
    Set {
        /// `key=#rrggbb` pairs.
        #[arg(required = true)]
        assignments: Vec<String>,
    },
    /// List the accent (fader color) variants this theme offers.
    Accents,
    /// Generate a new accent variant, artwork and layouts.
    AddAccent {
        /// Folder/layout name, e.g. `crimson`.
        name: String,
        /// Target color, `#rrggbb`.
        color: String,
        /// Existing accent to recolor from.
        #[arg(long, default_value = "blue")]
        from: String,
        /// Take only hue and saturation from the color, keeping the source
        /// artwork's lightness — so the new accent sits tonally with the set.
        #[arg(long)]
        keep_tone: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut theme = ThemeDir::open(&cli.theme)
        .with_context(|| format!("open theme at {}", cli.theme.display()))?;

    match cli.command {
        Command::Info => {
            println!("name       {}", theme.name);
            println!("ini        {}", theme.ini_path().display());
            println!("images     {}", theme.images_dir().display());
            println!("rtconfig   {}", theme.rtconfig_path().display());
            println!("colors     {}", theme.ini().len());
            println!("accents    {}", theme.accents()?.join(", "));
        }

        Command::Colors { filter, group } => {
            let keys = theme.ini().keys();
            let wanted = group.map(|g| g.to_ascii_lowercase());
            for (grp, members) in groups::group_all(keys) {
                if let Some(w) = &wanted
                    && !grp.label().to_ascii_lowercase().contains(w)
                    && !format!("{grp:?}").to_ascii_lowercase().contains(w)
                {
                    continue;
                }
                let shown: Vec<&str> = members
                    .into_iter()
                    .filter(|k| filter.as_ref().is_none_or(|f| k.contains(f.as_str())))
                    .collect();
                if shown.is_empty() {
                    continue;
                }
                println!("\n{}", grp.label());
                for key in shown {
                    let raw = theme.ini().int(key).unwrap_or(0);
                    if groups::is_color(key) {
                        println!("  {key:<34} {}", Rgb::from_colorref(raw).to_hex());
                    } else {
                        println!("  {key:<34} {raw}");
                    }
                }
            }
        }

        Command::Set { assignments } => {
            for pair in &assignments {
                let (key, value) = pair
                    .split_once('=')
                    .with_context(|| format!("expected key=#rrggbb, got {pair:?}"))?;
                let color = Rgb::parse_hex(value)
                    .with_context(|| format!("color for {key}"))?;
                let before = theme.ini().color(key);
                theme.ini_mut().set_color(key, color);
                match before {
                    Some(b) => println!("{key}: {} -> {}", b.to_hex(), color.to_hex()),
                    None => println!("{key}: (new) -> {}", color.to_hex()),
                }
            }
            theme.save_ini()?;
            println!("\nWrote {}", theme.ini_path().display());
        }

        Command::Accents => {
            for accent in theme.accents()? {
                println!("{accent}");
            }
        }

        Command::AddAccent {
            name,
            color,
            from,
            keep_tone,
        } => {
            let color = Rgb::parse_hex(&color)?;
            let report = add_accent(&theme, &name, color, &from, keep_tone)?;
            for path in &report.images {
                println!("wrote {}", path.display());
            }
            match report.layouts {
                0 => println!("layouts already registered in {}", report.rtconfig.display()),
                n => println!("added {n} layout blocks to {}", report.rtconfig.display()),
            }
        }
    }

    Ok(())
}

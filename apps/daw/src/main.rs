//! daw — CLI tool for live-querying a running REAPER instance

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use eyre::Result;

#[derive(Parser)]
#[command(name = "daw", about = "Live-query a running REAPER instance")]
struct Cli {
    /// Unix socket path (auto-discovers from /tmp if omitted)
    #[arg(long, global = true)]
    socket: Option<PathBuf>,

    /// Output as JSON
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show project info
    Info,
    /// List all tracks
    Tracks,
    /// Show details for a specific track
    Track {
        /// Track name or index
        track: String,
    },
    /// List FX chain for a track
    Fx {
        /// Track name or index
        track: String,
    },
    /// List parameters for an FX on a track
    Params {
        /// Track name or index
        track: String,
        /// FX name or index
        fx: String,
    },
    /// Show transport state
    Transport,
    /// List markers
    Markers,
    /// List regions
    Regions,
    /// List all installed plugins
    Plugins,
    /// Check if a DAW instance is reachable
    Ping,

    // -- Process & Project Management --

    /// Launch a REAPER instance
    Launch {
        /// Config ID (e.g., "fts-tracks", "fts-signal")
        #[arg(long)]
        config: Option<String>,
    },
    /// Quit a running REAPER instance (sends SIGTERM)
    Quit {
        /// PID of the REAPER instance to kill
        #[arg(long)]
        pid: Option<u32>,
    },
    /// List open project tabs
    Projects,
    /// Open a project file
    Open {
        /// Path to the .rpp project file
        path: String,
    },
    /// Close a project tab
    Close {
        /// GUID of the project to close (defaults to current)
        #[arg(long)]
        guid: Option<String>,
    },
    /// Add a new track
    AddTrack {
        /// Track name (default: "New Track")
        #[arg(long)]
        name: Option<String>,
        /// Insert at index (default: append)
        #[arg(long)]
        at: Option<u32>,
    },
    /// Remove a track
    RemoveTrack {
        /// Track name or index
        track: String,
    },

    // -- File Operations --

    /// Combine multiple RPP files into a single project
    Combine {
        /// Path to .RPL file or list of .RPP files
        input: String,
        /// Output .RPP file path (default: derived from input name)
        #[arg(short, long)]
        output: Option<String>,
        /// Gap between songs in measures (uses next song's tempo)
        #[arg(long, default_value = "0")]
        gap: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    // Commands that don't need an RPC connection
    match cli.command {
        Command::Launch { ref config } => {
            return daw_cli::cmd_launch(config.as_deref());
        }
        Command::Quit { pid } => {
            return daw_cli::cmd_quit(pid);
        }
        Command::Combine { ref input, ref output, gap } => {
            return daw_cli::cmd_combine(input, output.as_deref(), gap);
        }
        _ => {}
    }

    // All other commands need a DAW connection
    let daw = daw_cli::connect(cli.socket).await?;

    match cli.command {
        Command::Info => daw_cli::cmd_info(&daw, cli.json).await?,
        Command::Tracks => daw_cli::cmd_tracks(&daw, cli.json).await?,
        Command::Track { ref track } => daw_cli::cmd_track(&daw, track, cli.json).await?,
        Command::Fx { ref track } => daw_cli::cmd_fx(&daw, track, cli.json).await?,
        Command::Params { ref track, ref fx } => daw_cli::cmd_params(&daw, track, fx, cli.json).await?,
        Command::Transport => daw_cli::cmd_transport(&daw, cli.json).await?,
        Command::Markers => daw_cli::cmd_markers(&daw, cli.json).await?,
        Command::Regions => daw_cli::cmd_regions(&daw, cli.json).await?,
        Command::Plugins => daw_cli::cmd_plugins(&daw, cli.json).await?,
        Command::Ping => daw_cli::cmd_ping(&daw).await?,
        Command::Projects => daw_cli::cmd_projects(&daw, cli.json).await?,
        Command::Open { ref path } => daw_cli::cmd_open(&daw, path, cli.json).await?,
        Command::Close { ref guid } => daw_cli::cmd_close(&daw, guid.as_deref()).await?,
        Command::AddTrack { ref name, at } => daw_cli::cmd_add_track(&daw, name.as_deref(), at, cli.json).await?,
        Command::RemoveTrack { ref track } => daw_cli::cmd_remove_track(&daw, track).await?,
        // Already handled above
        Command::Launch { .. } | Command::Quit { .. } | Command::Combine { .. } => unreachable!(),
    }

    Ok(())
}

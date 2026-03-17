//! daw-cli library — reusable components for DAW CLI tools.
//!
//! Provides socket discovery, connection management, track/FX resolution,
//! formatting helpers, and command implementations for querying a running
//! REAPER instance via the roam RPC protocol.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use daw::Daw;
use daw::service::FxType;
use eyre::{Result, bail};
use roam::ErasedCaller;
use roam::SessionHandle;
use serde_json::json;

/// A DAW connection that keeps the roam session alive.
///
/// The `SessionHandle` must be kept alive for the duration of use —
/// dropping it closes the underlying roam session and all RPC calls will fail.
pub struct DawConnection {
    pub daw: Daw,
    _session: SessionHandle,
}

impl std::ops::Deref for DawConnection {
    type Target = Daw;
    fn deref(&self) -> &Daw {
        &self.daw
    }
}

// ============================================================================
// Socket Discovery
// ============================================================================

const SOCKET_DIR: &str = "/tmp";
const SOCKET_PREFIX: &str = "fts-daw-";
const SOCKET_SUFFIX: &str = ".sock";

pub fn discover_socket() -> Option<PathBuf> {
    let entries = std::fs::read_dir(SOCKET_DIR).ok()?;
    let mut sockets: Vec<(u32, PathBuf)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let filename = path.file_name()?.to_str()?;
            let rest = filename.strip_prefix(SOCKET_PREFIX)?;
            let pid_str = rest.strip_suffix(SOCKET_SUFFIX)?;
            let pid: u32 = pid_str.parse().ok()?;
            // Check if process is alive
            let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
            if alive { Some((pid, path)) } else { None }
        })
        .collect();

    // Sort by PID (most recent process likely has highest PID)
    sockets.sort_by(|a, b| b.0.cmp(&a.0));
    sockets.into_iter().next().map(|(_, path)| path)
}

/// Discover all DAW sockets in /tmp, returning (pid, path) pairs.
pub fn discover_all_sockets() -> Vec<(u32, PathBuf)> {
    let entries = match std::fs::read_dir(SOCKET_DIR) {
        Ok(e) => e,
        Err(_) => return vec![],
    };
    let mut sockets: Vec<(u32, PathBuf)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let filename = path.file_name()?.to_str()?;
            let rest = filename.strip_prefix(SOCKET_PREFIX)?;
            let pid_str = rest.strip_suffix(SOCKET_SUFFIX)?;
            let pid: u32 = pid_str.parse().ok()?;
            let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
            if alive { Some((pid, path)) } else { None }
        })
        .collect();
    sockets.sort_by(|a, b| b.0.cmp(&a.0));
    sockets
}

// ============================================================================
// REAPER Launcher
// ============================================================================

/// A known REAPER configuration (app bundle + role).
pub struct ReaperConfig {
    pub id: &'static str,
    pub label: &'static str,
    pub executable: String,
    pub resources: String,
    pub role: &'static str,
}

fn fts_home() -> String {
    if let Ok(p) = std::env::var("FTS_HOME") {
        return p;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let production = format!("{home}/Music/FastTrackStudio");
    if std::path::Path::new(&format!("{production}/Reaper/reaper.ini")).exists() {
        return production;
    }
    format!("{home}/Music/Dev/FastTrackStudio")
}

pub fn reaper_configs() -> Vec<ReaperConfig> {
    let fts = fts_home();
    let live_app = format!("{fts}/Reaper/FTS-LIVE.app");
    let executable = format!("{live_app}/Contents/MacOS/REAPER");
    let resources = format!("{live_app}/Contents/Resources");

    vec![
        ReaperConfig {
            id: "fts-tracks",
            label: "FTS-TRACKS (Session)",
            executable: executable.clone(),
            resources: resources.clone(),
            role: "session",
        },
        ReaperConfig {
            id: "fts-signal",
            label: "FTS-SIGNAL (Signal)",
            executable,
            resources,
            role: "signal",
        },
    ]
}

pub fn config_by_id(id: &str) -> Option<ReaperConfig> {
    reaper_configs().into_iter().find(|c| c.id == id)
}

pub fn spawn_reaper(config: &ReaperConfig) -> Result<u32> {
    let mut cmd = Command::new(&config.executable);
    cmd.current_dir(&config.resources)
        .env("FTS_DAW_ROLE", config.role)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg("-newinst")
        .arg("-nosplash")
        .arg("-ignoreerrors");

    let child = cmd
        .spawn()
        .map_err(|e| eyre::eyre!("Failed to spawn REAPER ({}) at {}: {e}", config.label, config.executable))?;

    let pid = child.id();
    drop(child);
    Ok(pid)
}

pub fn kill_reaper(pid: u32) -> bool {
    Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Spawn REAPER and wait for its Unix socket to appear, then connect.
///
/// Returns `(Daw, pid, socket_path)` on success. The caller is responsible
/// for calling `teardown_owned(pid, socket_path)` when done.
pub async fn launch_and_connect(config_id: &str) -> Result<(DawConnection, u32, PathBuf)> {
    let config = config_by_id(config_id)
        .ok_or_else(|| eyre::eyre!("Unknown REAPER config: {config_id}"))?;

    eprintln!("Spawning REAPER ({})...", config.label);
    let pid = spawn_reaper(&config)?;
    let socket_path = PathBuf::from(format!("/tmp/fts-daw-{pid}.sock"));
    let _ = std::fs::remove_file(&socket_path); // remove any stale

    // Wait up to 30s for socket to appear
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    eprint!("  Waiting for socket");
    loop {
        if socket_path.exists() {
            break;
        }
        if std::time::Instant::now() > deadline {
            eprintln!();
            kill_reaper(pid);
            return Err(eyre::eyre!("Timed out waiting for REAPER socket after 30s"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        eprint!(".");
    }
    eprintln!("\n  Socket ready: {}", socket_path.display());

    let daw = connect(Some(socket_path.clone())).await?;
    Ok((daw, pid, socket_path))
}

/// Kill an owned REAPER instance and remove its socket file.
pub fn teardown_owned(pid: u32, socket: &PathBuf) {
    kill_reaper(pid);
    let _ = std::fs::remove_file(socket);
    eprintln!("REAPER (PID {pid}) stopped.");
}

// ============================================================================
// Connection
// ============================================================================

pub async fn connect(socket: Option<PathBuf>) -> Result<DawConnection> {
    let path = match socket {
        Some(p) => p,
        None => discover_socket()
            .ok_or_else(|| eyre::eyre!("No DAW socket found in /tmp. Is REAPER running with the FTS extension?"))?,
    };

    eprintln!("Connecting to {}", path.display());

    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::net::UnixStream::connect(&path),
    )
    .await
    .map_err(|_| eyre::eyre!("Timed out connecting to {}", path.display()))?
    .map_err(|e| eyre::eyre!("Failed to connect to {}: {}", path.display(), e))?;

    let link = roam_stream::StreamLink::unix(stream);
    let (caller, session) = roam::initiator(link)
        .establish::<roam::DriverCaller>(())
        .await
        .map_err(|e| eyre::eyre!("Failed to establish roam session: {:?}", e))?;

    Ok(DawConnection {
        daw: Daw::new(ErasedCaller::new(caller)),
        _session: session,
    })
}

// ============================================================================
// Track Resolution
// ============================================================================

/// Parse a track argument as either an index (if numeric) or name.
/// Returns (guid, name).
pub async fn resolve_track(daw: &Daw, track_arg: &str) -> Result<(String, String)> {
    let project = daw.current_project().await?;
    let tracks = project.tracks();

    // Try as index first
    if let Ok(idx) = track_arg.parse::<u32>() {
        if let Some(handle) = tracks.by_index(idx).await? {
            let info = handle.info().await?;
            return Ok((info.guid.clone(), info.name.clone()));
        }
        bail!("No track at index {idx}");
    }

    // Try as name
    if let Some(handle) = tracks.by_name(track_arg).await? {
        let info = handle.info().await?;
        return Ok((info.guid.clone(), info.name.clone()));
    }

    bail!("No track named \"{track_arg}\"");
}

/// Resolve a track argument and return the TrackHandle directly.
pub async fn resolve_track_handle(daw: &Daw, track_arg: &str) -> Result<daw::TrackHandle> {
    let (guid, _) = resolve_track(daw, track_arg).await?;
    let project = daw.current_project().await?;
    project
        .tracks()
        .by_guid(&guid)
        .await?
        .ok_or_else(|| eyre::eyre!("Track not found"))
}

/// Resolve an FX argument (index or name) on a track's FX chain.
pub async fn resolve_fx_handle(
    fx_chain: &daw::FxChain,
    fx_arg: &str,
    track_name: &str,
) -> Result<daw::FxHandle> {
    let fx_handle = if let Ok(idx) = fx_arg.parse::<u32>() {
        fx_chain.by_index(idx).await?
    } else {
        fx_chain.by_name(fx_arg).await?
    };
    fx_handle.ok_or_else(|| eyre::eyre!("FX \"{fx_arg}\" not found on track \"{track_name}\""))
}

// ============================================================================
// Formatting Helpers
// ============================================================================

pub fn format_position(pos: &daw::service::primitives::Position) -> String {
    if let Some(ref musical) = pos.musical {
        format!("{}.{}.{:03}", musical.measure, musical.beat, musical.subdivision)
    } else if let Some(ref time) = pos.time {
        let secs = time.as_seconds();
        let mins = (secs / 60.0).floor() as u32;
        let remaining = secs - (mins as f64 * 60.0);
        format!("{}:{:06.3}", mins, remaining)
    } else {
        "?".to_string()
    }
}

pub fn vol_to_db(vol: f64) -> String {
    if vol <= 0.0 {
        "-inf dB".to_string()
    } else {
        format!("{:.1} dB", 20.0 * vol.log10())
    }
}

pub fn pan_to_string(pan: f64) -> String {
    if pan.abs() < 0.005 {
        "C".to_string()
    } else if pan < 0.0 {
        format!("{:.0}%L", pan.abs() * 100.0)
    } else {
        format!("{:.0}%R", pan * 100.0)
    }
}

pub fn fx_type_str(ft: &FxType) -> &'static str {
    match ft {
        FxType::Vst2 => "VST2",
        FxType::Vst3 => "VST3",
        FxType::Au => "AU",
        FxType::Js => "JS",
        FxType::Clap => "CLAP",
        FxType::Unknown => "?",
    }
}

pub fn flags_str(muted: bool, soloed: bool, armed: bool) -> String {
    let mut flags = Vec::new();
    if muted { flags.push("M"); }
    if soloed { flags.push("S"); }
    if armed { flags.push("R"); }
    if flags.is_empty() { "-".to_string() } else { flags.join("") }
}

// ============================================================================
// Commands
// ============================================================================

pub async fn cmd_info(daw: &Daw, as_json: bool) -> Result<()> {
    let project = daw.current_project().await?;
    let info = project.info().await?;
    let track_count = project.n_tracks().await?;
    let transport = project.transport().get_state().await?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&json!({
            "name": info.name,
            "path": info.path,
            "guid": info.guid,
            "track_count": track_count,
            "tempo": transport.tempo.bpm,
            "time_signature": format!("{}/{}", transport.time_signature.numerator, transport.time_signature.denominator),
        }))?);
    } else {
        println!("Project: {}", info.name);
        println!("Path:    {}", info.path);
        println!("GUID:    {}", info.guid);
        println!("Tracks:  {}", track_count);
        println!("Tempo:   {:.1} BPM", transport.tempo.bpm);
        println!("Time:    {}/{}", transport.time_signature.numerator, transport.time_signature.denominator);
    }
    Ok(())
}

pub async fn cmd_tracks(daw: &Daw, as_json: bool) -> Result<()> {
    let project = daw.current_project().await?;
    let all_tracks = project.tracks().all().await?;

    if as_json {
        let arr: Vec<_> = all_tracks
            .iter()
            .map(|t| {
                json!({
                    "index": t.index,
                    "name": t.name,
                    "guid": t.guid,
                    "muted": t.muted,
                    "soloed": t.soloed,
                    "armed": t.armed,
                    "selected": t.selected,
                    "volume": t.volume,
                    "volume_db": vol_to_db(t.volume),
                    "pan": t.pan,
                    "is_folder": t.is_folder,
                    "folder_depth": t.folder_depth,
                    "fx_count": t.fx_count,
                    "input_fx_count": t.input_fx_count,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        if all_tracks.is_empty() {
            println!("No tracks.");
            return Ok(());
        }

        // Header
        println!(
            "{:>4}  {:<30}  {:>5}  {:>9}  {:>5}  {:>3}",
            "#", "Name", "Flags", "Volume", "Pan", "FX"
        );
        println!("{}", "-".repeat(68));

        for t in &all_tracks {
            let indent = if t.folder_depth > 0 {
                "  ".repeat(t.folder_depth as usize)
            } else {
                String::new()
            };
            let name = format!(
                "{}{}{}",
                indent,
                if t.is_folder { "[" } else { "" },
                t.name,
            );
            let name = if t.is_folder {
                format!("{}]", name)
            } else {
                name
            };
            println!(
                "{:>4}  {:<30}  {:>5}  {:>9}  {:>5}  {:>3}",
                t.index,
                if name.len() > 30 { &name[..30] } else { &name },
                flags_str(t.muted, t.soloed, t.armed),
                vol_to_db(t.volume),
                pan_to_string(t.pan),
                t.fx_count,
            );
        }
    }
    Ok(())
}

pub async fn cmd_track(daw: &Daw, track_arg: &str, as_json: bool) -> Result<()> {
    let handle = resolve_track_handle(daw, track_arg).await?;
    let t = handle.info().await?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&json!({
            "index": t.index,
            "name": t.name,
            "guid": t.guid,
            "muted": t.muted,
            "soloed": t.soloed,
            "armed": t.armed,
            "selected": t.selected,
            "volume": t.volume,
            "volume_db": vol_to_db(t.volume),
            "pan": t.pan,
            "is_folder": t.is_folder,
            "folder_depth": t.folder_depth,
            "parent_guid": t.parent_guid,
            "visible_in_tcp": t.visible_in_tcp,
            "visible_in_mixer": t.visible_in_mixer,
            "fx_count": t.fx_count,
            "input_fx_count": t.input_fx_count,
            "color": t.color,
        }))?);
    } else {
        println!("Track #{}: {}", t.index, t.name);
        println!("  GUID:     {}", t.guid);
        println!("  Volume:   {} ({:.4})", vol_to_db(t.volume), t.volume);
        println!("  Pan:      {}", pan_to_string(t.pan));
        println!("  Muted:    {}", t.muted);
        println!("  Soloed:   {}", t.soloed);
        println!("  Armed:    {}", t.armed);
        println!("  Selected: {}", t.selected);
        println!("  Folder:   {}", t.is_folder);
        if let Some(ref parent) = t.parent_guid {
            println!("  Parent:   {}", parent);
        }
        println!("  FX:       {} (input: {})", t.fx_count, t.input_fx_count);
        if let Some(color) = t.color {
            println!("  Color:    #{:06X}", color);
        }
    }
    Ok(())
}

pub async fn cmd_fx(daw: &Daw, track_arg: &str, as_json: bool) -> Result<()> {
    let (guid, track_name) = resolve_track(daw, track_arg).await?;
    let project = daw.current_project().await?;
    let handle = project
        .tracks()
        .by_guid(&guid)
        .await?
        .ok_or_else(|| eyre::eyre!("Track not found"))?;
    let fx_chain = handle.fx_chain();
    let fx_list = fx_chain.all().await?;

    if as_json {
        let arr: Vec<_> = fx_list
            .iter()
            .map(|f| {
                json!({
                    "index": f.index,
                    "name": f.name,
                    "plugin_name": f.plugin_name,
                    "plugin_type": fx_type_str(&f.plugin_type),
                    "guid": f.guid,
                    "enabled": f.enabled,
                    "offline": f.offline,
                    "parameter_count": f.parameter_count,
                    "preset_name": f.preset_name,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        if fx_list.is_empty() {
            println!("No FX on track \"{}\".", track_name);
            return Ok(());
        }
        println!("FX chain for \"{}\" ({} plugins):", track_name, fx_list.len());
        println!();
        for f in &fx_list {
            let status = if !f.enabled {
                " [BYPASS]"
            } else if f.offline {
                " [OFFLINE]"
            } else {
                ""
            };
            println!(
                "  {:>2}. {} ({}){}",
                f.index,
                f.name,
                fx_type_str(&f.plugin_type),
                status,
            );
            if let Some(ref preset) = f.preset_name {
                println!("      Preset: {}", preset);
            }
            println!("      Params: {}  GUID: {}", f.parameter_count, f.guid);
        }
    }
    Ok(())
}

pub async fn cmd_plugins(daw: &Daw, as_json: bool) -> Result<()> {
    let plugins = daw.installed_plugins().await?;

    if as_json {
        let arr: Vec<_> = plugins
            .iter()
            .map(|p| {
                json!({
                    "name": p.name,
                    "ident": p.ident,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        println!("{:<4} {:<50} {}", "#", "Name", "Identifier");
        println!("{}", "\u{2500}".repeat(100));
        for (i, p) in plugins.iter().enumerate() {
            println!("{:<4} {:<50} {}", i, p.name, p.ident);
        }
        println!("\n{} plugins installed", plugins.len());
    }
    Ok(())
}

pub async fn cmd_params(daw: &Daw, track_arg: &str, fx_arg: &str, as_json: bool) -> Result<()> {
    let (_, track_name) = resolve_track(daw, track_arg).await?;
    let handle = resolve_track_handle(daw, track_arg).await?;
    let fx_chain = handle.fx_chain();
    let fx_handle = resolve_fx_handle(&fx_chain, fx_arg, &track_name).await?;
    let fx_info = fx_handle.info().await?;
    let params = fx_handle.parameters().await?;

    if as_json {
        let arr: Vec<_> = params
            .iter()
            .map(|p| {
                let mut obj = json!({
                    "index": p.index,
                    "name": p.name,
                    "value": p.value,
                    "formatted": p.formatted,
                    "is_toggle": p.is_toggle,
                });
                if let Some(steps) = p.step_count {
                    obj["step_count"] = json!(steps);
                }
                if !p.step_labels.is_empty() {
                    obj["step_labels"] = json!(
                        p.step_labels.iter().map(|(v, l)| json!({"value": v, "label": l})).collect::<Vec<_>>()
                    );
                }
                obj
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        println!(
            "Parameters for \"{}\" on \"{}\" ({} params):",
            fx_info.name, track_name, params.len()
        );
        println!();
        println!(
            "{:>4}  {:<35}  {:>8}  {}",
            "#", "Name", "Value", "Display"
        );
        println!("{}", "-".repeat(75));

        for p in &params {
            println!(
                "{:>4}  {:<35}  {:>8.4}  {}",
                p.index,
                if p.name.len() > 35 { &p.name[..35] } else { &p.name },
                p.value,
                p.formatted,
            );
        }
    }
    Ok(())
}

pub async fn cmd_transport(daw: &Daw, as_json: bool) -> Result<()> {
    let project = daw.current_project().await?;
    let state = project.transport().get_state().await?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&json!({
            "play_state": format!("{:?}", state.play_state),
            "record_mode": format!("{:?}", state.record_mode),
            "looping": state.looping,
            "tempo": state.tempo.bpm,
            "playrate": state.playrate,
            "time_signature": format!("{}/{}", state.time_signature.numerator, state.time_signature.denominator),
            "playhead": format_position(&state.playhead_position),
            "edit_cursor": format_position(&state.edit_position),
        }))?);
    } else {
        println!("Transport");
        println!("  State:     {:?}", state.play_state);
        println!("  Playhead:  {}", format_position(&state.playhead_position));
        println!("  Edit:      {}", format_position(&state.edit_position));
        println!("  Tempo:     {:.1} BPM", state.tempo.bpm);
        println!("  Time Sig:  {}/{}", state.time_signature.numerator, state.time_signature.denominator);
        println!("  Playrate:  {:.2}x", state.playrate);
        println!("  Looping:   {}", state.looping);
        if let Some(ref lr) = state.loop_region {
            println!("  Loop:      {:.3}s - {:.3}s", lr.start_seconds, lr.end_seconds);
        }
        println!("  Record:    {:?}", state.record_mode);
    }
    Ok(())
}

pub async fn cmd_markers(daw: &Daw, as_json: bool) -> Result<()> {
    let project = daw.current_project().await?;
    let markers = project.markers().all().await?;

    if as_json {
        let arr: Vec<_> = markers
            .iter()
            .map(|m| {
                json!({
                    "id": m.id,
                    "name": m.name,
                    "position": format_position(&m.position),
                    "color": m.color,
                    "guid": m.guid,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        if markers.is_empty() {
            println!("No markers.");
            return Ok(());
        }
        println!(
            "{:>4}  {:<14}  {}",
            "ID", "Position", "Name"
        );
        println!("{}", "-".repeat(45));
        for m in &markers {
            println!(
                "{:>4}  {:<14}  {}",
                m.id.map(|i| i.to_string()).unwrap_or_default(),
                format_position(&m.position),
                m.name,
            );
        }
    }
    Ok(())
}

pub async fn cmd_regions(daw: &Daw, as_json: bool) -> Result<()> {
    let project = daw.current_project().await?;
    let regions = project.regions().all().await?;

    if as_json {
        let arr: Vec<_> = regions
            .iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "name": r.name,
                    "start": format_position(&r.time_range.start),
                    "end": format_position(&r.time_range.end),
                    "color": r.color,
                    "guid": r.guid,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        if regions.is_empty() {
            println!("No regions.");
            return Ok(());
        }
        println!(
            "{:>4}  {:<14}  {:<14}  {}",
            "ID", "Start", "End", "Name"
        );
        println!("{}", "-".repeat(55));
        for r in &regions {
            println!(
                "{:>4}  {:<14}  {:<14}  {}",
                r.id.map(|i| i.to_string()).unwrap_or_default(),
                format_position(&r.time_range.start),
                format_position(&r.time_range.end),
                r.name,
            );
        }
    }
    Ok(())
}

pub async fn cmd_ping(daw: &Daw) -> Result<()> {
    if daw.healthcheck().await {
        println!("OK");
    } else {
        bail!("Health check failed");
    }
    Ok(())
}

// ============================================================================
// Process & Project Management Commands
// ============================================================================

pub fn cmd_launch(config_id: Option<&str>) -> Result<()> {
    let id = config_id.unwrap_or("fts-tracks");
    let config = config_by_id(id)
        .ok_or_else(|| {
            let known: Vec<_> = reaper_configs().iter().map(|c| c.id).collect();
            eyre::eyre!("Unknown config \"{id}\". Known configs: {}", known.join(", "))
        })?;

    let pid = spawn_reaper(&config)?;
    println!("Launched {} (PID {pid})", config.label);
    Ok(())
}

pub fn cmd_quit(pid: Option<u32>) -> Result<()> {
    let target_pid = match pid {
        Some(p) => p,
        None => {
            // Extract PID from the discovered socket
            let sockets = discover_all_sockets();
            if sockets.is_empty() {
                bail!("No running DAW instances found");
            }
            if sockets.len() > 1 {
                eprintln!("Multiple instances found:");
                for (pid, path) in &sockets {
                    eprintln!("  PID {pid}  {}", path.display());
                }
                eprintln!("Killing most recent (PID {})", sockets[0].0);
            }
            sockets[0].0
        }
    };

    if kill_reaper(target_pid) {
        println!("Sent SIGTERM to PID {target_pid}");
    } else {
        bail!("Failed to kill PID {target_pid}");
    }
    Ok(())
}

pub async fn cmd_projects(daw: &Daw, as_json: bool) -> Result<()> {
    let projects = daw.projects().await?;

    if as_json {
        let mut arr = Vec::new();
        for (i, p) in projects.iter().enumerate() {
            let info = p.info().await?;
            arr.push(json!({
                "index": i,
                "name": info.name,
                "guid": info.guid,
                "path": info.path,
            }));
        }
        println!("{}", serde_json::to_string_pretty(&arr)?);
    } else {
        if projects.is_empty() {
            println!("No open projects.");
            return Ok(());
        }
        println!(
            "{:>3}  {:<30}  {:<38}  {}",
            "#", "Name", "GUID", "Path"
        );
        println!("{}", "-".repeat(100));
        for (i, p) in projects.iter().enumerate() {
            let info = p.info().await?;
            println!(
                "{:>3}  {:<30}  {:<38}  {}",
                i,
                if info.name.len() > 30 { &info.name[..30] } else { &info.name },
                info.guid,
                info.path,
            );
        }
    }
    Ok(())
}

pub async fn cmd_open(daw: &Daw, path: &str, as_json: bool) -> Result<()> {
    let project = daw.open_project(path).await?;
    let info = project.info().await?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&json!({
            "name": info.name,
            "guid": info.guid,
            "path": info.path,
        }))?);
    } else {
        println!("Opened project: {}", info.name);
        println!("  GUID: {}", info.guid);
        println!("  Path: {}", info.path);
    }
    Ok(())
}

pub async fn cmd_close(daw: &Daw, guid: Option<&str>) -> Result<()> {
    let target_guid = match guid {
        Some(g) => g.to_string(),
        None => {
            let project = daw.current_project().await?;
            let info = project.info().await?;
            info.guid.clone()
        }
    };

    daw.close_project(&target_guid).await?;
    println!("Closed project {target_guid}");
    Ok(())
}

pub async fn cmd_add_track(daw: &Daw, name: Option<&str>, at_index: Option<u32>, as_json: bool) -> Result<()> {
    let project = daw.current_project().await?;
    let track_name = name.unwrap_or("New Track");
    let handle = project.tracks().add(track_name, at_index).await?;
    let info = handle.info().await?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&json!({
            "index": info.index,
            "name": info.name,
            "guid": info.guid,
        }))?);
    } else {
        println!("Added track #{}: {} (GUID: {})", info.index, info.name, info.guid);
    }
    Ok(())
}

pub async fn cmd_remove_track(daw: &Daw, track_arg: &str) -> Result<()> {
    let (guid, name) = resolve_track(daw, track_arg).await?;
    let project = daw.current_project().await?;
    project.tracks().remove(daw::service::TrackRef::Guid(guid.clone())).await?;
    println!("Removed track \"{}\" ({})", name, guid);
    Ok(())
}

// ── File Operations ──────────────────────────────────────────────────────────

pub fn cmd_combine(input: &str, output: Option<&str>, gap_measures: u32) -> Result<()> {
    use dawfile_reaper::setlist_rpp::{self, CombineOptions};
    use std::path::Path;

    let input_path = Path::new(input);
    if !input_path.exists() {
        eyre::bail!("Input file not found: {}", input);
    }

    // Determine output path
    let output_path = if let Some(out) = output {
        PathBuf::from(out)
    } else {
        // Default: same name as input but with .RPP extension, in same directory
        let stem = input_path.file_stem().unwrap_or_default();
        let parent = input_path.parent().unwrap_or(Path::new("."));
        parent.join(format!("{}.RPP", stem.to_string_lossy()))
    };

    let options = CombineOptions { gap_measures };

    // Parse RPL or treat as single RPP
    let is_rpl = input_path
        .extension()
        .map_or(false, |ext| ext.eq_ignore_ascii_case("rpl"));

    let (combined, song_infos) = if is_rpl {
        setlist_rpp::combine_rpl(input_path, &options)?
    } else {
        // Single RPP or list of RPPs — for now just treat as RPL
        setlist_rpp::combine_rpl(input_path, &options)?
    };

    std::fs::write(&output_path, &combined)?;

    // Print summary
    println!("Combined {} songs → {}", song_infos.len(), output_path.display());
    if gap_measures > 0 {
        println!("Gap: {} measure(s) between songs", gap_measures);
    }
    println!();
    let mut total = 0.0;
    for (i, info) in song_infos.iter().enumerate() {
        println!(
            "  {:>2}. {:<40} {:>6.1}s ({:.0}:{:02.0})",
            i + 1,
            info.name,
            info.global_start_seconds,
            (info.duration_seconds / 60.0).floor(),
            info.duration_seconds % 60.0,
        );
        total = info.global_start_seconds + info.duration_seconds;
    }
    println!();
    println!(
        "Total: {:.0}:{:02.0}",
        (total / 60.0).floor(),
        total % 60.0,
    );

    Ok(())
}

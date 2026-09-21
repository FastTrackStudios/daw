//! Load an RPP project, decode its audio sources, and play it through
//! the default cpal output via `AudioEngine::attached_to` — with a
//! line-based transport prompt on stdin.
//!
//! Run with (release, so the audio callback keeps up):
//!
//! ```bash
//! cargo run --release -p daw-standalone --features rpp-loader --example play_rpp -- /path/to/song.rpp
//! ```
//!
//! Pipeline:
//! 1. `project_loader::load_rpp_via_bay` parses the file + materializes
//!    audio sources through a project-relative resolver.
//! 2. `Standalone::attach_audio_engine(guid)` builds a cpal output
//!    stream whose callback drives `ProjectRenderer` every block.
//! 3. The prompt drives the `Transport` service.
//!
//! The transport engine spawns its tasks via `architect::platform::spawn`,
//! which on native is `tokio::spawn` — so everything runs inside a tokio
//! runtime, or attaching the engine panics with "no reactor running".

use std::io::{BufRead, Write};
use std::path::PathBuf;

use daw_proto::ProjectContext;
use daw_proto::transport::service::Transport;
use daw_standalone::media_bay::ProjectRelativeResolver;
use daw_standalone::project_loader::load_rpp_via_bay;
use daw_standalone::sync::Standalone;

const HELP: &str = "\
commands:
  p | space      play / pause
  s              stop and return to start
  g <time>       go to time (seconds, or m:ss)
  + <sec> / - <sec>
                 nudge forward / back (default 5 s)
  l <a> <b>      loop between two times, and turn looping on
  l              turn looping off
  t <bpm>        set tempo
  r <rate>       set play rate (1.0 = normal)
  ?              show transport state
  h              this help
  q              quit";

fn main() -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let _guard = rt.enter();

    let path = std::env::args()
        .nth(1)
        .ok_or("usage: play_rpp <rpp-file>")?;
    let rpp_path = PathBuf::from(&path);
    let rpp_text = std::fs::read_to_string(&rpp_path).map_err(|e| e.to_string())?;
    let project_dir = rpp_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let daw = Standalone::new();
    let project_name = rpp_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("project");

    // Install a project-relative filesystem resolver on the bay.
    // RPP source paths are usually relative to the project dir;
    // `ProjectRelativeResolver` handles that uniformly. On WASM the
    // app installs a JS-backed resolver here instead.
    daw.media_bay()
        .set_file_resolver(Box::new(ProjectRelativeResolver::new(project_dir.clone())));

    println!("Loading {path}...");
    let (proj, audio) = load_rpp_via_bay(
        &daw,
        project_name,
        rpp_path.to_string_lossy().as_ref(),
        &rpp_text,
    )?;

    println!(
        "  tracks={} items={} takes={} markers={} regions={} tempo_points={} hw_outs={}",
        proj.track_count,
        proj.item_count,
        proj.take_count,
        proj.marker_count,
        proj.region_count,
        proj.tempo_point_count,
        proj.hw_output_count,
    );
    println!(
        "  decoded {} audio sources ({} failed, {} no source)",
        audio.loaded,
        audio.failed.len(),
        audio.skipped_no_source,
    );
    for (take, err) in &audio.failed {
        eprintln!("    ! {take}: {err}");
    }

    // Attach the cpal audio engine. The output callback now renders
    // the loaded project every block. Dropping `_engine` stops audio.
    let _engine = daw.attach_audio_engine(&proj.project_guid)?;
    let ctx = ProjectContext::Project(proj.project_guid.clone());

    println!("{HELP}");
    status(&daw, &ctx);
    prompt();

    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        match run_command(&daw, &ctx, &line) {
            Ok(Flow::Quit) => break,
            Ok(Flow::Continue) => {}
            Err(e) => eprintln!("  ! {e}"),
        }
        prompt();
    }

    let _ = Transport::stop(&daw, ctx);
    Ok(())
}

enum Flow {
    Continue,
    Quit,
}

fn run_command(daw: &Standalone, ctx: &ProjectContext, line: &str) -> Result<Flow, String> {
    let err = |e| format!("{e:?}");
    // A bare space is the play/pause key; everything else is trimmed.
    let (cmd, args) = if line == " " {
        ("p", Vec::new())
    } else {
        let mut words = line.split_whitespace();
        match words.next() {
            Some(cmd) => (cmd, words.collect::<Vec<_>>()),
            None => return Ok(Flow::Continue),
        }
    };
    match (cmd, args.as_slice()) {
        ("p", []) => Transport::play_pause(daw, ctx.clone()).map_err(err)?,
        ("s", []) => {
            Transport::stop(daw, ctx.clone()).map_err(err)?;
            Transport::goto_start(daw, ctx.clone()).map_err(err)?;
        }
        ("g", [t]) => Transport::set_position(daw, ctx.clone(), parse_time(t)?).map_err(err)?,
        ("+" | "-", rest) => {
            let step = match rest {
                [] => 5.0,
                [t] => parse_time(t)?,
                _ => return Err(format!("usage: {cmd} [seconds]")),
            };
            let step = if cmd == "-" { -step } else { step };
            let to = (Transport::get_position(daw, ctx.clone()) + step).max(0.0);
            Transport::set_position(daw, ctx.clone(), to).map_err(err)?;
        }
        ("l", []) => Transport::set_loop(daw, ctx.clone(), false).map_err(err)?,
        ("l", [a, b]) => {
            let (a, b) = (parse_time(a)?, parse_time(b)?);
            if b <= a {
                return Err("loop end must be after its start".into());
            }
            Transport::set_time_selection(daw, ctx.clone(), a, b).map_err(err)?;
            Transport::set_loop(daw, ctx.clone(), true).map_err(err)?;
        }
        ("t", [bpm]) => {
            let bpm = bpm.parse::<f64>().map_err(|e| e.to_string())?;
            Transport::set_tempo(daw, ctx.clone(), bpm).map_err(err)?;
        }
        ("r", [rate]) => {
            let rate = rate.parse::<f64>().map_err(|e| e.to_string())?;
            Transport::set_playrate(daw, ctx.clone(), rate).map_err(err)?;
        }
        ("?", []) => {}
        ("h", []) => println!("{HELP}"),
        ("q", []) => return Ok(Flow::Quit),
        _ => return Err(format!("unknown command `{}` (h for help)", line.trim())),
    }
    status(daw, ctx);
    Ok(Flow::Continue)
}

/// `90`, `90.5` or `1:30` → seconds.
fn parse_time(s: &str) -> Result<f64, String> {
    let bad = || format!("bad time `{s}` (use seconds or m:ss)");
    match s.split_once(':') {
        Some((m, sec)) => {
            let m = m.parse::<f64>().map_err(|_| bad())?;
            let sec = sec.parse::<f64>().map_err(|_| bad())?;
            Ok(m * 60.0 + sec)
        }
        None => s.parse::<f64>().map_err(|_| bad()),
    }
}

fn fmt_time(secs: f64) -> String {
    let m = (secs / 60.0).floor();
    format!("{}:{:06.3}", m as u64, secs - m * 60.0)
}

fn status(daw: &Standalone, ctx: &ProjectContext) {
    let pos = Transport::get_position(daw, ctx.clone());
    let state = Transport::get_play_state(daw, ctx.clone());
    let tempo = Transport::get_tempo(daw, ctx.clone());
    let rate = Transport::get_playrate(daw, ctx.clone());
    let looping = match Transport::get_time_selection(daw, ctx.clone()) {
        Some(sel) if Transport::is_looping(daw, ctx.clone()) => format!(
            "loop {}–{}",
            fmt_time(sel.start_seconds),
            fmt_time(sel.end_seconds)
        ),
        _ => "loop off".into(),
    };
    println!(
        "  {state:?}  {}  {tempo:.2} bpm  rate {rate:.2}  {looping}",
        fmt_time(pos)
    );
}

fn prompt() {
    print!("> ");
    let _ = std::io::stdout().flush();
}

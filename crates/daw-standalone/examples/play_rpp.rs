//! Load and play a REAPER project file.
//!
//! Usage:
//!   cargo run -p daw-standalone --features rpp-loader --example play_rpp -- "/path/to/project.RPP"
//!
//! Controls:
//!   SPACE = play/pause
//!   S     = stop (reset to start)
//!   LEFT  = seek back 5s
//!   RIGHT = seek forward 5s
//!   Q     = quit
//!   1-9   = toggle mute on track 1-9
//!   M     = unmute all

use daw_standalone::audio_engine::{AudioEngine, rpp_loader};
use std::io::{Read, Write, stdin, stdout};

fn main() {
    tracing_subscriber::fmt::init();

    let rpp_path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: play_rpp <path-to-rpp>");
        eprintln!("Example: play_rpp \"/Users/codywright/Music/Projects/Live Tracks/Just Friends/Vienna - Couch/Vienna - Couch.RPP\"");
        std::process::exit(1);
    });

    println!("Initializing audio engine...");
    let engine = AudioEngine::new().expect("Failed to create audio engine");

    println!("Loading RPP: {rpp_path}");
    let rpp_text = std::fs::read_to_string(&rpp_path).expect("Failed to read RPP file");
    let rpp_dir = std::path::Path::new(&rpp_path).parent().unwrap();

    let project = rpp_loader::load_rpp(&engine, &rpp_text, |file_path| {
        // Resolve relative paths against the RPP directory
        let path = if std::path::Path::new(file_path).is_absolute() {
            std::path::PathBuf::from(file_path)
        } else {
            rpp_dir.join(file_path)
        };
        std::fs::read(&path).ok()
    })
    .expect("Failed to load RPP project");

    println!("\n=== Loaded Project ===");
    println!("Sample rate: {} Hz", project.sample_rate);
    println!("Duration: {:.1}s ({:.0}:{:04.1})",
        project.duration,
        (project.duration / 60.0).floor(),
        project.duration % 60.0
    );
    println!("Tracks: {}", project.tracks.len());

    for (i, track) in project.tracks.iter().enumerate() {
        println!(
            "  [{}] {} — pos: {:.1}s, len: {:.1}s, offset: {:.1}s, file: {}",
            i + 1,
            track.track_name,
            track.position,
            track.length,
            track.source_offset,
            track.source_file
        );
    }

    if !project.failed.is_empty() {
        println!("\nFailed to load:");
        for (file, reason) in &project.failed {
            println!("  {file} — {reason}");
        }
    }

    println!("\n=== Controls ===");
    println!("  ENTER = play/pause");
    println!("  s     = stop");
    println!("  ,     = seek -5s");
    println!("  .     = seek +5s");
    println!("  1-9   = toggle mute on track");
    println!("  m     = unmute all");
    println!("  q     = quit");
    println!();

    // Start playback
    engine.play();
    println!("Playing...");

    // Set terminal to raw mode for single-char input
    enable_raw_mode();

    let mut buf = [0u8; 1];
    loop {
        // Show position
        let pos = engine.position_seconds();
        let state = if engine.is_playing() { "PLAYING" } else { "PAUSED " };
        print!("\r  {state} {:.0}:{:04.1} / {:.0}:{:04.1}  ",
            (pos / 60.0).floor(), pos % 60.0,
            (project.duration / 60.0).floor(), project.duration % 60.0
        );
        stdout().flush().ok();

        // Non-blocking read with timeout
        if stdin().read(&mut buf).is_ok() {
            match buf[0] {
                b'q' | 3 => break, // q or Ctrl+C
                b'\n' | b'\r' | b' ' => {
                    if engine.is_playing() {
                        engine.pause();
                    } else {
                        engine.play();
                    }
                }
                b's' => {
                    engine.stop();
                    println!("\r  Stopped.                              ");
                }
                b',' => {
                    let pos = engine.position_seconds();
                    engine.seek((pos - 5.0).max(0.0));
                }
                b'.' => {
                    let pos = engine.position_seconds();
                    engine.seek(pos + 5.0);
                }
                b'm' => {
                    for track in &project.tracks {
                        engine.set_track_muted(track.handle, false);
                    }
                    println!("\r  Unmuted all tracks.                   ");
                }
                c @ b'1'..=b'9' => {
                    let idx = (c - b'1') as usize;
                    if let Some(track) = project.tracks.get(idx) {
                        // Toggle mute (we don't track state here, just toggle via gain)
                        let current = engine.track_gain(track.handle);
                        if current > 0.0 {
                            engine.set_track_muted(track.handle, true);
                            println!("\r  Muted: {}                             ", track.track_name);
                        } else {
                            engine.set_track_muted(track.handle, false);
                            println!("\r  Unmuted: {}                           ", track.track_name);
                        }
                    }
                }
                _ => {}
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    disable_raw_mode();
    println!("\nDone.");
}

// Simple terminal raw mode for macOS/Linux
fn enable_raw_mode() {
    #[cfg(unix)]
    {
        use std::process::Command;
        Command::new("stty").args(["-icanon", "-echo", "min", "0", "time", "1"])
            .status().ok();
    }
}

fn disable_raw_mode() {
    #[cfg(unix)]
    {
        use std::process::Command;
        Command::new("stty").args(["icanon", "echo"])
            .status().ok();
    }
}

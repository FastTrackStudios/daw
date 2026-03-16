//! Real-world setlist combination test.
//!
//! Combines the Battle SP26 JF Tracks RPL into a setlist + shell copies.
//! This test uses actual production RPP files and writes the output to
//! the Z - SETLISTS directory.
//!
//! Run with:
//!   cargo test -p dawfile-reaper --test setlist_real_rpl -- --nocapture

use dawfile_reaper::io::read_project;
use dawfile_reaper::setlist_rpp::{
    self, build_song_infos_from_projects, concatenate_projects, measures_to_seconds,
    project_to_rpp_text, resolve_song_bounds, write_role_setlists, STANDARD_ROLES,
};
use std::path::PathBuf;

const RPL_PATH: &str = "/Users/codywright/Downloads/Battle SP26 JF Tracks.RPL";
const OUTPUT_DIR: &str = "/Users/codywright/Music/Projects/Live Tracks/Z - SETLISTS/Just Friends Battle of the Bands SP26";
const SETLIST_NAME: &str = "Just Friends Battle of the Bands SP26";

#[test]
fn combine_battle_sp26_rpl() {
    // Skip if the RPL doesn't exist (CI environment)
    if !std::path::Path::new(RPL_PATH).exists() {
        println!("Skipping: RPL file not found at {}", RPL_PATH);
        return;
    }

    // ── 1. Parse RPL and read all projects ───────────────────────────
    let rpp_paths = setlist_rpp::parse_rpl(RPL_PATH.as_ref()).unwrap();
    println!("\n═══ BATTLE SP26 SETLIST COMBINATION ═══\n");
    println!("RPL: {} ({} songs)", RPL_PATH, rpp_paths.len());

    let mut projects = Vec::new();
    let mut names = Vec::new();

    for path in &rpp_paths {
        let name = setlist_rpp::song_name_from_path(path);
        println!("  Parsing: {} ...", name);
        match read_project(path) {
            Ok(project) => {
                let bounds = resolve_song_bounds(&project);
                let tempo = project.tempo_envelope.as_ref()
                    .map(|e| e.default_tempo)
                    .unwrap_or(120.0);
                println!("    ✓ {} tracks, {:.0} BPM, bounds {:.1}→{:.1}s ({:.1}s)",
                    project.tracks.len(), tempo,
                    bounds.start, bounds.end, bounds.end - bounds.start);
                projects.push(project);
                names.push(name);
            }
            Err(e) => {
                println!("    ✗ Failed: {}", e);
            }
        }
    }

    assert!(!projects.is_empty(), "Should parse at least one project");
    println!("\nParsed {}/{} projects successfully", projects.len(), rpp_paths.len());

    // ── 2. Build song infos with 2-measure gap at 120 BPM ───────────
    let gap = measures_to_seconds(16, 120.0, 4);
    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let song_infos = build_song_infos_from_projects(&projects, &name_refs, gap);

    println!("\nSong layout:");
    for si in &song_infos {
        println!("  {:<40} @ {:>6.1}s  ({:.1}s)", si.name, si.global_start_seconds, si.duration_seconds);
    }

    let total = song_infos.last()
        .map(|s| s.global_start_seconds + s.duration_seconds)
        .unwrap_or(0.0);
    println!("\nTotal timeline: {:.1}s ({:.1} minutes)", total, total / 60.0);

    // ── 3. Concatenate into combined setlist ─────────────────────────
    let combined = concatenate_projects(&projects, &song_infos);

    println!("\nCombined project:");
    println!("  Tracks: {}", combined.tracks.len());
    println!("  Tempo points: {}", combined.tempo_envelope.as_ref().map_or(0, |e| e.points.len()));
    println!("  Markers/regions: {}", combined.markers_regions.all.len());

    // Print track structure
    println!("\nTrack structure:");
    for (i, t) in combined.tracks.iter().enumerate() {
        let folder = t.folder.as_ref()
            .map(|f| format!(" [{:?}]", f.folder_state))
            .unwrap_or_default();
        let items = if t.items.is_empty() { String::new() } else { format!(" ({} items)", t.items.len()) };
        println!("  {:>2}. {}{}{}", i, t.name, folder, items);
    }

    // ── 4. Write to output directory ─────────────────────────────────
    println!("\nWriting to: {}", OUTPUT_DIR);

    let output_dir = PathBuf::from(OUTPUT_DIR);
    // Only generate the roles we actually need for this setlist
    let roles: &[&str] = &["Vocals", "Guitar", "Keys", "Drum Enhancement"];

    let paths = write_role_setlists(&combined, roles, SETLIST_NAME, &output_dir)
        .expect("Failed to write role setlists");

    println!("\nGenerated {} files:", paths.len());
    for path in &paths {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        println!("  {} ({:.1} KB)", path.file_name().unwrap().to_string_lossy(), size as f64 / 1024.0);
    }

    // ── 5. Verify shell copies have correct structure ────────────────
    println!("\nVerifying shell copies:");
    for role in roles {
        let filename = format!("{} - {}.RPP", role, SETLIST_NAME);
        let path = output_dir.join(&filename);
        assert!(path.exists(), "Missing: {}", filename);

        let shell_text = std::fs::read_to_string(&path).unwrap();
        // Basic sanity checks
        assert!(shell_text.contains("TEMPOENVEX"), "{} should have tempo envelope", role);
        assert!(shell_text.contains("RULERLANE"), "{} should have ruler lanes", role);
        assert!(shell_text.contains(&format!("NAME {:?}", role)), "{} should have role folder", role);
        println!("  {} ✓", role);
    }

    // Verify master
    let master_filename = format!("Tracks - {}.RPP", SETLIST_NAME);
    let master_path = output_dir.join(&master_filename);
    assert!(master_path.exists(), "Missing master: {}", master_filename);
    println!("  Tracks (master) ✓");

    println!("\n═══ BATTLE SP26 SETLIST GENERATED SUCCESSFULLY ═══\n");
}

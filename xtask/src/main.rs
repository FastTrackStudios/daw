use clap::{Parser, Subcommand};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;

/// DAW workspace developer tasks.
#[derive(Parser)]
#[command(name = "daw-xtask")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Set up the FTS-TESTING.app REAPER bundle for integration tests.
    SetupTestBundles {
        /// Recreate bundles even if they already exist.
        #[arg(long)]
        force: bool,
    },
    /// Run REAPER integration tests (spawns REAPER, runs #[reaper_test] tests).
    ReaperTest {
        /// Specific test name filter (passed to cargo test as filter).
        filter: Option<String>,
        /// Keep REAPER open after tests complete (for inspecting results).
        #[arg(long)]
        keep_open: bool,
    },
    /// Set up REAPER instance rigs (desktop entries, icons, wrapper scripts).
    ///
    /// On Linux: generates .desktop files, wrapper scripts, icons, and launch.json configs.
    /// On macOS: generates wrapper .app bundles (delegates to setup-test-bundles).
    SetupRigs {
        /// Recreate all rigs even if they already exist.
        #[arg(long)]
        force: bool,
    },
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

fn reaper_dir() -> String {
    format!("{}/Reaper", fts_home())
}

fn reaper_exe() -> String {
    format!("{}/FTS-LIVE.app/Contents/MacOS/REAPER", reaper_dir())
}

fn reaper_resources() -> String {
    format!("{}/FTS-LIVE.app/Contents/Resources", reaper_dir())
}

fn reaper_ini() -> String {
    format!("{}/reaper.ini", reaper_dir())
}

fn default_theme() -> String {
    format!("{}/ColorThemes/Default_7.0.ReaperThemeZip", reaper_dir())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::SetupTestBundles { force } => setup_test_bundles(force),
        Cmd::ReaperTest { filter, keep_open } => reaper_test(filter, keep_open),
        Cmd::SetupRigs { force } => setup_rigs(force),
    }
}

struct BundleSpec {
    app_name: &'static str,
    role: &'static str,
    bundle_id: &'static str,
}

const BUNDLES: &[BundleSpec] = &[BundleSpec {
    app_name: "FTS-TESTING",
    role: "testing",
    bundle_id: "com.fasttrackstudio.testing",
}];

fn setup_test_bundles(force: bool) -> Result<(), Box<dyn std::error::Error>> {
    let reaper_dir = reaper_dir();
    println!("Setting up REAPER test bundles in {reaper_dir}");

    // Build reaper-launcher
    print!("  Building reaper-launcher...");
    let status = Command::new("cargo")
        .args([
            "build",
            "-p",
            "reaper-launcher",
            "--release",
            "--bin",
            "reaper-launcher",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()?;
    if !status.success() {
        return Err("Failed to build reaper-launcher".into());
    }
    println!(" OK");

    // Find the built binary
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let launcher_bin = workspace_root.join("target/release/reaper-launcher");
    if !launcher_bin.exists() {
        return Err(format!("Launcher binary not found: {}", launcher_bin.display()).into());
    }

    // Timestamp-based version busts macOS icon cache
    let version = format!(
        "1.0.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );

    let base_dir = PathBuf::from(&reaper_dir);

    for spec in BUNDLES {
        let bundle_dir = base_dir.join(format!("{}.app", spec.app_name));
        let contents_dir = bundle_dir.join("Contents");
        let macos_dir = contents_dir.join("MacOS");
        let resources_dir = contents_dir.join("Resources");
        let wrapper_exe = macos_dir.join("REAPER");
        let plist_path = contents_dir.join("Info.plist");

        if !force && wrapper_exe.exists() && plist_path.exists() {
            println!(
                "  SKIP {}.app (already exists, use --force to recreate)",
                spec.app_name
            );
            continue;
        }

        print!("  {}.app ...", spec.app_name);

        if force && bundle_dir.exists() {
            std::fs::remove_dir_all(&bundle_dir)?;
        }

        // Create directory structure
        std::fs::create_dir_all(&macos_dir)?;
        std::fs::create_dir_all(&resources_dir)?;

        // Write launch.json
        let launch_config = reaper_launcher::LaunchConfig {
            role: spec.role.to_string(),
            rig_type: None,
            reaper_executable: reaper_exe(),
            resources_dir: reaper_resources(),
            ini_path: reaper_ini(),
            ini_overrides: reaper_launcher::ReaperIniConfig {
                undo_max_mem: Some(0), // 0 disables undo and save prompts
                theme: Some(default_theme()),
            },
            restore_ini_after_launch: false,
            reaper_args: reaper_launcher::LaunchConfig::standard_reaper_args(),
        };
        launch_config
            .save(&contents_dir.join("launch.json"))
            .map_err(|e| format!("Failed to write launch.json: {e}"))?;

        // Copy reaper-launcher binary
        std::fs::copy(&launcher_bin, &wrapper_exe).map_err(|e| {
            format!(
                "Failed to copy launcher binary into {}.app: {e}",
                spec.app_name
            )
        })?;

        // Write Info.plist
        let app_name = spec.app_name;
        let bundle_id = spec.bundle_id;
        let plist_content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>{app_name}</string>
    <key>CFBundleDisplayName</key>
    <string>{app_name}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleExecutable</key>
    <string>REAPER</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>LSUIElement</key>
    <false/>
    <key>CFBundleIconFile</key>
    <string>main-mac</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>"#
        );
        std::fs::write(&plist_path, plist_content)?;

        // Ad-hoc sign so macOS doesn't block it
        let _ = Command::new("codesign")
            .args(["--force", "--sign", "-"])
            .arg(&bundle_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        // Re-register with LaunchServices
        let _ = Command::new(
            "/System/Library/Frameworks/CoreServices.framework/\
             Frameworks/LaunchServices.framework/Support/lsregister",
        )
        .args(["-f"])
        .arg(&bundle_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

        println!(" OK");
    }

    println!("\nDone. FTS-TESTING.app is ready in {reaper_dir}");

    Ok(())
}

// ============================================================================
// setup-rigs: Generate cross-platform REAPER instance launchers
// ============================================================================

/// A rig specification for generating launcher infrastructure.
struct RigSpec {
    /// Unique identifier, used in file paths and desktop entry IDs.
    id: &'static str,
    /// Human-readable name shown in desktop menus.
    name: &'static str,
    /// One-line description.
    comment: &'static str,
    /// DAW role (e.g., "testing", "signal", "session").
    role: &'static str,
    /// Optional rig type (e.g., "guitar", "bass").
    rig_type: Option<&'static str>,
    /// Badge color RGB.
    color: (u8, u8, u8),
    /// Badge text on the icon.
    badge: &'static str,
}

const RIGS: &[RigSpec] = &[
    RigSpec {
        id: "fts-daw-test",
        name: "DAW Test",
        comment: "REAPER integration test instance (daw repo)",
        role: "testing",
        rig_type: None,
        color: (0x4d, 0x4d, 0x4d),
        badge: "TEST",
    },
    RigSpec {
        id: "fts-daw-secondary",
        name: "DAW Secondary",
        comment: "Secondary REAPER test instance for multi-role integration tests",
        role: "secondary",
        rig_type: None,
        color: (0x1e, 0x40, 0xaf),
        badge: "SEC",
    },
];

fn setup_rigs(force: bool) -> Result<(), Box<dyn std::error::Error>> {
    // On macOS, delegate to setup_test_bundles (existing .app bundle workflow)
    if cfg!(target_os = "macos") {
        println!("macOS detected — delegating to setup-test-bundles");
        return setup_test_bundles(force);
    }

    // ── Linux: generate desktop entries, wrapper scripts, icons ──────────

    // Resolve REAPER paths from environment (set by devenv/nix)
    let reaper_executable = std::env::var("FTS_REAPER_EXECUTABLE")
        .unwrap_or_else(|_| which_command("reaper").unwrap_or_else(|| "reaper".to_string()));
    let reaper_resources = std::env::var("FTS_REAPER_RESOURCES").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{home}/.config/REAPER")
    });
    let ini_path = std::env::var("FTS_REAPER_INI")
        .unwrap_or_else(|_| format!("{reaper_resources}/reaper.ini"));

    println!("Setting up REAPER rigs (Linux)");
    println!("  REAPER executable: {reaper_executable}");
    println!("  Resources dir:     {reaper_resources}");
    println!("  INI path:          {ini_path}");

    // Build reaper-launcher binary
    print!("  Building reaper-launcher...");
    let status = Command::new("cargo")
        .args([
            "build",
            "-p",
            "reaper-launcher",
            "--release",
            "--bin",
            "reaper-launcher",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()?;
    if !status.success() {
        return Err("Failed to build reaper-launcher".into());
    }
    println!(" OK");

    let home = std::env::var("HOME").map_err(|_| "HOME not set")?;

    // Install reaper-launcher binary to ~/.local/bin/ so wrapper scripts can exec it
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let launcher_src = workspace_root.join("target/release/reaper-launcher");
    let local_bin = PathBuf::from(&home).join(".local/bin");
    std::fs::create_dir_all(&local_bin)?;
    let launcher_dst = local_bin.join("reaper-launcher");
    std::fs::copy(&launcher_src, &launcher_dst)
        .map_err(|e| format!("Failed to install reaper-launcher to ~/.local/bin: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&launcher_dst, std::fs::Permissions::from_mode(0o755));
    }
    println!("  reaper-launcher → {}", launcher_dst.display());

    for rig in RIGS {
        println!("\n  {} ({})", rig.name, rig.id);

        // Each rig gets its own resources_dir with a dedicated reaper.ini.
        // UserPlugins is symlinked from the shared REAPER install so extensions
        // are shared, but per-rig ini settings (like undomaxmem=0) are isolated.
        let rig_resources =
            PathBuf::from(&home).join(format!(".config/fts/rigs/{}/resources", rig.id));
        std::fs::create_dir_all(&rig_resources)?;

        // Symlink UserPlugins from shared REAPER install if not already present
        let rig_user_plugins = rig_resources.join("UserPlugins");
        let shared_user_plugins = PathBuf::from(&reaper_resources).join("UserPlugins");
        if !rig_user_plugins.exists() && shared_user_plugins.exists() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&shared_user_plugins, &rig_user_plugins)?;
        }

        // Write rig-specific reaper.ini with undomaxmem=0
        let rig_ini = rig_resources.join("reaper.ini");
        if !rig_ini.exists() {
            // Seed from shared ini if it exists, otherwise create minimal
            if PathBuf::from(&ini_path).exists() {
                std::fs::copy(&ini_path, &rig_ini)?;
            } else {
                std::fs::write(&rig_ini, "[reaper]\n")?;
            }
        }
        // Patch undomaxmem=0 into rig ini
        {
            let content = std::fs::read_to_string(&rig_ini)?;
            if !content.contains("undomaxmem=") {
                let patched = content.replace("[reaper]\n", "[reaper]\nundomaxmem=0\n");
                std::fs::write(&rig_ini, patched)?;
            } else {
                // Update existing value
                let patched = content
                    .lines()
                    .map(|l| {
                        if l.starts_with("undomaxmem=") {
                            "undomaxmem=0"
                        } else {
                            l
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                std::fs::write(&rig_ini, patched)?;
            }
        }

        let rig_resources_str = rig_resources.to_string_lossy().to_string();
        let rig_ini_str = rig_ini.to_string_lossy().to_string();

        // 1. Install launch.json
        let launch_config = reaper_launcher::LaunchConfig {
            role: rig.role.to_string(),
            rig_type: rig.rig_type.map(|s| s.to_string()),
            reaper_executable: reaper_executable.clone(),
            resources_dir: rig_resources_str.clone(),
            ini_path: rig_ini_str.clone(),
            ini_overrides: reaper_launcher::ReaperIniConfig {
                undo_max_mem: Some(0), // 0 disables undo and save-on-close prompts
                ..Default::default()
            },
            restore_ini_after_launch: false,
            reaper_args: reaper_launcher::LaunchConfig::standard_reaper_args(),
        };

        let config_path = reaper_launcher::desktop::install_launch_config(rig.id, &launch_config)?;
        println!("    launch.json  → {}", config_path.display());

        // 2. Install wrapper script
        let fhs_wrapper = std::env::var("FTS_FHS_WRAPPER").ok();
        let script = reaper_launcher::desktop::generate_wrapper_script(
            &config_path,
            &launcher_dst,
            fhs_wrapper.as_deref(),
        );
        let script_path = reaper_launcher::desktop::install_wrapper_script(rig.id, &script)?;
        println!("    wrapper      → {}", script_path.display());

        // 3. Generate and install icons
        let icon_config = reaper_launcher::icon_gen::IconConfig {
            badge_text: rig.badge.to_string(),
            color: rig.color,
            sizes: vec![48, 128, 256],
        };
        reaper_launcher::icon_gen::generate_and_install_icons(rig.id, &icon_config)?;
        println!(
            "    icons        → ~/.local/share/icons/hicolor/{{48,128,256}}x*/apps/{}.png",
            rig.id
        );

        // 4. Install .desktop file
        let desktop_config = reaper_launcher::desktop::DesktopEntryConfig {
            id: rig.id.to_string(),
            name: rig.name.to_string(),
            comment: rig.comment.to_string(),
            icon_name: rig.id.to_string(),
            exec_command: format!("{home}/.local/bin/{} %F", rig.id),
            categories: "AudioVideo;Audio;".to_string(),
            keywords: vec![
                "reaper".to_string(),
                "daw".to_string(),
                rig.role.to_string(),
                "fasttrackstudio".to_string(),
            ],
        };
        let desktop_path = reaper_launcher::desktop::install_desktop_entry(&desktop_config)?;
        println!("    .desktop     → {}", desktop_path.display());
    }

    // Refresh desktop database
    reaper_launcher::desktop::refresh_desktop_database();
    println!("\nDone. Rigs are ready.");

    Ok(())
}

// ============================================================================
// reaper-test: Run REAPER integration tests
// ============================================================================

/// Load the daw-test rig's launch.json if it exists.
/// Returns the config or None if not set up yet.
fn load_daw_test_rig() -> Option<reaper_launcher::LaunchConfig> {
    let home = std::env::var("HOME").ok()?;
    let config_path = PathBuf::from(format!("{home}/.config/fts/rigs/fts-daw-test/launch.json"));
    reaper_launcher::LaunchConfig::load(&config_path).ok()
}

fn reaper_test(filter: Option<String>, keep_open: bool) -> Result<(), Box<dyn std::error::Error>> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let ci = std::env::var("CI").is_ok();
    let timeout_secs: u64 = std::env::var("REAPER_TEST_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let ext_log = PathBuf::from("/tmp/fts-daw-test.log");

    // ── Rig config ─────────────────────────────────────────────────────────
    let rig_config = load_daw_test_rig();
    section(ci, "reaper-test: rig");
    if rig_config.is_some() {
        println!("  rig: fts-daw-test (~/.config/fts/rigs/fts-daw-test/launch.json)");
    } else {
        println!("  WARNING: fts-daw-test rig not found — run `cargo xtask setup-rigs`");
        println!("  Falling back to legacy test config");
    }

    // ── Step 1: Build the test extension ──────────────────────────────────
    section(ci, "reaper-test: build extension");
    println!("Building daw-test-extension...");
    let status = Command::new("cargo")
        .args(["build", "-p", "daw-test-extension"])
        .status()?;
    if !status.success() {
        return Err("Failed to build daw-test-extension".into());
    }
    end_section(ci);

    // ── Step 1b: Build daw-guest-example ────────────────────────────────
    section(ci, "reaper-test: build guest example");
    println!("Building daw-guest-example...");
    let status = Command::new("cargo")
        .args(["build", "-p", "daw-guest-example"])
        .status()?;
    if !status.success() {
        return Err("Failed to build daw-guest-example".into());
    }
    end_section(ci);

    // ── Step 2: Build test binaries (no-run) ──────────────────────────────
    section(ci, "reaper-test: build test binaries");
    println!("Building test binaries...");
    let status = Command::new("cargo")
        .args(["test", "-p", "daw-reaper", "--no-run"])
        .status()?;
    if !status.success() {
        return Err("Failed to build daw-reaper test binaries".into());
    }
    end_section(ci);

    // ── Step 3: Install the .so into REAPER's UserPlugins dir ─────────────
    section(ci, "reaper-test: install plugin");
    let user_plugins_dir = if let Some(ref rig) = rig_config {
        PathBuf::from(&rig.resources_dir).join("UserPlugins")
    } else {
        // No rig config — use the path that fts-test's reaper-headless script creates.
        // reaper-headless sets REAPER_CONFIG="${HOME}/.config/REAPER" and creates
        // UserPlugins there, so that's where REAPER will scan for extensions.
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(format!("{home}/.config/REAPER/UserPlugins"))
    };

    // REAPER expects "reaper_" prefix (not "lib" prefix) in UserPlugins.
    let so_src_name = "libreaper_daw_test.so";
    let so_dst_name = "reaper_daw_test.so";
    let so_path = workspace_root.join("target/debug").join(so_src_name);
    if !so_path.exists() {
        let dylib_src_name = "libreaper_daw_test.dylib";
        let dylib_dst_name = "reaper_daw_test.dylib";
        let dylib_path = workspace_root.join("target/debug").join(dylib_src_name);
        if dylib_path.exists() {
            install_plugin(&dylib_path, dylib_dst_name, &user_plugins_dir)?;
        } else {
            return Err(format!(
                "Built library not found at {} or {}",
                so_path.display(),
                dylib_path.display()
            )
            .into());
        }
    } else {
        install_plugin(&so_path, so_dst_name, &user_plugins_dir)?;
    }
    end_section(ci);

    // ── Step 3b: Install daw-guest into fts-extensions/ ──────────────────
    section(ci, "reaper-test: install guest extensions");
    let fts_ext_dir = user_plugins_dir.join("fts-extensions");
    std::fs::create_dir_all(&fts_ext_dir)?;
    let guest_src = workspace_root.join("target/debug/daw-guest");
    if guest_src.exists() {
        let guest_dst = fts_ext_dir.join("daw-guest");
        std::fs::copy(&guest_src, &guest_dst)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&guest_dst, std::fs::Permissions::from_mode(0o755))?;
        }
        println!("  Installed daw-guest -> {}", guest_dst.display());
    } else {
        println!(
            "  WARNING: daw-guest binary not found at {}",
            guest_src.display()
        );
    }
    end_section(ci);

    // ── Step 4: Clean stale sockets and log ───────────────────────────────
    if let Ok(entries) = std::fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("fts-daw-") && name.ends_with(".sock") {
                    let _ = std::fs::remove_file(&path);
                    println!("  Removed stale socket: {name}");
                } else if name.starts_with("fts-daw-") && name.ends_with(".bootstrap.sock") {
                    let _ = std::fs::remove_file(&path);
                    println!("  Removed stale bootstrap socket: {name}");
                }
            }
        }
    }
    let _ = std::fs::remove_file(&ext_log); // remove stale log from previous run

    // ── Step 5: Spawn REAPER ──────────────────────────────────────────────
    section(ci, "reaper-test: spawn REAPER");
    let home = std::env::var("HOME").unwrap_or_default();
    let daw_test_wrapper = format!("{home}/.local/bin/fts-daw-test");
    let reaper_log = PathBuf::from("/tmp/fts-daw-reaper.log");

    let (reaper_exe, pass_reaper_args) = if PathBuf::from(&daw_test_wrapper).exists() {
        (daw_test_wrapper, false)
    } else if let Ok(exe) = std::env::var("FTS_REAPER_EXECUTABLE") {
        (exe, true)
    } else {
        ("reaper".to_string(), true)
    };

    let fts_test = find_fts_test();
    let needs_fhs = std::env::var("DISPLAY").map_or(true, |d| d.is_empty());

    println!("  exe:         {reaper_exe}");
    println!("  pass args:   {pass_reaper_args}");
    println!("  needs fhs:   {needs_fhs}");
    if let Some(ref fts) = fts_test {
        println!("  fts-test:    {fts}");
    } else if needs_fhs {
        println!("  WARNING: no fts-test found and no DISPLAY — REAPER may fail");
    }
    println!("  timeout:     {timeout_secs}s (REAPER_TEST_TIMEOUT_SECS)");
    println!("  logs:");
    println!("    REAPER process → {}", reaper_log.display());
    println!("    extension      → {}", ext_log.display());

    // Redirect REAPER stdout/stderr to its own log file (keeps LV2 spam off CI output)
    let reaper_log_file = std::fs::File::create(&reaper_log)
        .map_err(|e| format!("Failed to create REAPER log {}: {e}", reaper_log.display()))?;
    let reaper_log_stderr = reaper_log_file.try_clone()?;

    let mut reaper_child = if needs_fhs {
        if let Some(ref fts) = fts_test {
            let mut cmd = Command::new(fts);
            cmd.arg(&reaper_exe);
            if pass_reaper_args {
                cmd.args(["-newinst", "-nosplash", "-ignoreerrors"]);
            }
            cmd.stdout(reaper_log_file).stderr(reaper_log_stderr);
            println!("  spawning: {fts} {reaper_exe}");
            cmd.spawn()
                .map_err(|e| format!("Failed to spawn via fts-test: {e}"))?
        } else {
            let mut cmd = Command::new(&reaper_exe);
            if pass_reaper_args {
                cmd.args(["-newinst", "-nosplash", "-ignoreerrors"]);
            }
            cmd.stdout(reaper_log_file).stderr(reaper_log_stderr);
            println!("  spawning: {reaper_exe} (no fhs wrapper)");
            cmd.spawn()
                .map_err(|e| format!("Failed to spawn REAPER: {e}"))?
        }
    } else {
        let mut cmd = Command::new(&reaper_exe);
        if pass_reaper_args {
            cmd.args(["-newinst", "-nosplash", "-ignoreerrors"]);
        }
        cmd.stdout(reaper_log_file).stderr(reaper_log_stderr);
        println!("  spawning: {reaper_exe}");
        cmd.spawn()
            .map_err(|e| format!("Failed to spawn REAPER: {e}"))?
    };

    let reaper_pid = reaper_child.id();
    println!("  spawned PID: {reaper_pid}");

    // ── Step 6: Wait for socket, tail extension log ───────────────────────
    section(ci, "reaper-test: waiting for REAPER ready");
    println!("  Waiting up to {timeout_secs}s for fts-daw-*.sock …");
    println!("  Extension log: {}", ext_log.display());

    let start = std::time::Instant::now();
    let deadline = start + std::time::Duration::from_secs(timeout_secs);

    // Background thread: tail the extension log and print new lines as they appear
    let ext_log_clone = ext_log.clone();
    let _log_tailer = std::thread::spawn(move || {
        let mut pos = 0u64;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            if std::time::Instant::now() > deadline {
                break;
            }
            if let Ok(mut f) = std::fs::File::open(&ext_log_clone) {
                if f.seek(SeekFrom::Start(pos)).is_ok() {
                    let reader = BufReader::new(&f);
                    for line in reader.lines().map_while(|l| l.ok()) {
                        println!("  [ext] {line}");
                        pos += line.len() as u64 + 1; // +1 for newline
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    });

    let socket_path = loop {
        // Check if REAPER process died unexpectedly
        match reaper_child.try_wait() {
            Ok(Some(status)) => {
                println!("\n  REAPER exited early with status: {status}");
                dump_log_on_failure(&ext_log, "extension");
                dump_log_on_failure(&reaper_log, "REAPER process");
                return Err("REAPER process exited before socket was created".into());
            }
            Ok(None) => {} // still running
            Err(e) => println!("  Warning: could not check REAPER status: {e}"),
        }

        if let Some(sock) = find_fts_daw_socket() {
            let elapsed = start.elapsed().as_secs_f32();
            println!("\n  Socket ready after {elapsed:.1}s: {sock}");
            break sock;
        }

        if std::time::Instant::now() > deadline {
            let elapsed = start.elapsed().as_secs();
            println!("\n  Timed out after {elapsed}s");
            let _ = reaper_child.kill();
            let _ = reaper_child.wait();
            dump_log_on_failure(&ext_log, "extension");
            dump_log_on_failure(&reaper_log, "REAPER process");
            list_tmp_sockets();
            return Err(
                format!("Timed out after {timeout_secs}s waiting for fts-daw-*.sock").into(),
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(300));
        let elapsed = start.elapsed().as_secs();
        if elapsed % 5 == 0 && elapsed > 0 {
            print!("\r  [{elapsed}s] waiting …   ");
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
    };

    // Brief pause to let the listener fully bind
    std::thread::sleep(std::time::Duration::from_millis(500));
    end_section(ci);

    // ── Step 7: Run tests ─────────────────────────────────────────────────
    section(ci, "reaper-test: run tests");
    println!("Running cargo test -p daw-reaper …");
    let mut test_cmd = Command::new("cargo");
    test_cmd.args([
        "test",
        "-p",
        "daw-reaper",
        "--",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]);
    if let Some(ref f) = filter {
        test_cmd.arg(f);
    }
    test_cmd.env("FTS_SOCKET", &socket_path);
    if keep_open {
        test_cmd.env("FTS_KEEP_OPEN", "1");
    }

    let mut test_child = test_cmd.spawn()?;
    let test_timeout = std::time::Duration::from_secs(60);
    let test_start = std::time::Instant::now();

    // Poll for test completion with a 60s timeout.
    // We must kill REAPER *before* waiting for the test process to exit,
    // because the test binary's shared tokio Runtime holds a ROAM driver
    // task connected to REAPER — it won't shut down until the connection
    // closes (i.e. REAPER dies).
    let tests_passed = loop {
        match test_child.try_wait()? {
            Some(status) => break status.success(),
            None if test_start.elapsed() > test_timeout => {
                println!("Test process did not exit within 60s — killing it");
                let _ = test_child.kill();
                let _ = test_child.wait();
                break false;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(200)),
        }
    };
    end_section(ci);

    // ── Step 8: Kill REAPER (unless --keep-open) ──────────────────────────
    if keep_open {
        println!("REAPER left running (PID {reaper_pid}) — kill manually when done");
    } else {
        println!("Killing REAPER (PID {reaper_pid})…");
        let _ = reaper_child.kill();
        let _ = reaper_child.wait();
        let _ = std::fs::remove_file(&socket_path);

        // The test process may still be alive (blocked on Runtime drop waiting
        // for the ROAM driver). Now that REAPER is dead the socket will EOF
        // and the driver should exit. Give it a few seconds then force-kill.
        for _ in 0..20 {
            if test_child.try_wait()?.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        if test_child.try_wait()?.is_none() {
            println!("Test process still alive after REAPER killed — force killing");
            let _ = test_child.kill();
            let _ = test_child.wait();
        }
    }

    // ── Step 9: Report results ────────────────────────────────────────────
    if !tests_passed {
        dump_log_on_failure(&ext_log, "extension");
        dump_log_on_failure(&reaper_log, "REAPER process");
        println!("Per-test logs: /tmp/reaper-tests/");
        return Err("Some tests failed".into());
    }

    println!("\nAll tests passed!");
    Ok(())
}

/// Install (symlink) the plugin library into the given UserPlugins directory.
fn install_plugin(
    lib_path: &Path,
    lib_name: &str,
    user_plugins_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(user_plugins_dir)?;

    let dest = user_plugins_dir.join(lib_name);

    // Remove existing symlink/file
    let _ = std::fs::remove_file(&dest);

    // Create symlink
    #[cfg(unix)]
    std::os::unix::fs::symlink(lib_path, &dest)?;
    #[cfg(not(unix))]
    std::fs::copy(lib_path, &dest)?;

    println!("  Installed {} -> {}", dest.display(), lib_path.display());
    Ok(())
}

/// Scan /tmp for any fts-daw-*.sock file and return its path as a String.
fn find_fts_daw_socket() -> Option<String> {
    let entries = std::fs::read_dir("/tmp").ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("fts-daw-") && name.ends_with(".sock") {
                return Some(path.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// Find a command on PATH.
fn which_command(name: &str) -> Option<String> {
    Command::new("which").arg(name).output().ok().and_then(|o| {
        if o.status.success() {
            String::from_utf8(o.stdout)
                .ok()
                .map(|s| s.trim().to_string())
        } else {
            None
        }
    })
}

/// Print a section header. In CI (GitHub Actions) emits `::group::` for collapsible logs.
fn section(ci: bool, name: &str) {
    if ci {
        println!("::group::{name}");
    } else {
        println!("\n── {name} ──");
    }
}

/// End a section. In CI emits `::endgroup::`.
fn end_section(ci: bool) {
    if ci {
        println!("::endgroup::");
    }
}

/// Dump the extension log to stdout (called on failure).
fn dump_log_on_failure(log_path: &Path, label: &str) {
    if let Ok(content) = std::fs::read_to_string(log_path) {
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        const MAX_TAIL: usize = 80;
        let start = total.saturating_sub(MAX_TAIL);
        println!(
            "\n── {} log: {} ({} lines{})",
            label,
            log_path.display(),
            total,
            if start > 0 {
                format!(", showing last {MAX_TAIL}")
            } else {
                String::new()
            }
        );
        for line in &lines[start..] {
            println!("  {line}");
        }
    } else {
        println!("  (no {} log at {})", label, log_path.display());
    }
}

/// List any fts-daw-*.sock files currently in /tmp (diagnostic helper).
fn list_tmp_sockets() {
    println!("  fts-daw-*.sock files in /tmp:");
    if let Ok(entries) = std::fs::read_dir("/tmp") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) {
                if name.starts_with("fts-daw-") && name.ends_with(".sock") {
                    println!("    {name}");
                }
            }
        }
    }
}

/// Find the `fts-test` launcher (Xvfb + FHS wrapper).
/// Checks PATH first, then the local devenv profile, then system nix locations.
fn find_fts_test() -> Option<String> {
    if let Some(p) = which_command("fts-test") {
        return Some(p);
    }

    // Stable devenv profile symlink — works without entering the shell
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let devenv_fts = workspace_root.join(".devenv/profile/bin/fts-test");
    if devenv_fts.exists() {
        return Some(devenv_fts.to_string_lossy().into_owned());
    }

    // System/user nix profile fallbacks
    let candidates = [
        "/run/current-system/sw/bin/fts-test",
        "/nix/var/nix/profiles/default/bin/fts-test",
    ];
    candidates
        .iter()
        .find(|p| PathBuf::from(p).exists())
        .map(|s| s.to_string())
}

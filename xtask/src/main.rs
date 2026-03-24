use clap::{Parser, Subcommand};
use reaper_test::runner::{self, TestPackage, TestRunner};
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

/// Canonical REAPER resources directory — delegates to `runner::fts_reaper_resources`.
fn fts_reaper_resources() -> PathBuf {
    runner::fts_reaper_resources()
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
    let reaper_executable = std::env::var("FTS_REAPER_EXECUTABLE").unwrap_or_else(|_| {
        runner::which_command("reaper").unwrap_or_else(|| "reaper".to_string())
    });
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

        // All rigs share the same canonical REAPER resources directory.
        let rig_resources = fts_reaper_resources();
        std::fs::create_dir_all(rig_resources.join("UserPlugins"))?;

        // Symlink UserPlugins from the nix REAPER install if empty
        // (so REAPER finds its built-in plugins alongside our extensions)

        // Write reaper.ini with undomaxmem=0
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
    // Try canonical location first, then legacy
    let canonical = PathBuf::from(format!("{home}/.config/fts/rigs/fts-daw-test/launch.json"));
    reaper_launcher::LaunchConfig::load(&canonical).ok()
}

fn reaper_test(filter: Option<String>, keep_open: bool) -> Result<(), Box<dyn std::error::Error>> {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let ci = std::env::var("CI").is_ok();
    let timeout_secs: u64 = std::env::var("REAPER_TEST_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let resources_dir = runner::fts_reaper_resources();

    let runner = TestRunner {
        resources_dir: resources_dir.clone(),
        extension_log: PathBuf::from("/tmp/daw-bridge.log"),
        timeout_secs,
        keep_open,
        ci,
        extension_whitelist: vec![],
    };

    // ── Rig config ─────────────────────────────────────────────────────────
    let rig_config = load_daw_test_rig();
    runner::section(ci, "reaper-test: rig");
    if rig_config.is_some() {
        println!("  rig: fts-daw-test (~/.config/fts/rigs/fts-daw-test/launch.json)");
    } else {
        println!("  WARNING: fts-daw-test rig not found — run `cargo xtask setup-rigs`");
        println!("  Falling back to legacy test config");
    }

    // ── Step 1: Build the test extension ──────────────────────────────────
    runner::section(ci, "reaper-test: build extension");
    println!("Building daw-bridge...");
    let status = Command::new("cargo")
        .args(["build", "-p", "daw-bridge"])
        .status()?;
    if !status.success() {
        return Err("Failed to build daw-bridge".into());
    }
    runner::end_section(ci);

    // ── Step 1b: Build daw-guest-example ────────────────────────────────
    runner::section(ci, "reaper-test: build guest example");
    println!("Building daw-guest-example...");
    let status = Command::new("cargo")
        .args(["build", "-p", "daw-guest-example"])
        .status()?;
    if !status.success() {
        return Err("Failed to build daw-guest-example".into());
    }
    runner::end_section(ci);

    // ── Step 2: Build test binaries (no-run) ──────────────────────────────
    runner::section(ci, "reaper-test: build test binaries");
    println!("Building test binaries...");
    let status = Command::new("cargo")
        .args(["test", "-p", "daw-reaper", "--no-run"])
        .status()?;
    if !status.success() {
        return Err("Failed to build daw-reaper test binaries".into());
    }
    runner::end_section(ci);

    // ── Step 3: Install the .so into REAPER's UserPlugins dir ─────────────
    runner::section(ci, "reaper-test: install plugin");
    let user_plugins_dir = resources_dir.join("UserPlugins");
    std::fs::create_dir_all(&user_plugins_dir)?;

    let so_src_name = "libreaper_daw_bridge.so";
    let so_dst_name = "reaper_daw_bridge.so";
    let so_path = workspace_root.join("target/debug").join(so_src_name);
    if !so_path.exists() {
        let dylib_src_name = "libreaper_daw_bridge.dylib";
        let dylib_dst_name = "reaper_daw_bridge.dylib";
        let dylib_path = workspace_root.join("target/debug").join(dylib_src_name);
        if dylib_path.exists() {
            runner::install_plugin(&dylib_path, dylib_dst_name, &user_plugins_dir)?;
        } else {
            return Err(format!(
                "Built library not found at {} or {}",
                so_path.display(),
                dylib_path.display()
            )
            .into());
        }
    } else {
        runner::install_plugin(&so_path, so_dst_name, &user_plugins_dir)?;
    }
    runner::end_section(ci);

    // ── Step 3b: Install daw-guest into fts-extensions/ ──────────────────
    runner::section(ci, "reaper-test: install guest extensions");
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
    runner::end_section(ci);

    // ── Step 4: Clean, pre-warm, patch INI ────────────────────────────────
    runner.clean_stale_sockets();
    runner.prewarm_reaper();
    runner.patch_ini();

    // ── Step 5: Spawn REAPER ──────────────────────────────────────────────
    let mut reaper = runner.spawn_reaper()?;
    reaper.wait_for_socket(&runner)?;

    // ── Step 6: Run tests ─────────────────────────────────────────────────
    let packages = vec![TestPackage {
        package: "daw-reaper".into(),
        features: vec![],
        test_threads: 1,
        default_skips: vec!["timer_responsive_for_60s".into()],
        test_binary: None,
    }];

    let tests_passed = runner.run_tests(&mut reaper, &packages, filter.as_deref())?;

    // ── Step 7: Cleanup and report ────────────────────────────────────────
    if !tests_passed {
        reaper.report_failure(&runner);
        reaper.stop(&runner);
        return Err("Some tests failed".into());
    }

    reaper.stop(&runner);
    println!("\nAll tests passed!");
    Ok(())
}

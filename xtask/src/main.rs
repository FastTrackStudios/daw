use clap::{Parser, Subcommand};
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
}

/// Where FTS REAPER bundles live.
const FTS_TRACKS_DIR: &str =
    "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS";

/// The real REAPER binary inside FTS-LIVE (the shared REAPER installation).
const REAPER_EXE: &str =
    "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/MacOS/REAPER";

/// Shared REAPER resources directory.
const REAPER_RESOURCES: &str =
    "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/Resources";

/// Path to reaper.ini.
const REAPER_INI: &str =
    "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/reaper.ini";

/// Default theme.
const DEFAULT_THEME: &str =
    "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/ColorThemes/Default_7.0.ReaperThemeZip";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::SetupTestBundles { force } => setup_test_bundles(force),
    }
}

struct BundleSpec {
    app_name: &'static str,
    role: &'static str,
    bundle_id: &'static str,
}

const BUNDLES: &[BundleSpec] = &[
    BundleSpec {
        app_name: "FTS-TESTING",
        role: "testing",
        bundle_id: "com.fasttrackstudio.testing",
    },
];

fn setup_test_bundles(force: bool) -> Result<(), Box<dyn std::error::Error>> {
    println!("Setting up REAPER test bundles in {FTS_TRACKS_DIR}");

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
        return Err(format!(
            "Launcher binary not found: {}",
            launcher_bin.display()
        )
        .into());
    }

    // Timestamp-based version busts macOS icon cache
    let version = format!(
        "1.0.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    );

    let base_dir = PathBuf::from(FTS_TRACKS_DIR);

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
            reaper_executable: REAPER_EXE.to_string(),
            resources_dir: REAPER_RESOURCES.to_string(),
            ini_path: REAPER_INI.to_string(),
            ini_overrides: reaper_launcher::ReaperIniConfig {
                undo_max_mem: None, // preserve original for testing
                theme: Some(DEFAULT_THEME.to_string()),
            },
            restore_ini_after_launch: false,
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

    println!("\nDone. FTS-TESTING.app is ready in {FTS_TRACKS_DIR}");

    Ok(())
}

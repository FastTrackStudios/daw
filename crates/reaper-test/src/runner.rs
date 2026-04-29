//! REAPER test runner — shared xtask infrastructure for spawning REAPER,
//! waiting for readiness, running `#[reaper_test]` binaries, and cleaning up.
//!
//! Feature-gated behind `runner` so test binaries don't pull in these deps.

use std::io::{BufRead, BufReader, Seek, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

// ─────────────────────────────────────────────────────────────
//  TestRunner — orchestrates a single REAPER test session
// ─────────────────────────────────────────────────────────────

/// Configuration for a REAPER test session.
pub struct TestRunner {
    /// Canonical REAPER resources directory (contains UserPlugins, reaper.ini, …).
    pub resources_dir: PathBuf,
    /// Path to the extension log file (e.g. `/tmp/daw-bridge.log`).
    pub extension_log: PathBuf,
    /// How many seconds to wait for the REAPER socket before giving up.
    pub timeout_secs: u64,
    /// Keep REAPER open after tests for manual inspection.
    pub keep_open: bool,
    /// Run REAPER headless (DISPLAY=""). Set to false to use the current display,
    /// which allows plugin GUIs to actually open and render.
    pub headless: bool,
    /// Running under CI (GitHub Actions) — enables `::group::` log sections.
    pub ci: bool,
    /// If set, only these guest extensions will be loaded by daw-bridge.
    /// Passed to REAPER as `FTS_EXTENSION_WHITELIST` env var.
    pub extension_whitelist: Vec<String>,
}

/// A test package to run inside the spawned REAPER session.
pub struct TestPackage {
    /// Cargo package name (e.g. `"daw-reaper"`, `"signal"`).
    pub package: String,
    /// Extra `--features` to enable (e.g. `["daw"]`).
    pub features: Vec<String>,
    /// How many test threads (`--test-threads=N`). 1 = serial.
    pub test_threads: u32,
    /// Tests to always skip when no filter is specified.
    pub default_skips: Vec<String>,
    /// Optional test binary name (`--test <name>`) to run a specific integration test file.
    pub test_binary: Option<String>,
}

/// A running REAPER process, ready for tests.
pub struct RunningReaper {
    child: Child,
    /// The discovered or assigned socket path.
    pub socket_path: String,
    /// Path to the REAPER process stdout/stderr log.
    pub reaper_log: PathBuf,
    pid: u32,
}

impl TestRunner {
    /// Create a `TestRunner` with sensible defaults.
    ///
    /// Only `resources_dir` is required. Everything else uses reasonable defaults:
    /// - `extension_log`: `<resources_dir>/daw-bridge.log`
    /// - `timeout_secs`: 60 (or `REAPER_TEST_TIMEOUT_SECS` env var)
    /// - `headless`: true
    /// - `keep_open`: from `FTS_KEEP_OPEN` env var
    /// - `ci`: from `CI` env var
    pub fn new(resources_dir: impl Into<PathBuf>) -> Self {
        let resources_dir = resources_dir.into();
        let timeout_secs = std::env::var("REAPER_TEST_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);
        Self {
            extension_log: resources_dir.join("daw-bridge.log"),
            resources_dir,
            timeout_secs,
            keep_open: std::env::var("FTS_KEEP_OPEN").unwrap_or_default() == "1",
            headless: true,
            ci: std::env::var("CI").is_ok(),
            extension_whitelist: vec![],
        }
    }

    /// Set the extension log path.
    pub fn with_extension_log(mut self, path: impl Into<PathBuf>) -> Self {
        self.extension_log = path.into();
        self
    }

    /// Set the test timeout in seconds.
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Set whether to run headless (DISPLAY="").
    pub fn with_headless(mut self, headless: bool) -> Self {
        self.headless = headless;
        self
    }

    /// Build and install the daw-bridge extension into the resources dir.
    ///
    /// `daw_workspace` should point to the root of the daw repo (containing
    /// `crates/daw-bridge/`). Builds in release mode and symlinks into UserPlugins.
    pub fn install_daw_bridge(
        &self,
        daw_workspace: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("── Building daw-bridge (release) ──");
        let status = Command::new("cargo")
            .args(["build", "-p", "daw-bridge", "--release"])
            .current_dir(daw_workspace)
            .status()?;
        if !status.success() {
            return Err("Failed to build daw-bridge".into());
        }

        let lib_path = daw_workspace.join("target/release/libreaper_daw_bridge.so");
        let plugins_dir = self.resources_dir.join("UserPlugins");
        install_plugin(&lib_path, "reaper_daw_bridge.so", &plugins_dir)?;
        Ok(())
    }

    /// Build and install a consumer extension into the resources dir.
    ///
    /// `workspace` should point to the workspace root. `package` is the cargo
    /// package name (e.g. `"reaper-input-extension"`). `lib_name` is the output
    /// library name (e.g. `"reaper_fts_input.so"`).
    pub fn install_extension(
        &self,
        workspace: &Path,
        package: &str,
        lib_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("── Building {package} (release) ──");
        let status = Command::new("cargo")
            .args(["build", "-p", package, "--release"])
            .current_dir(workspace)
            .status()?;
        if !status.success() {
            return Err(format!("Failed to build {package}").into());
        }

        // Derive the .so name from the package name (replace hyphens with underscores)
        let so_stem = package.replace('-', "_");
        let lib_path = workspace.join(format!("target/release/lib{so_stem}.so"));
        let plugins_dir = self.resources_dir.join("UserPlugins");
        install_plugin(&lib_path, lib_name, &plugins_dir)?;
        Ok(())
    }

    /// Remove all stale `fts-daw-*.sock` and `fts-daw-*.bootstrap.sock` files from `/tmp`.
    pub fn clean_stale_sockets(&self) {
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
        // Remove stale extension log
        let _ = std::fs::remove_file(&self.extension_log);
    }

    /// Run REAPER briefly to populate the `[nag]` token in `reaper.ini`,
    /// suppressing the evaluation dialog on subsequent runs.
    pub fn prewarm_reaper(&self) {
        let reaper_ini = self.resources_dir.join("reaper.ini");
        let needs_prewarm = reaper_ini
            .exists()
            .then(|| std::fs::read_to_string(&reaper_ini).ok())
            .flatten()
            .map_or(true, |content| !content.contains("[nag]"));

        if !needs_prewarm {
            return;
        }

        section(self.ci, "reaper-test: pre-warm (dismiss evaluation dialog)");
        println!("  [nag] section missing from reaper.ini — running REAPER briefly to populate it");

        let reaper_exe = resolve_reaper_exe();
        let fts_test = find_fts_test();
        let needs_fhs = std::env::var("DISPLAY").map_or(true, |d| d.is_empty());

        let prewarm_args: Vec<String> = vec![
            "-cfgfile".into(),
            reaper_ini.to_string_lossy().into_owned(),
            "-newinst".into(),
            "-nosplash".into(),
            "-ignoreerrors".into(),
        ];

        let spawn_result = if needs_fhs {
            if let Some(ref fts) = fts_test {
                let mut cmd = Command::new(fts);
                cmd.arg(&reaper_exe);
                cmd.args(&prewarm_args);
                cmd.stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                cmd.spawn()
            } else {
                let mut cmd = Command::new(&reaper_exe);
                cmd.args(&prewarm_args);
                cmd.stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                cmd.spawn()
            }
        } else {
            let mut cmd = Command::new(&reaper_exe);
            cmd.args(&prewarm_args);
            cmd.stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            cmd.spawn()
        };

        let Ok(mut prewarm_child) = spawn_result else {
            println!("  WARNING: Failed to spawn pre-warm REAPER");
            end_section(self.ci);
            return;
        };

        println!("  Waiting 10s for REAPER to initialize...");
        std::thread::sleep(std::time::Duration::from_secs(10));
        let _ = prewarm_child.kill();
        let _ = prewarm_child.wait();

        // Clean up sockets/logs from the pre-warm run
        self.clean_stale_sockets();

        // Verify the nag was written
        let has_nag = std::fs::read_to_string(&reaper_ini)
            .map(|c| c.contains("[nag]"))
            .unwrap_or(false);
        if has_nag {
            println!("  Pre-warm complete — [nag] token written to reaper.ini");
        } else {
            println!("  WARNING: Pre-warm did not produce [nag] token — timer may stall");
        }
        end_section(self.ci);
    }

    /// Patch `reaper.ini` to suppress the version-check dialog and optionally
    /// set `audiodriver` from `FTS_AUDIO_DRIVER`.
    pub fn patch_ini(&self) {
        let reaper_ini = self.resources_dir.join("reaper.ini");

        if !reaper_ini.exists() {
            println!("  reaper.ini not yet created — reaper-headless will write defaults");
            return;
        }

        // Patch audio driver if explicitly requested via environment
        if let Ok(audio_driver) = std::env::var("FTS_AUDIO_DRIVER") {
            let ini = reaper_launcher::ReaperIni::new(&reaper_ini);
            let _ = ini.set("audiodriver", &audio_driver);
            println!("  audiodriver: {audio_driver} (from FTS_AUDIO_DRIVER)");
        }

        // Patch lastt in [verchk] to suppress version-check dialog
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if let Ok(content) = std::fs::read_to_string(&reaper_ini) {
            let patched = if content.contains("lastt=") {
                content
                    .lines()
                    .map(|l| {
                        if l.starts_with("lastt=") {
                            format!("lastt={now_ts}")
                        } else {
                            l.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else if content.contains("[verchk]") {
                content.replace("[verchk]", &format!("[verchk]\nlastt={now_ts}"))
            } else {
                format!("{content}\n[verchk]\nlastt={now_ts}\n")
            };
            let _ = std::fs::write(&reaper_ini, &patched);
            println!("  Patched lastt={now_ts} in [verchk] (suppress version check dialog)");
        }

        // Remove stale projecttab entries so REAPER starts with a single clean tab.
        // Previous test runs can leave 100+ empty tabs in the ini, causing create_project()
        // to fail (REAPER's tab limit) and slowing down all project-scanning loops.
        if let Ok(content) = std::fs::read_to_string(&reaper_ini) {
            let original_lines = content.lines().count();
            let cleaned: String = content
                .lines()
                .filter(|l| !l.starts_with("projecttab") && !l.starts_with("lastproject="))
                .collect::<Vec<_>>()
                .join("\n");
            let cleaned_lines = cleaned.lines().count();
            let removed = original_lines - cleaned_lines;
            if removed > 0 {
                let _ = std::fs::write(&reaper_ini, &cleaned);
                println!(
                    "  Removed {removed} stale projecttab/lastproject entries from reaper.ini"
                );
            }
        }

        // Dump ini for diagnostics in CI
        if self.ci {
            if let Ok(content) = std::fs::read_to_string(&reaper_ini) {
                println!("  reaper.ini contents:");
                for line in content.lines() {
                    println!("    {line}");
                }
            }
        }
    }

    /// Spawn REAPER with FHS wrapper support and return a [`RunningReaper`].
    pub fn spawn_reaper(&self) -> Result<RunningReaper, Box<dyn std::error::Error>> {
        section(self.ci, "reaper-test: spawn REAPER");

        let reaper_exe = resolve_reaper_exe();
        let reaper_launcher_prefix = std::env::var("FTS_REAPER_LAUNCHER").ok();
        let fts_test = find_fts_test();
        let reaper_env = find_reaper_env();
        let needs_fhs = std::env::var("DISPLAY").map_or(true, |d| d.is_empty());
        // For GUI mode, use the full GUI REAPER from PATH (ignores FTS_REAPER_EXECUTABLE
        // which points to reaper-headless). The GUI REAPER's libSwell links X11/GL.
        let gui_reaper_exe = if !self.headless {
            resolve_gui_reaper_exe()
        } else {
            reaper_exe.clone()
        };
        let reaper_ini = self.resources_dir.join("reaper.ini");
        let reaper_log = PathBuf::from("/tmp/fts-daw-reaper.log");

        let reaper_args: Vec<String> = vec![
            "-cfgfile".into(),
            reaper_ini.to_string_lossy().into_owned(),
            "-newinst".into(),
            "-nosplash".into(),
            "-ignoreerrors".into(),
        ];

        println!("  exe:         {reaper_exe}");
        if let Some(ref launcher) = reaper_launcher_prefix {
            println!("  launcher:    {launcher}");
        }
        println!("  config dir:  {}", self.resources_dir.display());
        println!("  ini:         {}", reaper_ini.display());
        println!("  headless:    {}", self.headless);
        println!("  needs fhs:   {needs_fhs}");
        if let Some(ref fts) = fts_test {
            println!("  fts-test:    {fts}");
        } else if needs_fhs {
            println!("  WARNING: no fts-test found and no DISPLAY — REAPER may fail");
        }
        if let Some(ref env) = reaper_env {
            println!("  reaper-env:  {env}");
        }
        println!(
            "  timeout:     {}s (REAPER_TEST_TIMEOUT_SECS)",
            self.timeout_secs
        );
        if !self.extension_whitelist.is_empty() {
            println!("  whitelist:   {}", self.extension_whitelist.join(","));
        }
        println!("  logs:");
        println!("    REAPER process → {}", reaper_log.display());
        println!("    extension      → {}", self.extension_log.display());

        // Redirect REAPER stdout/stderr to its own log file
        let reaper_log_file = std::fs::File::create(&reaper_log)
            .map_err(|e| format!("Failed to create REAPER log {}: {e}", reaper_log.display()))?;
        let reaper_log_stderr = reaper_log_file.try_clone()?;

        // Build the REAPER command, optionally prefixed with a launcher (e.g. pw-jack)
        let effective_exe: String;
        let mut extra_prefix_args: Vec<String> = Vec::new();
        if let Some(ref launcher) = reaper_launcher_prefix {
            effective_exe = launcher.clone();
            extra_prefix_args.push(reaper_exe.clone());
        } else {
            effective_exe = reaper_exe.clone();
        }

        // Whitelist env var — applied to whichever command we build
        let whitelist_val = if self.extension_whitelist.is_empty() {
            None
        } else {
            Some(self.extension_whitelist.join(","))
        };

        // Helper: apply common env vars to any REAPER command.
        // - DISPLAY="" makes REAPER headless (no visible window); pass-through when headless=false
        // - FTS_SYNC_NO_MDNS=1 prevents mDNS advertisement that would interfere
        //   with multi-instance tests which manage their own mDNS discovery
        // - FTS_SYNC_NO_LINK=1 prevents Ableton Link cross-talk
        let headless = self.headless;
        let reaper_log_for_nih = reaper_log.clone();
        let apply_env = |cmd: &mut Command| {
            if headless {
                cmd.env("DISPLAY", "");
            }
            // When not headless, inherit DISPLAY from the environment so GUI tests
            // can open plugin windows and actually render frames.
            cmd.env("FTS_SYNC_NO_MDNS", "1");
            cmd.env("FTS_SYNC_NO_LINK", "1");
            // Route nih_plug log output (nih_log!/nih_warn!/nih_error!) to the REAPER log
            // so GPU/surface messages show up in the same file as other test output.
            cmd.env("NIH_LOG", &reaper_log_for_nih);
            if let Some(ref wl) = whitelist_val {
                cmd.env("FTS_EXTENSION_WHITELIST", wl);
            }
        };

        let reaper_child = if needs_fhs {
            if let Some(ref fts) = fts_test {
                let mut cmd = Command::new(fts);
                cmd.arg(&effective_exe);
                cmd.args(&extra_prefix_args);
                cmd.args(&reaper_args);
                apply_env(&mut cmd);
                cmd.stdout(reaper_log_file).stderr(reaper_log_stderr);
                println!(
                    "  spawning: {fts} {effective_exe} {} {}",
                    extra_prefix_args.join(" "),
                    reaper_args.join(" ")
                );
                cmd.spawn()
                    .map_err(|e| format!("Failed to spawn via fts-test: {e}"))?
            } else {
                let mut cmd = Command::new(&effective_exe);
                cmd.args(&extra_prefix_args);
                cmd.args(&reaper_args);
                apply_env(&mut cmd);
                cmd.stdout(reaper_log_file).stderr(reaper_log_stderr);
                println!(
                    "  spawning: {effective_exe} {} {} (no fhs wrapper)",
                    extra_prefix_args.join(" "),
                    reaper_args.join(" ")
                );
                cmd.spawn()
                    .map_err(|e| format!("Failed to spawn REAPER: {e}"))?
            }
        } else if !headless {
            // GUI mode with a real display: use the GUI REAPER (which has X11/GL in
            // libSwell) prefixed by reaper-env (bubblewrap FHS chroot for GL/EGL).
            // FTS_REAPER_EXECUTABLE points to reaper-headless and is intentionally
            // ignored here — gui_reaper_exe comes from `which reaper` (nix profile).
            if let Some(ref env_bin) = reaper_env {
                let mut cmd = Command::new(env_bin);
                cmd.arg(&gui_reaper_exe);
                cmd.args(&reaper_args);
                apply_env(&mut cmd);
                cmd.stdout(reaper_log_file).stderr(reaper_log_stderr);
                println!(
                    "  spawning: {env_bin} {gui_reaper_exe} {}",
                    reaper_args.join(" ")
                );
                cmd.spawn()
                    .map_err(|e| format!("Failed to spawn via reaper-env: {e}"))?
            } else {
                // reaper-env not found — fall back to plain GUI REAPER with warning
                println!("  WARNING: reaper-env not found; plugin GUIs may fail without FHS env");
                let mut cmd = Command::new(&gui_reaper_exe);
                cmd.args(&reaper_args);
                apply_env(&mut cmd);
                cmd.stdout(reaper_log_file).stderr(reaper_log_stderr);
                println!("  spawning: {gui_reaper_exe} {}", reaper_args.join(" "));
                cmd.spawn()
                    .map_err(|e| format!("Failed to spawn GUI REAPER: {e}"))?
            }
        } else {
            // Headless with a display: use plain reaper (no GUI windows needed)
            let mut cmd = Command::new(&effective_exe);
            cmd.args(&extra_prefix_args);
            cmd.args(&reaper_args);
            apply_env(&mut cmd);
            cmd.stdout(reaper_log_file).stderr(reaper_log_stderr);
            println!(
                "  spawning: {effective_exe} {} {}",
                extra_prefix_args.join(" "),
                reaper_args.join(" ")
            );
            cmd.spawn()
                .map_err(|e| format!("Failed to spawn REAPER: {e}"))?
        };

        let pid = reaper_child.id();
        println!("  spawned PID: {pid}");

        Ok(RunningReaper {
            child: reaper_child,
            socket_path: String::new(), // filled in by wait_for_socket
            reaper_log,
            pid,
        })
    }

    /// Run test packages against a running REAPER, returning true if all passed.
    pub fn run_tests(
        &self,
        reaper: &mut RunningReaper,
        packages: &[TestPackage],
        filter: Option<&str>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        section(self.ci, "reaper-test: run tests");

        let mut all_passed = true;
        for pkg in packages {
            println!("Running cargo test -p {} …", pkg.package);
            let mut test_cmd = Command::new("cargo");
            test_cmd.args(["test", "-p", &pkg.package]);

            // Add features
            if !pkg.features.is_empty() {
                test_cmd.arg("--features");
                test_cmd.arg(pkg.features.join(","));
            }

            // Filter to a specific test binary (integration test file)
            if let Some(ref bin) = pkg.test_binary {
                test_cmd.args(["--test", bin]);
            }

            test_cmd.args([
                "--",
                "--ignored",
                "--nocapture",
                &format!("--test-threads={}", pkg.test_threads),
            ]);

            if let Some(f) = filter {
                test_cmd.arg(f);
            } else {
                // Apply default skips
                for skip in &pkg.default_skips {
                    test_cmd.args(["--skip", skip]);
                }
            }

            test_cmd.env("FTS_SOCKET", &reaper.socket_path);
            if self.keep_open {
                test_cmd.env("FTS_KEEP_OPEN", "1");
            }

            let mut test_child = test_cmd.spawn()?;
            let test_timeout = std::time::Duration::from_secs(self.timeout_secs);
            let test_start = std::time::Instant::now();

            let tests_passed = loop {
                match test_child.try_wait()? {
                    Some(status) => break status.success(),
                    None if test_start.elapsed() > test_timeout => {
                        println!(
                            "Test process did not exit within {}s — killing it",
                            self.timeout_secs
                        );
                        let _ = test_child.kill();
                        let _ = test_child.wait();
                        break false;
                    }
                    None => std::thread::sleep(std::time::Duration::from_millis(200)),
                }
            };

            if !tests_passed {
                all_passed = false;
            }

            // After tests, we need to wait for the test process to fully exit
            // (it may be blocked on Runtime drop waiting for VOX driver).
            // Don't kill REAPER yet — there may be more packages to run.
            for _ in 0..20 {
                if test_child.try_wait()?.is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            if test_child.try_wait()?.is_none() {
                println!("Test process still alive — force killing");
                let _ = test_child.kill();
                let _ = test_child.wait();
            }
        }

        end_section(self.ci);
        Ok(all_passed)
    }
}

impl RunningReaper {
    /// Wait for the REAPER socket to appear, tailing the extension log in the background.
    /// On success, populates `self.socket_path`.
    pub fn wait_for_socket(
        &mut self,
        runner: &TestRunner,
    ) -> Result<(), Box<dyn std::error::Error>> {
        section(runner.ci, "reaper-test: waiting for REAPER ready");
        println!(
            "  Waiting up to {}s for fts-daw-*.sock …",
            runner.timeout_secs
        );
        println!("  Extension log: {}", runner.extension_log.display());

        let start = std::time::Instant::now();
        let deadline = start + std::time::Duration::from_secs(runner.timeout_secs);

        // Background thread: tail the extension log
        let ext_log_clone = runner.extension_log.clone();
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
                            pos += line.len() as u64 + 1;
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        });

        let socket_path = loop {
            // Check if REAPER process died unexpectedly
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    println!("\n  REAPER exited early with status: {status}");
                    dump_log_on_failure(&runner.extension_log, "extension");
                    dump_log_on_failure(&self.reaper_log, "REAPER process");
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
                let _ = self.child.kill();
                let _ = self.child.wait();
                dump_log_on_failure(&runner.extension_log, "extension");
                dump_log_on_failure(&self.reaper_log, "REAPER process");
                list_tmp_sockets();
                return Err(format!(
                    "Timed out after {}s waiting for fts-daw-*.sock",
                    runner.timeout_secs
                )
                .into());
            }

            std::thread::sleep(std::time::Duration::from_millis(300));
            let elapsed = start.elapsed().as_secs();
            if elapsed % 5 == 0 && elapsed > 0 {
                print!("\r  [{elapsed}s] waiting …   ");
                let _ = std::io::stdout().flush();
            }
        };

        // Brief pause to let the listener fully bind
        std::thread::sleep(std::time::Duration::from_millis(500));
        self.socket_path = socket_path;
        end_section(runner.ci);
        Ok(())
    }

    /// Stop the REAPER process and clean up.
    pub fn stop(mut self, runner: &TestRunner) {
        if runner.keep_open {
            println!(
                "REAPER left running (PID {}) — close it normally to exit",
                self.pid
            );
            // Block on REAPER until the user closes it. The bwrap sandbox
            // we spawned through (fts-test) uses --die-with-parent, so
            // letting the xtask process exit here would tear REAPER down
            // immediately. Wait synchronously instead so the xtask
            // (and its bwrap parent chain) stays alive for the whole
            // inspection session.
            let _ = self.child.wait();
            let _ = std::fs::remove_file(&self.socket_path);
            return;
        }
        println!("Killing REAPER (PID {})…", self.pid);
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket_path);
    }

    /// Get the PID of the REAPER process.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Report failure: dump logs and print per-test log directory.
    pub fn report_failure(&self, runner: &TestRunner) {
        dump_log_on_failure(&runner.extension_log, "extension");
        dump_log_on_failure(&self.reaper_log, "REAPER process");
        println!("Per-test logs: /tmp/reaper-tests/");
    }
}

// ─────────────────────────────────────────────────────────────
//  Free functions (utilities)
// ─────────────────────────────────────────────────────────────

/// Scan /tmp for any `fts-daw-*.sock` file and return its path.
pub fn find_fts_daw_socket() -> Option<String> {
    let entries = std::fs::read_dir("/tmp").ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("fts-daw-")
                && name.ends_with(".sock")
                && !name.contains(".bootstrap.")
            {
                return Some(path.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// Find the `fts-test` launcher (Xvfb + FHS wrapper).
pub fn find_fts_test() -> Option<String> {
    if let Some(p) = which_command("fts-test") {
        return Some(p);
    }

    // Stable devenv profile symlink
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent());
    if let Some(root) = workspace_root {
        let devenv_fts = root.join(".devenv/profile/bin/fts-test");
        if devenv_fts.exists() {
            return Some(devenv_fts.to_string_lossy().into_owned());
        }
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

/// Find the `reaper-env` FHS chroot wrapper.
///
/// `reaper-env` is a bubblewrap FHS chroot that provides GL/EGL libraries at
/// standard system paths. It takes any binary as its first arg and runs it
/// inside the chroot, preserving /nix/store access. Used as a prefix launcher
/// for the GUI REAPER binary so that plugins using WGPU/OpenGL can find the
/// GL/EGL libraries they need via dlopen.
pub fn find_reaper_env() -> Option<String> {
    if let Some(p) = which_command("reaper-env") {
        return Some(p);
    }

    let candidates = [
        "/run/current-system/sw/bin/reaper-env",
        "/nix/var/nix/profiles/default/bin/reaper-env",
    ];
    candidates
        .iter()
        .find(|p| PathBuf::from(p).exists())
        .map(|s| s.to_string())
}

/// Find a command on PATH.
pub fn which_command(name: &str) -> Option<String> {
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

/// Resolve the REAPER executable path from environment.
///
/// Uses `FTS_REAPER_EXECUTABLE` if set, then falls back to `which reaper`.
/// `FTS_REAPER_EXECUTABLE` typically points to `reaper-headless` for CI use.
pub fn resolve_reaper_exe() -> String {
    std::env::var("FTS_REAPER_EXECUTABLE")
        .or_else(|_| which_command("reaper").ok_or(()))
        .unwrap_or_else(|_| "reaper".to_string())
}

/// Resolve the GUI REAPER executable, ignoring `FTS_REAPER_EXECUTABLE`.
///
/// `FTS_REAPER_EXECUTABLE` typically points to `reaper-headless` which lacks
/// X11/GL and cannot open plugin windows. GUI tests need the full REAPER binary
/// that links libSwell with X11 and GL support. This function always uses
/// `which reaper` to find the GUI binary from the user's Nix profile.
pub fn resolve_gui_reaper_exe() -> String {
    which_command("reaper").unwrap_or_else(|| "reaper".to_string())
}

/// Canonical REAPER resources directory shared by all rigs and CI.
pub fn fts_reaper_resources() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    if let Ok(p) = std::env::var("FTS_REAPER_CONFIG") {
        return PathBuf::from(p.replace("$HOME", &home));
    }
    PathBuf::from(format!("{home}/.config/FastTrackStudio/Reaper"))
}

/// Install (symlink) a plugin library into the given UserPlugins directory.
pub fn install_plugin(
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

/// Print a section header. In CI (GitHub Actions) emits `::group::` for collapsible logs.
pub fn section(ci: bool, name: &str) {
    if ci {
        println!("::group::{name}");
    } else {
        println!("\n── {name} ──");
    }
}

/// End a section. In CI emits `::endgroup::`.
pub fn end_section(ci: bool) {
    if ci {
        println!("::endgroup::");
    }
}

/// Dump the last N lines of a log file to stdout (called on failure).
pub fn dump_log_on_failure(log_path: &Path, label: &str) {
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

/// List any `fts-daw-*.sock` files currently in `/tmp` (diagnostic helper).
pub fn list_tmp_sockets() {
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

/// Scan test directories for files matching a filter string.
/// Checks both filenames and function definitions inside the files.
pub fn find_matching_test_binaries(filter: &str, test_dirs: &[&Path]) -> Vec<String> {
    let mut matches = Vec::new();
    for dir in test_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "rs").unwrap_or(false) {
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if stem.contains(filter) {
                    if !matches.contains(&stem.to_string()) {
                        matches.push(stem.to_string());
                    }
                    continue;
                }
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    let pattern = format!("fn {filter}");
                    if contents.contains(&pattern) && !matches.contains(&stem.to_string()) {
                        matches.push(stem.to_string());
                    }
                }
            }
        }
    }
    matches
}

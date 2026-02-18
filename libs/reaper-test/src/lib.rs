//! Runtime library for REAPER integration tests.
//!
//! Provides:
//! - [`ReaperTestContext`] — wraps shared `Daw` + project handle + per-test logging
//! - [`run_reaper_test`] — assigns a project tab, runs test, cleans up
//! - [`ReaperProcess`] — spawn/wait/kill-on-drop guard
//! - [`connect_daw`] — connection + polling logic
//! - Re-exports `#[reaper_test]` from `reaper-test-macro`

use daw_control::{Daw, Project, TrackHandle};
use eyre::Result;
use host_client::HostConnector;
use std::{
    fs::{self, File},
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    pin::Pin,
    process::{Child, Command},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    time::Duration,
};
use tokio::runtime::Runtime;

// Re-export the proc-macro so users can `use reaper_test::reaper_test;`
pub use reaper_test_macro::reaper_test;

// ─────────────────────────────────────────────────────────────
//  Constants
// ─────────────────────────────────────────────────────────────

pub const SOCKET_PATH: &str = "/tmp/fts-control.sock";
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const REAPER_BOOT_TIMEOUT_SECS: u64 = 30;
pub const REAPER_EXECUTABLE: &str =
    "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/MacOS/REAPER";
pub const REAPER_RESOURCES: &str =
    "/Users/codywright/Music/FastTrackStudio/Reaper/FTS-TRACKS/FTS-LIVE.app/Contents/Resources";
pub const LOG_DIR: &str = "/tmp/reaper-tests";

// ─────────────────────────────────────────────────────────────
//  ReaperProcess — spawn/kill guard
// ─────────────────────────────────────────────────────────────

/// RAII guard that spawns a REAPER process and kills it on drop.
pub struct ReaperProcess(Child);

impl ReaperProcess {
    /// Spawn REAPER with no project (empty session).
    pub fn spawn() -> Result<Self> {
        let _ = std::fs::remove_file(SOCKET_PATH);
        let child = Command::new(REAPER_EXECUTABLE)
            .current_dir(REAPER_RESOURCES)
            .arg("-nosplash")
            .arg("-ignoreerrors")
            .spawn()
            .map_err(|e| eyre::eyre!("Failed to spawn REAPER at {REAPER_EXECUTABLE}: {e}"))?;
        println!("  Spawned REAPER (PID {})", child.id());
        Ok(Self(child))
    }

    /// Spawn REAPER with a specific project file.
    pub fn spawn_with_project(project_path: &str) -> Result<Self> {
        let _ = std::fs::remove_file(SOCKET_PATH);
        let child = Command::new(REAPER_EXECUTABLE)
            .current_dir(REAPER_RESOURCES)
            .arg("-nosplash")
            .arg("-ignoreerrors")
            .arg(project_path)
            .spawn()
            .map_err(|e| eyre::eyre!("Failed to spawn REAPER at {REAPER_EXECUTABLE}: {e}"))?;
        println!(
            "  Spawned REAPER (PID {}) with project {}",
            child.id(),
            project_path
        );
        Ok(Self(child))
    }

    /// Block until the Unix socket appears (REAPER is ready for connections).
    pub fn wait_for_socket(&self) -> Result<()> {
        let socket = Path::new(SOCKET_PATH);
        let deadline = std::time::Instant::now() + Duration::from_secs(REAPER_BOOT_TIMEOUT_SECS);
        print!("  Waiting for socket");
        while !socket.exists() {
            if std::time::Instant::now() > deadline {
                println!();
                return Err(eyre::eyre!(
                    "Timed out after {REAPER_BOOT_TIMEOUT_SECS}s waiting for {SOCKET_PATH}"
                ));
            }
            std::thread::sleep(Duration::from_millis(500));
            print!(".");
        }
        println!("\n  Socket ready");
        Ok(())
    }
}

impl Drop for ReaperProcess {
    fn drop(&mut self) {
        println!("\n  Killing REAPER (PID {})...", self.0.id());
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_file(SOCKET_PATH);
    }
}

// ─────────────────────────────────────────────────────────────
//  Shared DAW connection (singleton per test process)
// ─────────────────────────────────────────────────────────────

/// Shared runtime + DAW connection that persists across all tests.
///
/// Each `#[reaper_test]` creates its own `#[tokio::test]` runtime, but the
/// ROAM driver task must live on a single runtime for the entire process.
/// If the creating runtime drops, the driver dies and all subsequent tests
/// get `DriverGone`. Solving this by owning a long-lived runtime here.
struct SharedState {
    runtime: Runtime,
    daw: Daw,
}

static SHARED: OnceLock<SharedState> = OnceLock::new();
/// Serializes initialization so only one thread builds the runtime + connection.
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Get or create the shared DAW connection (and its runtime).
///
/// Established once and reused across all parallel tests.
/// Only project tab creation is per-test.
fn shared_daw() -> Result<Daw> {
    if let Some(state) = SHARED.get() {
        return Ok(state.daw.clone());
    }

    // Serialize initialization across threads
    let _guard = INIT_LOCK.lock().unwrap();

    // Double-check after acquiring lock
    if let Some(state) = SHARED.get() {
        return Ok(state.daw.clone());
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| eyre::eyre!("Failed to build shared runtime: {e}"))?;

    let daw = runtime.block_on(connect_daw())?;
    let _ = SHARED.set(SharedState { runtime, daw });
    Ok(SHARED.get().unwrap().daw.clone())
}

/// Run a future on the shared runtime (where the ROAM driver lives).
///
/// This is the key to making parallel tests work: all DAW calls go through
/// the same runtime that owns the driver task, so it never becomes `DriverGone`.
pub fn block_on_shared<F: Future<Output = T>, T>(f: F) -> T {
    let state = SHARED
        .get()
        .expect("shared_daw() must be called before block_on_shared()");
    state.runtime.block_on(f)
}

// ─────────────────────────────────────────────────────────────
//  Batch project tab pool
// ─────────────────────────────────────────────────────────────

/// How many non-isolated tests share a single project tab before
/// a new tab is created for the next batch.
const BATCH_SIZE: u32 = 10;

/// A project tab shared by a batch of tests.
///
/// The first test in the batch creates the tab. Each test increments
/// `active_count` on entry and decrements on exit. When `active_count`
/// drops to zero AND `slots_claimed >= BATCH_SIZE` (batch is full and
/// all tests done), the tab is closed.
struct BatchTab {
    project: Project,
    guid: String,
    /// How many tests are currently running in this batch.
    active_count: AtomicU32,
    /// How many slots have been claimed (monotonically increasing).
    slots_claimed: AtomicU32,
    /// Signaled when `active_count` reaches 0.
    done: Condvar,
    done_mutex: Mutex<()>,
}

/// Global batch pool.
///
/// `current_batch` holds the batch that new non-isolated tests join.
/// `retired_batches` holds batches that are full but still have active tests.
struct BatchPool {
    current_batch: Option<Arc<BatchTab>>,
    retired_batches: Vec<Arc<BatchTab>>,
}

static BATCH_POOL: Mutex<Option<BatchPool>> = Mutex::new(None);

/// Claim a project tab from the batch pool. Returns the batch and the project.
///
/// If the current batch has room, the test joins it. Otherwise, a new batch
/// is created. The caller must call `release_batch` when the test completes.
async fn claim_batch_tab(daw: &Daw) -> Result<Arc<BatchTab>> {
    let mut pool = BATCH_POOL.lock().unwrap();
    let pool = pool.get_or_insert_with(|| BatchPool {
        current_batch: None,
        retired_batches: Vec::new(),
    });

    // Check if current batch has room
    if let Some(ref batch) = pool.current_batch {
        let claimed = batch.slots_claimed.fetch_add(1, Ordering::SeqCst) + 1;
        if claimed <= BATCH_SIZE {
            batch.active_count.fetch_add(1, Ordering::SeqCst);
            return Ok(Arc::clone(batch));
        }
        // Batch is full — retire it and fall through to create a new one
        batch.slots_claimed.fetch_sub(1, Ordering::SeqCst); // undo our claim
        let retired = pool.current_batch.take().unwrap();
        pool.retired_batches.push(retired);
    }

    // Create a new batch tab
    let project = daw
        .create_project()
        .await
        .map_err(|e| eyre::eyre!("Failed to create batch project tab: {e}"))?;
    let guid = project.guid().to_string();

    // Clean slate
    project.tracks().remove_all().await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let batch = Arc::new(BatchTab {
        project,
        guid,
        active_count: AtomicU32::new(1),
        slots_claimed: AtomicU32::new(1),
        done: Condvar::new(),
        done_mutex: Mutex::new(()),
    });

    pool.current_batch = Some(Arc::clone(&batch));
    Ok(batch)
}

/// Release a batch tab after a test completes.
///
/// Decrements `active_count`. If this is the last test AND the batch is
/// full (or retired), closes the project tab.
async fn release_batch_tab(daw: &Daw, batch: &Arc<BatchTab>) {
    let prev = batch.active_count.fetch_sub(1, Ordering::SeqCst);
    if prev == 1 {
        // We were the last active test — notify waiters and maybe close
        let _guard = batch.done_mutex.lock().unwrap();
        batch.done.notify_all();
        drop(_guard);

        // Check if this batch is retired (no more tests will join)
        let is_retired = {
            let pool = BATCH_POOL.lock().unwrap();
            if let Some(ref pool) = *pool {
                // Retired if it's not the current batch, OR current batch is full
                let is_current = pool
                    .current_batch
                    .as_ref()
                    .map(|b| Arc::ptr_eq(b, batch))
                    .unwrap_or(false);
                !is_current || batch.slots_claimed.load(Ordering::SeqCst) >= BATCH_SIZE
            } else {
                true
            }
        };

        if is_retired {
            // Close the tab: remove tracks first to avoid save dialog
            let _ = batch.project.tracks().remove_all().await;
            if let Err(e) = daw.close_project(&batch.guid).await {
                eprintln!(
                    "Warning: failed to close batch project tab {}: {e}",
                    &batch.guid[..16.min(batch.guid.len())]
                );
            }

            // Remove from retired list
            let mut pool = BATCH_POOL.lock().unwrap();
            if let Some(ref mut pool) = *pool {
                pool.retired_batches.retain(|b| !Arc::ptr_eq(b, batch));
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────
//  Connection
// ─────────────────────────────────────────────────────────────

/// Connect to a running REAPER instance via Unix socket, polling until
/// a project with a non-empty GUID is available.
pub async fn connect_daw() -> Result<Daw> {
    let connector = HostConnector::unix(SOCKET_PATH);
    let connection = tokio::time::timeout(CONNECT_TIMEOUT, connector.connect())
        .await
        .map_err(|_| eyre::eyre!("Timed out connecting to REAPER at {SOCKET_PATH}"))?
        .map_err(|e| eyre::eyre!("Failed to connect to REAPER: {e}"))?;
    let daw = Daw::new(connection.handle().clone());

    print!("  Waiting for project");
    for _ in 0..60 {
        if let Ok(project) = daw.current_project().await {
            if let Ok(info) = project.info().await {
                if !info.guid.is_empty() {
                    println!(
                        " OK ({})",
                        if info.name.is_empty() {
                            info.guid.as_str()
                        } else {
                            info.name.as_str()
                        }
                    );
                    return Ok(daw);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        print!(".");
    }
    println!();
    Err(eyre::eyre!("Timed out waiting for project to load"))
}

// ─────────────────────────────────────────────────────────────
//  ReaperTestContext
// ─────────────────────────────────────────────────────────────

/// Context passed to each `#[reaper_test]` function.
///
/// For isolated tests, this wraps a dedicated REAPER project tab.
/// For batched tests, this wraps a project tab shared with up to
/// `BATCH_SIZE` other tests — cleanup happens when the whole batch
/// finishes, not per-test.
pub struct ReaperTestContext {
    /// Connected DAW handle (shared across all tests).
    pub daw: Daw,
    /// This test's project (isolated tab or batch-shared tab).
    pub project: Project,
    /// Path to the test assets directory (where `.RTrackTemplate` files live).
    asset_dir: PathBuf,
    /// Test function name.
    test_name: String,
    /// Per-test log file.
    log_file: Mutex<File>,
}

impl ReaperTestContext {
    /// Find a track by exact name in this test's project, returning an error if not found.
    pub async fn track_by_name(&self, name: &str) -> Result<TrackHandle> {
        self.project
            .tracks()
            .by_name(name)
            .await?
            .ok_or_else(|| eyre::eyre!("track not found: '{name}'"))
    }

    /// Get this test's isolated project handle.
    ///
    /// This returns the test's own project tab, NOT whatever happens to be
    /// the "current" project in REAPER. Safe for parallel use.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Get the full path to an asset file in the test assets directory.
    pub fn asset_path(&self, filename: &str) -> PathBuf {
        self.asset_dir.join(filename)
    }

    /// Load a `.RTrackTemplate` file into this test's project.
    ///
    /// Parses `<TRACK>` blocks from the template and inserts each as a new
    /// track with its full chunk state. The template name is resolved relative
    /// to the test assets directory (e.g., `"testing-stockjs-guitar-rig"`
    /// loads `tests/reaper-assets/testing-stockjs-guitar-rig.RTrackTemplate`).
    pub async fn load_template(&self, template_name: &str) -> Result<()> {
        let template_path = self
            .asset_dir
            .join(format!("{template_name}.RTrackTemplate"));
        load_template(&self.project, &template_path).await
    }

    /// Write a message to this test's log file.
    pub fn log(&self, msg: &str) {
        if let Ok(mut f) = self.log_file.lock() {
            let _ = writeln!(f, "{msg}");
        }
    }

    /// Get the path to this test's log file.
    pub fn log_path(&self) -> PathBuf {
        PathBuf::from(LOG_DIR).join(format!("{}.log", self.test_name))
    }
}

// ─────────────────────────────────────────────────────────────
//  Template loading
// ─────────────────────────────────────────────────────────────

/// Parse `<TRACK ...>` blocks from an RPP / `.RTrackTemplate` file.
///
/// Returns each top-level `<TRACK ... >` block as a separate string,
/// including the `<TRACK` opener and closing `>`.
fn parse_track_blocks(content: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current_block = String::new();
    let mut depth: i32 = 0;
    let mut in_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if !in_block {
            if trimmed.starts_with("<TRACK") {
                in_block = true;
                depth = 1;
                current_block.clear();
                current_block.push_str(line);
                current_block.push('\n');
            }
        } else {
            current_block.push_str(line);
            current_block.push('\n');

            if trimmed.starts_with('<') {
                depth += 1;
            } else if trimmed == ">" {
                depth -= 1;
                if depth == 0 {
                    blocks.push(current_block.clone());
                    current_block.clear();
                    in_block = false;
                }
            }
        }
    }

    blocks
}

/// Load a `.RTrackTemplate` file into a specific project tab.
///
/// For each `<TRACK>` block in the template:
/// 1. Insert a new track via `add_track`
/// 2. Set its full state via `set_track_chunk`
async fn load_template(project: &Project, template_path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(template_path)
        .map_err(|e| eyre::eyre!("Failed to read template {}: {e}", template_path.display()))?;

    let blocks = parse_track_blocks(&content);
    if blocks.is_empty() {
        return Err(eyre::eyre!(
            "No <TRACK> blocks found in {}",
            template_path.display()
        ));
    }

    println!(
        "  Loading template: {} ({} tracks)",
        template_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy(),
        blocks.len()
    );

    let tracks = project.tracks();

    for (i, chunk) in blocks.iter().enumerate() {
        let track = tracks.add(&format!("__template_{i}"), None).await?;
        track.set_chunk(chunk.clone()).await?;
    }

    // Brief settle time for REAPER to process the chunks.
    tokio::time::sleep(Duration::from_millis(500)).await;

    Ok(())
}

// ─────────────────────────────────────────────────────────────
//  Test runner
// ─────────────────────────────────────────────────────────────

/// The function type that `#[reaper_test]` generates for test bodies.
pub type TestBodyFn = dyn Fn(&ReaperTestContext) -> Pin<Box<dyn Future<Output = Result<()>> + '_>>;

/// Run a single REAPER integration test.
///
/// This is called by the code generated by `#[reaper_test]`. It:
/// 1. Gets the shared DAW connection (established once per process)
/// 2. If `isolated`, creates a dedicated project tab for this test alone.
///    Otherwise, claims a slot in a **batch** of up to `BATCH_SIZE` tests
///    that share one project tab. The tab is closed only when all tests
///    in the batch finish — no per-test cleanup races.
/// 3. Constructs a [`ReaperTestContext`] and calls the test body
/// 4. Cleans up: isolated tests close their tab immediately; batched tests
///    decrement their batch counter and the last one out closes the tab.
///
/// This function is **synchronous** — it runs all async work on the shared
/// runtime via `block_on_shared`. This prevents `DriverGone` errors from
/// each `#[test]` getting a separate tokio runtime.
pub fn run_reaper_test(
    test_name: &str,
    isolated: bool,
    body: impl Fn(&ReaperTestContext) -> Pin<Box<dyn Future<Output = Result<()>> + '_>>,
) -> Result<()> {
    // Ensure log directory exists
    let log_dir = Path::new(LOG_DIR);
    fs::create_dir_all(log_dir).map_err(|e| eyre::eyre!("Failed to create log dir: {e}"))?;

    let log_path = log_dir.join(format!("{test_name}.log"));
    let log_file = File::create(&log_path)
        .map_err(|e| eyre::eyre!("Failed to create log file {}: {e}", log_path.display()))?;

    // Get the shared DAW connection (also initializes the shared runtime)
    let daw = shared_daw()?;

    // All async work runs on the shared runtime
    block_on_shared(async {
        // Acquire a project tab — either isolated or from the batch pool
        let batch: Option<Arc<BatchTab>>;
        let project: Project;
        let is_own_tab: bool;

        if isolated {
            // Create a dedicated project tab for this test
            let p = daw
                .create_project()
                .await
                .map_err(|e| eyre::eyre!("[{test_name}] Failed to create project tab: {e}"))?;

            println!(
                "[{test_name}] Created isolated project tab (guid: {})",
                p.guid()
            );

            // Remove default tracks from new project tab
            p.tracks().remove_all().await?;
            tokio::time::sleep(Duration::from_millis(200)).await;

            project = p;
            batch = None;
            is_own_tab = true;
        } else {
            // Claim a slot in the current batch
            let b = claim_batch_tab(&daw).await?;
            project = b.project.clone();
            batch = Some(b);
            is_own_tab = false;
        };

        // Determine asset directory from CARGO_MANIFEST_DIR of the test crate.
        let asset_dir = std::env::var("CARGO_MANIFEST_DIR")
            .map(|d| PathBuf::from(d).join("tests").join("reaper-assets"))
            .unwrap_or_else(|_| PathBuf::from("tests/reaper-assets"));

        let ctx = ReaperTestContext {
            daw: daw.clone(),
            project: project.clone(),
            asset_dir,
            test_name: test_name.to_string(),
            log_file: Mutex::new(log_file),
        };

        // Run the test body
        let result = body(&ctx).await;

        // Cleanup
        if is_own_tab {
            // Isolated: remove all tracks then close the tab
            let project_guid = project.guid().to_string();
            let _ = project.tracks().remove_all().await;
            if let Err(e) = daw.close_project(&project_guid).await {
                eprintln!("[{test_name}] Warning: failed to close project tab: {e}");
            }
        } else if let Some(ref b) = batch {
            // Batched: release our slot — last test out closes the tab
            release_batch_tab(&daw, b).await;
        }

        // On failure, print the log file path
        if result.is_err() {
            eprintln!("[{test_name}] FAILED — log: {}", log_path.display());
        }

        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_track_block() {
        let input = "<TRACK\n  NAME \"Test\"\n  VOLPAN 1 0\n>\n";
        let blocks = parse_track_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].starts_with("<TRACK"));
        assert!(blocks[0].trim_end().ends_with(">"));
    }

    #[test]
    fn parse_nested_track_block() {
        let input = r#"<TRACK
  NAME "Outer"
  <FXCHAIN
    <VST "ReaEQ" foo.dll
    >
  >
>
"#;
        let blocks = parse_track_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("FXCHAIN"));
    }

    #[test]
    fn parse_multiple_tracks() {
        let input = r#"<TRACK
  NAME "Track 1"
>
<TRACK
  NAME "Track 2"
>
"#;
        let blocks = parse_track_blocks(input);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].contains("Track 1"));
        assert!(blocks[1].contains("Track 2"));
    }
}

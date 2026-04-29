//! REAPER ↔ DAW bridge extension.
//!
//! Loads daw-reaper's service implementations inside REAPER and exposes them
//! via Unix socket and SHM so external processes (tests, FTS, CLI tools)
//! can control the DAW through vox RPC.

mod guest_loader;
mod project_import;
mod routed_handler;
mod shm_host;

// ============================================================================
// RT-safe Global Allocator
// ============================================================================

#[global_allocator]
static ALLOCATOR: daw_allocator::FtsAllocator = daw_allocator::FtsAllocator::new();

use fragile::Fragile;
use reaper_high::{MainTaskMiddleware, Reaper as HighReaper};
use reaper_low::PluginContext;
use reaper_macros::reaper_extension_plugin;
use reaper_medium::ReaperSession;
use std::cell::RefCell;
use std::error::Error;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::net::UnixListener;
use tracing::{debug, info, warn};

use routed_handler::{DawConnectionAcceptor, RoutedHandler};

// Service dispatchers for method ID routing
use daw::service::{
    ActionRegistryServiceDispatcher, AudioEngineServiceDispatcher, BatchServiceDispatcher,
    ExtStateServiceDispatcher, FxServiceDispatcher, HealthServiceDispatcher,
    InputServiceDispatcher, ItemServiceDispatcher, LiveMidiServiceDispatcher,
    MarkerServiceDispatcher, MidiAnalysisServiceDispatcher, MidiServiceDispatcher,
    PluginLoaderServiceDispatcher, ProjectServiceDispatcher, RegionServiceDispatcher,
    RoutingServiceDispatcher, TakeServiceDispatcher, TempoMapServiceDispatcher,
    ToolbarServiceDispatcher, TrackServiceDispatcher, TransportServiceDispatcher,
};

// ============================================================================
// Global State (TaskSupport for main thread dispatch)
// ============================================================================

use crossbeam_channel::{Receiver, Sender};
use daw_allocator::{FtsRuntime, FtsRuntimeConfig, RtDetector};
use reaper_high::{MainThreadTask, TaskSupport};

static GLOBAL: OnceLock<Global> = OnceLock::new();

struct Global {
    task_support: TaskSupport,
    task_sender: Sender<MainThreadTask>,
    task_receiver: Receiver<MainThreadTask>,
}

impl Global {
    fn init() {
        GLOBAL.get_or_init(|| {
            let (task_sender, task_receiver) = crossbeam_channel::unbounded();
            info!("Global TaskSupport initialized");
            Global {
                task_support: TaskSupport::new(task_sender.clone()),
                task_sender,
                task_receiver,
            }
        });
    }

    fn get() -> &'static Global {
        GLOBAL
            .get()
            .expect("Global not initialized — call Global::init() first")
    }

    fn task_support() -> &'static TaskSupport {
        &Global::get().task_support
    }

    fn create_task_middleware(&self) -> MainTaskMiddleware {
        MainTaskMiddleware::new(self.task_sender.clone(), self.task_receiver.clone())
    }
}

// ============================================================================
// Application State
// ============================================================================

struct App {
    session: RefCell<ReaperSession>,
    #[allow(dead_code)]
    tokio_runtime: tokio::runtime::Runtime,
    task_middleware: RefCell<MainTaskMiddleware>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App").finish_non_exhaustive()
    }
}

impl App {
    fn new(session: ReaperSession) -> Result<Self, Box<dyn Error>> {
        let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;

        Global::init();

        let task_middleware = Global::get().create_task_middleware();

        Ok(Self {
            session: RefCell::new(session),
            tokio_runtime,
            task_middleware: RefCell::new(task_middleware),
        })
    }

    fn process_tasks(&self) {
        self.task_middleware.borrow_mut().run();
    }

    fn initialize(&self) -> Result<(), Box<dyn Error>> {
        info!("Initializing daw-bridge...");

        self.tokio_runtime.block_on(async {
            register_daw_dispatcher().await;
        });

        info!("daw-bridge initialized");
        Ok(())
    }
}

// ============================================================================
// DAW Dispatcher Registration
// ============================================================================

async fn register_daw_dispatcher() {
    info!("Registering DAW dispatcher...");

    // Set TaskSupport for daw-reaper to use
    daw::reaper::set_task_support(Global::task_support());

    // Initialize all broadcasters
    daw::reaper::init_transport_broadcaster();
    daw::reaper::init_fx_broadcaster();
    daw::reaper::init_track_broadcaster();
    daw::reaper::init_item_broadcaster();
    daw::reaper::init_routing_broadcaster();
    daw::reaper::init_tempo_map_broadcaster();
    info!("All broadcasters initialized");

    // Create REAPER implementations
    let transport = daw::reaper::ReaperTransport::new();
    let project = daw::reaper::ReaperProject::new();
    let marker = daw::reaper::ReaperMarker::new();
    let region = daw::reaper::ReaperRegion::new();
    let tempo_map = daw::reaper::ReaperTempoMap::new();
    let audio_engine = daw::reaper::ReaperAudioEngine::new();
    let midi = daw::reaper::ReaperMidi::new();
    let midi_analysis = daw::reaper::ReaperMidiAnalysis::new();
    let fx = daw::reaper::ReaperFx::new();
    let track = daw::reaper::ReaperTrack::new();
    let routing = daw::reaper::ReaperRouting::new();
    let live_midi = daw::reaper::ReaperLiveMidi::new();
    let ext_state = daw::reaper::ReaperExtState::new();
    let item = daw::reaper::ReaperItem::new();
    let take = daw::reaper::ReaperTake::new();
    let health = daw::reaper::ReaperHealth::new();
    let action_registry = daw::reaper::ReaperActionRegistry::new();
    let input = daw::reaper::ReaperInput::new();
    let toolbar = daw::reaper::ReaperToolbar::new();
    let plugin_loader = daw::reaper::ReaperPluginLoader::new();
    let batch = daw::reaper::batch::BatchExecutor::new();
    let dock_host = daw_reaper_dioxus::ReaperDockHost::new();

    // Import service descriptor functions for method_id routing
    use daw::service::{
        action_registry_service_service_descriptor, audio_engine_service_service_descriptor,
        batch_service_service_descriptor, ext_state_service_service_descriptor,
        fx_service_service_descriptor, health_service_service_descriptor,
        input_service_service_descriptor, item_service_service_descriptor,
        live_midi_service_service_descriptor, marker_service_service_descriptor,
        midi_analysis_service_service_descriptor, midi_service_service_descriptor,
        plugin_loader_service_service_descriptor, project_service_service_descriptor,
        region_service_service_descriptor, routing_service_service_descriptor,
        take_service_service_descriptor, tempo_map_service_service_descriptor,
        toolbar_service_service_descriptor, track_service_service_descriptor,
        transport_service_service_descriptor,
    };

    // Compose all 16 service dispatchers via RoutedHandler
    let daw_handler = RoutedHandler::new()
        .with(
            transport_service_service_descriptor(),
            TransportServiceDispatcher::new(transport),
        )
        .with(
            project_service_service_descriptor(),
            ProjectServiceDispatcher::new(project),
        )
        .with(
            marker_service_service_descriptor(),
            MarkerServiceDispatcher::new(marker),
        )
        .with(
            region_service_service_descriptor(),
            RegionServiceDispatcher::new(region),
        )
        .with(
            tempo_map_service_service_descriptor(),
            TempoMapServiceDispatcher::new(tempo_map),
        )
        .with(
            audio_engine_service_service_descriptor(),
            AudioEngineServiceDispatcher::new(audio_engine),
        )
        .with(
            midi_service_service_descriptor(),
            MidiServiceDispatcher::new(midi),
        )
        .with(
            midi_analysis_service_service_descriptor(),
            MidiAnalysisServiceDispatcher::new(midi_analysis),
        )
        .with(
            fx_service_service_descriptor(),
            FxServiceDispatcher::new(fx),
        )
        .with(
            track_service_service_descriptor(),
            TrackServiceDispatcher::new(track),
        )
        .with(
            routing_service_service_descriptor(),
            RoutingServiceDispatcher::new(routing),
        )
        .with(
            live_midi_service_service_descriptor(),
            LiveMidiServiceDispatcher::new(live_midi),
        )
        .with(
            ext_state_service_service_descriptor(),
            ExtStateServiceDispatcher::new(ext_state),
        )
        .with(
            health_service_service_descriptor(),
            HealthServiceDispatcher::new(health),
        )
        .with(
            item_service_service_descriptor(),
            ItemServiceDispatcher::new(item),
        )
        .with(
            take_service_service_descriptor(),
            TakeServiceDispatcher::new(take),
        )
        .with(
            action_registry_service_service_descriptor(),
            ActionRegistryServiceDispatcher::new(action_registry),
        )
        .with(
            input_service_service_descriptor(),
            InputServiceDispatcher::new(input),
        )
        .with(
            toolbar_service_service_descriptor(),
            ToolbarServiceDispatcher::new(toolbar),
        )
        .with(
            plugin_loader_service_service_descriptor(),
            PluginLoaderServiceDispatcher::new(plugin_loader),
        )
        .with(
            batch_service_service_descriptor(),
            BatchServiceDispatcher::new(batch),
        )
        .with(
            daw_proto::dock_host::dock_host_service_service_descriptor(),
            daw_proto::dock_host::DockHostServiceDispatcher::new(dock_host),
        );

    // Build the connection acceptor from the routed handler
    let acceptor = DawConnectionAcceptor::new(daw_handler);

    // Start Unix socket server
    start_unix_socket_server(acceptor.clone());

    // Start SHM host for hot-reloadable guest processes
    if let Some(bootstrap_sock) = shm_host::start_shm_host(acceptor) {
        // Launch any guest executables found in UserPlugins/fts-extensions/
        guest_loader::launch_guests(&bootstrap_sock);
    }

    info!("DAW bridge registered (21 services, socket + SHM)");
}

// ============================================================================
// Unix Socket Server
// ============================================================================

/// Build the socket path for this REAPER instance.
///
/// Default: `/tmp/fts-daw-{pid}.sock` — matches what reaper-test discovers.
/// Override with `FTS_SOCKET` env var.
fn socket_path() -> PathBuf {
    std::env::var("FTS_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let pid = std::process::id();
            PathBuf::from(format!("/tmp/fts-daw-{pid}.sock"))
        })
}

fn start_unix_socket_server(acceptor: DawConnectionAcceptor) {
    let path = socket_path();

    // Remove stale socket from a previous run
    let _ = std::fs::remove_file(&path);

    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            warn!("Failed to bind Unix socket at {}: {}", path.display(), e);
            return;
        }
    };

    info!("Unix socket server listening on {}", path.display());

    moire::task::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    info!("Client connected via Unix socket");
                    let acceptor = acceptor.clone();
                    moire::task::spawn(async move {
                        let link = vox_stream::StreamLink::unix(stream);
                        let handshake = vox::HandshakeResult {
                            role: vox::SessionRole::Acceptor,
                            our_settings: vox::ConnectionSettings {
                                parity: vox::Parity::Even,
                                max_concurrent_requests: 64,
                            },
                            peer_settings: vox::ConnectionSettings {
                                parity: vox::Parity::Odd,
                                max_concurrent_requests: 64,
                            },
                            peer_supports_retry: true,
                            session_resume_key: None,
                            peer_resume_key: None,
                            our_schema: vec![],
                            peer_schema: vec![],
                        };
                        match vox::acceptor_conduit(vox::BareConduit::new(link), handshake)
                            .on_connection(acceptor)
                            .establish::<vox::DriverCaller>(())
                            .await
                        {
                            Ok((_caller, _session_handle)) => {
                                debug!("Unix socket session established");
                                std::future::pending::<()>().await;
                            }
                            Err(e) => {
                                warn!("Unix socket handshake failed: {:?}", e);
                            }
                        }
                    });
                }
                Err(e) => {
                    warn!("Unix socket accept error: {}", e);
                }
            }
        }
    });
}

// ============================================================================
// RT Thread Detection
// ============================================================================

/// Wraps REAPER's `IsInRealTimeAudio()` function pointer for RT detection.
struct ReaperRtDetector {
    is_in_rt_audio: unsafe extern "C" fn() -> i32,
}

// Safety: The function pointer is a static C function — safe to call from any thread.
unsafe impl Send for ReaperRtDetector {}
unsafe impl Sync for ReaperRtDetector {}

impl RtDetector for ReaperRtDetector {
    fn is_rt_thread(&self) -> bool {
        unsafe { (self.is_in_rt_audio)() != 0 }
    }
}

// ============================================================================
// Timer Callback & Entry Point
// ============================================================================

static APP_INSTANCE: OnceLock<Fragile<App>> = OnceLock::new();

fn get_app() -> Option<&'static Fragile<App>> {
    APP_INSTANCE.get()
}

/// Timer callback for periodic updates (runs on main thread ~30Hz)
/// Deferred eager-load of FTS CLAP plugins. Runs once on the first timer
/// tick so REAPER's CLAP scanner has already finished scanning.
static FX_PLUGINS_LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn timer_callback() {
    // catch_unwind prevents panics from unwinding through the C ABI boundary
    // (which is UB). Any panic inside is logged and the timer keeps running.
    let result = std::panic::catch_unwind(|| {
        // Deferred eager-load: run once after REAPER startup is complete
        if !FX_PLUGINS_LOADED.load(std::sync::atomic::Ordering::Relaxed) {
            FX_PLUGINS_LOADED.store(true, std::sync::atomic::Ordering::Relaxed);
            daw::reaper::eager_load_fx_plugins();
        }

        if let Some(app_fragile) = get_app() {
            let app = app_fragile.get();

            // Process main-thread task queue (reaper-high TaskSupport)
            app.process_tasks();

            // Process daw-allocator main-thread tasks (closures from any thread)
            if let Some(runtime) = FtsRuntime::try_get() {
                runtime.process_main_thread_tasks();
            }

            // Poll all broadcasters for state changes
            daw::reaper::poll_and_broadcast();
            daw::reaper::poll_and_broadcast_fx();
            daw::reaper::poll_and_broadcast_tracks();
            daw::reaper::poll_and_broadcast_items();
            daw::reaper::poll_and_broadcast_routing();
            daw::reaper::poll_and_broadcast_tempo_map();

            // Process deferred toolbar operations
            daw::reaper::process_toolbar_ops();
        }
    });
    if let Err(e) = result {
        warn!("timer_callback panicked: {:?}", e);
    }
}

/// REAPER extension entry point.
#[reaper_extension_plugin]
fn plugin_main(context: PluginContext) -> Result<(), Box<dyn Error>> {
    // Initialize tracing to /tmp/daw-bridge.log
    let log_file =
        std::fs::File::create("/tmp/daw-bridge.log").expect("Failed to create /tmp/daw-bridge.log");
    tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(log_file))
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into()),
        )
        .init();

    info!("daw-bridge starting...");

    // Store plugin context for PluginLoaderService before REAPER consumes it
    daw::reaper::set_plugin_context(context);

    // Initialize REAPER high-level API
    match HighReaper::load(context).setup() {
        Ok(_) => {
            info!("REAPER high-level API initialized");
            // Register hookcommand callback so custom action closures fire
            // when actions are triggered via Main_OnCommandEx / KBD_OnMainActionEx.
            if let Err(e) = HighReaper::get().wake_up() {
                debug!("REAPER high-level API wake_up failed: {e}");
            }
        }
        Err(_) => debug!("REAPER high-level API already initialized"),
    }

    // Initialize RT-safe allocator runtime.
    // Must happen after REAPER loads but before any RT audio processing.
    let reaper = HighReaper::get();
    if let Some(is_in_rt_audio) = reaper.medium_reaper().low().pointers().IsInRealTimeAudio {
        let detector = ReaperRtDetector { is_in_rt_audio };
        FtsRuntime::init(
            &ALLOCATOR,
            FtsRuntimeConfig {
                dealloc_channel_capacity: 10_000,
                rt_detector: Box::new(detector),
            },
        );
        info!("RT allocator initialized (async deallocation enabled)");
    } else {
        warn!("IsInRealTimeAudio not available — RT allocator running without async deallocation");
    }

    // Create a medium-level API session
    let session = ReaperSession::load(context);

    // Create the App (initializes Global/TaskSupport)
    let app = App::new(session)?;

    // Initialize (register DAW dispatcher + socket server)
    app.initialize()?;

    // FTS CLAP plugin eager-loading is deferred to the first timer tick
    // so REAPER's CLAP scanner finishes first (avoids dlopen conflicts).

    // Store app globally
    APP_INSTANCE
        .set(Fragile::new(app))
        .expect("App already initialized");

    // Register timer callback for periodic updates and Extensions menu hook
    let app = APP_INSTANCE.get().expect("App should be initialized").get();
    let mut session = app.session.borrow_mut();
    session.plugin_register_add_timer(timer_callback)?;
    daw::reaper::register_extension_menu(&mut session);

    // Register project file importer (Ableton .als → REAPER)
    let import_register = reaper_medium::OwnedProjectImportRegister::new(
        project_import::want_project_file,
        project_import::enum_file_extensions,
        project_import::load_project,
    );
    session.plugin_register_add_project_import(import_register)?;
    info!("Project import handler registered (.als)");

    drop(session);

    info!("daw-bridge initialized successfully");
    Ok(())
}

//! REAPER ↔ DAW bridge extension.
//!
//! Loads daw-reaper's service implementations inside REAPER and exposes them
//! via Unix socket and SHM so external processes (tests, FTS, CLI tools)
//! can control the DAW through roam RPC.

mod guest_loader;
mod routed_handler;
mod shm_host;

use fragile::Fragile;
use reaper_high::{MainTaskMiddleware, Reaper as HighReaper};
use reaper_low::PluginContext;
use reaper_macros::reaper_extension_plugin;
use reaper_medium::ReaperSession;
use std::cell::RefCell;
use std::error::Error;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use tokio::net::UnixListener;
use tracing::{debug, info, warn};

use routed_handler::{DawConnectionAcceptor, RoutedHandler};

// Service dispatchers for method ID routing
use daw::service::{
    AudioEngineServiceDispatcher, ExtStateServiceDispatcher, FxServiceDispatcher,
    HealthServiceDispatcher, ItemServiceDispatcher, LiveMidiServiceDispatcher,
    MarkerServiceDispatcher, MidiAnalysisServiceDispatcher, MidiServiceDispatcher,
    ProjectServiceDispatcher, RegionServiceDispatcher, RoutingServiceDispatcher,
    TakeServiceDispatcher, TempoMapServiceDispatcher, TrackServiceDispatcher,
    TransportServiceDispatcher,
};

// ============================================================================
// Global State (TaskSupport for main thread dispatch)
// ============================================================================

use crossbeam_channel::{Receiver, Sender};
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

    // Import service descriptor functions for method_id routing
    use daw::service::{
        audio_engine_service_service_descriptor, ext_state_service_service_descriptor,
        fx_service_service_descriptor, health_service_service_descriptor,
        item_service_service_descriptor, live_midi_service_service_descriptor,
        marker_service_service_descriptor, midi_analysis_service_service_descriptor,
        midi_service_service_descriptor, project_service_service_descriptor,
        region_service_service_descriptor, routing_service_service_descriptor,
        take_service_service_descriptor, tempo_map_service_service_descriptor,
        track_service_service_descriptor, transport_service_service_descriptor,
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

    info!("DAW bridge registered (16 services, socket + SHM)");
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
                        let link = roam_stream::StreamLink::unix(stream);
                        match roam::acceptor(roam::BareConduit::new(link))
                            .on_connection(acceptor)
                            .establish::<roam::DriverCaller>(())
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
// Timer Callback & Entry Point
// ============================================================================

static APP_INSTANCE: OnceLock<Fragile<App>> = OnceLock::new();

fn get_app() -> Option<&'static Fragile<App>> {
    APP_INSTANCE.get()
}

/// Timer callback for periodic updates (runs on main thread ~30Hz)
extern "C" fn timer_callback() {
    use std::sync::atomic::{AtomicBool, AtomicU64};
    static TICK_COUNT: AtomicU64 = AtomicU64::new(0);
    static AUDIO_INITIALIZED: AtomicBool = AtomicBool::new(false);
    let tick = TICK_COUNT.fetch_add(1, Ordering::Relaxed);

    if tick == 0 {
        info!("timer_callback: first tick — timer is running");
    }

    // Log every 30 ticks (~1s) for the first 5 seconds, for CI diagnostics
    if tick > 0 && tick <= 150 && tick % 30 == 0 {
        info!("timer_callback: tick {tick}");
    }

    // In headless mode, REAPER's main loop goes idle without an audio engine.
    // We must call Audio_Init() AFTER REAPER finishes its own initialization
    // (which runs after plugin_main). Tick 5 (~150ms) is safely past that point.
    if tick == 5 && !AUDIO_INITIALIZED.swap(true, Ordering::Relaxed) {
        info!("timer_callback: calling Audio_Init() to activate dummy audio driver");
        let reaper = HighReaper::get();
        reaper.medium_reaper().low().Audio_Init();
    }

    // catch_unwind prevents panics from unwinding through the C ABI boundary
    // (which is UB). Any panic inside is logged and the timer keeps running.
    let result = std::panic::catch_unwind(|| {
        if let Some(app_fragile) = get_app() {
            let app = app_fragile.get();

            // Process main-thread task queue
            app.process_tasks();

            // Poll all broadcasters for state changes
            daw::reaper::poll_and_broadcast();
            daw::reaper::poll_and_broadcast_fx();
            daw::reaper::poll_and_broadcast_tracks();
            daw::reaper::poll_and_broadcast_items();
            daw::reaper::poll_and_broadcast_routing();
            daw::reaper::poll_and_broadcast_tempo_map();
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

    // Initialize REAPER high-level API
    match HighReaper::load(context).setup() {
        Ok(_) => info!("REAPER high-level API initialized"),
        Err(_) => debug!("REAPER high-level API already initialized"),
    }

    // Create a medium-level API session
    let session = ReaperSession::load(context);

    // Create the App (initializes Global/TaskSupport)
    let app = App::new(session)?;

    // Initialize (register DAW dispatcher + socket server)
    app.initialize()?;

    // Store app globally
    APP_INSTANCE
        .set(Fragile::new(app))
        .expect("App already initialized");

    // Register timer callback for periodic updates
    let app = APP_INSTANCE.get().expect("App should be initialized").get();
    let mut session = app.session.borrow_mut();
    session.plugin_register_add_timer(timer_callback)?;
    drop(session);

    info!("daw-bridge initialized successfully");
    Ok(())
}

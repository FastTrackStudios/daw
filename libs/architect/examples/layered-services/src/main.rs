//! Layered services — the `#[architect::rpc]` + `Layer<B>`
//! composition story, end-to-end, in one runnable file.
//!
//! # What this shows
//!
//! 1. Declare service traits with `#[architect::rpc]`. The derive
//!    emits, per trait: the user trait (kept sync), a hidden async
//!    mirror, a `<T>Host` bridge, a `<T>Client` async caller, a
//!    `serve(backend)` mount verb, a `layer(backend)` immediate-bind
//!    shortcut, and a `Service` token that slots into `layers!`.
//!
//! 2. Implement those traits on two backends — one "live" with real
//!    state, one "mock" stateless. Both impl `Services`, declaring
//!    their canonical bundles via `layers![…::Service]`.
//!
//! 3. Three deployment shapes from the same trait:
//!
//!    - **Direct sync.** Call trait methods on the backend. No
//!      router, no dispatcher, no future. Zero overhead beyond a
//!      virtual call (monomorphized away in release builds).
//!
//!    - **Backend swap at mount.** `LiveBackend.into_router()` vs
//!      `MockBackend.into_router()` — same trait, same call site,
//!      different runtime behavior.
//!
//!    - **Per-service override.** Start from a live bundle, merge in
//!      a pre-mounted service bound to a different backend. Last
//!      merge wins on method-id collision — that's how mocks and
//!      bolt-ons compose.
//!
//! # What this deliberately doesn't show
//!
//! Transport. `LayerRouter` already implements
//! `vox::Handler<DriverReplySink>` — to actually drive it from a
//! client you plug it into a vox `Driver` paired with a transport.
//! That's covered elsewhere:
//!
//! - **Axum WebSocket**: see `examples/custom-server/` — wraps the
//!   same router shape in `architect::axum_ws::serve`.
//! - **Unix socket / IPC**: see `daw-bridge` in the FastTrackStudio
//!   daw repo — the dock-host extension mounts its
//!   `Reaper.into_router()` on a Unix socket so out-of-process tools
//!   share the same client types.
//!
//! Run with:
//!   cargo run -p example-layered-services

use std::sync::{Arc, Mutex};

use architect::{
    HasDispatcher, Layer, LayerGraph, LayerNode, LayerRouter, Services,
    dispatch::CurrentThreadDispatcher, layers,
};
use example_memory::ExampleRepoMemory;
use example_proto::{ExampleRepo, example_repo_layer};
use example_stub_backend::StubBackend;
use tracing::info;

/// One generic mount fn — identical code for an in-memory repo, a
/// third-party stub, or (with the `server` feature) the SeaORM storage.
/// The `#[derive(Entity)]`-emitted `<Entity>RepoLayer` token makes the
/// repo a `Layer` participant, so the backend is injected at this call
/// site and nothing downstream changes.
fn mount<B>(backend: B) -> LayerRouter
where
    B: ExampleRepo + Services + Clone + Send + Sync + 'static,
{
    backend.into_router()
}

// ── Service traits ────────────────────────────────────────────────────
//
// Each `#[architect::rpc]` trait lives in its own module so the
// emitted `Service` token (and the `serve` / `layer` / `<T>Client`
// items that come with it) gets its own namespace — `counter::Service`,
// `greeter::Service`, etc. This is the same pattern daw-proto uses
// for its 26-service surface.

pub mod counter {
    /// Trivial counter. Sync methods are bridged through the
    /// backend's dispatcher when invoked via the async client; called
    /// directly they cost a single virtual call.
    // `sync_client` opts into the blocking `CounterSyncClient` facade
    // (section 7) — emitted only on request so non-blocking consumers
    // don't carry it.
    #[architect::rpc(sync_client)]
    pub trait Counter {
        fn increment(&self, by: i64) -> i64;
        fn current(&self) -> i64;
    }
}

pub mod greeter {
    /// Trivial greeter. Borrowed args (`&str`) auto-convert to
    /// `String` on the async-mirror side; the sync trait keeps the
    /// borrowed signature so direct callers don't pay an allocation.
    #[architect::rpc]
    pub trait Greeter {
        fn greet(&self, name: &str) -> String;
    }
}

pub mod notes {
    /// Ambient per-call context — the "which project" axis the DAW
    /// threads through every method. Declared ONCE on the attribute;
    /// methods don't repeat it.
    #[derive(Clone, Debug, PartialEq, facet::Facet)]
    pub struct Room {
        pub id: u32,
    }

    #[architect::rpc(context = Room)]
    pub trait Notes {
        fn add(&self, text: String) -> usize;
        fn all(&self) -> Vec<String>;
    }
}

pub mod ticker {
    /// One counter change. `#[subscribe]` event payloads should carry
    /// full state (idempotent re-application), not diffs.
    #[derive(Clone, Debug, PartialEq, facet::Facet)]
    pub struct TickEvent {
        pub value: i64,
    }

    /// A trait with a `#[subscribe]` declaration. The marker is
    /// stripped from this (sync, callable) trait; the macro emits a
    /// `TickerStream` vox sibling carrying `async fn ticks(&self,
    /// sink: Tx<TickEvent>)`, a `TickerStreamSource` backend contract
    /// (`fn ticks_hub(&self) -> &PubSub<TickEvent>`), and
    /// `stream_serve` / `stream_layer` / `StreamService` mount verbs.
    #[architect::rpc]
    pub trait Ticker {
        fn tick(&self) -> i64;

        /// Every counter change, as it happens.
        #[subscribe]
        fn ticks(&self) -> TickEvent;
    }
}

// Each `#[rpc]` expansion emits a `prelude` module with glob-safe
// names — the trait plus `CounterService` / `counter_layer` /
// `counter_serve` / `CounterClient` (vox-gated). One glob per service
// replaces the hand-renamed five-item re-export block.
use counter::prelude::*;
use greeter::prelude::*;
use notes::Room;
use notes::prelude::*;
use ticker::TickEvent;
use ticker::prelude::*;

// ── Backends ──────────────────────────────────────────────────────────
//
// A backend is the "DI container" for our architecture — one struct
// that implements every service trait. State lives on the struct;
// service methods reach into `self` rather than dispatching through
// the router. Different backends provide different state and
// different impls.

/// Live backend: holds real counter state. Both service traits impl
/// directly against this type. The `HasDispatcher` derive points the
/// rpc bridge at a default-constructible dispatcher — the manual
/// four-line impl is only needed for dispatchers with runtime state.
///
/// The `ticks` hub is the publish side of the `#[subscribe]` stream:
/// the backend pushes into it wherever state changes; the emitted
/// stream host attaches every subscriber sink. `with_replay(1)` hands
/// late subscribers the last event on attach (cheap snapshot for
/// state-shaped streams).
#[derive(Clone, HasDispatcher)]
#[dispatch(CurrentThreadDispatcher)]
pub struct LiveBackend {
    counter: Arc<Mutex<i64>>,
    ticks: architect::PubSub<TickEvent>,
}

impl Default for LiveBackend {
    fn default() -> Self {
        Self {
            counter: Arc::default(),
            ticks: architect::PubSub::sliding(64).with_replay(1),
        }
    }
}

impl Counter for LiveBackend {
    fn increment(&self, by: i64) -> i64 {
        let mut g = self.counter.lock().expect("counter poisoned");
        *g += by;
        *g
    }
    fn current(&self) -> i64 {
        *self.counter.lock().expect("counter poisoned")
    }
}

impl Greeter for LiveBackend {
    fn greet(&self, name: &str) -> String {
        format!("Hello, {name}! (from LiveBackend)")
    }
}

impl Ticker for LiveBackend {
    fn tick(&self) -> i64 {
        let mut g = self.counter.lock().expect("counter poisoned");
        *g += 1;
        // Publish-on-write: every successful mutation broadcasts the
        // new authoritative state to every subscriber.
        self.ticks.publish(TickEvent { value: *g });
        *g
    }
}

impl TickerStreamSource for LiveBackend {
    fn ticks_hub(&self) -> &architect::PubSub<TickEvent> {
        &self.ticks
    }
}

/// Per-room note store. `context = Room` gave every method a leading
/// `ctx: &Room` — declared once on the trait, not per method.
#[derive(Clone, Default, HasDispatcher)]
#[dispatch(CurrentThreadDispatcher)]
pub struct NotesBackend {
    rows: Arc<Mutex<Vec<(u32, String)>>>,
}

impl Notes for NotesBackend {
    fn add(&self, ctx: &Room, text: String) -> usize {
        let mut rows = self.rows.lock().expect("notes poisoned");
        rows.push((ctx.id, text));
        rows.iter().filter(|(r, _)| *r == ctx.id).count()
    }

    fn all(&self, ctx: &Room) -> Vec<String> {
        self.rows
            .lock()
            .expect("notes poisoned")
            .iter()
            .filter(|(r, _)| *r == ctx.id)
            .map(|(_, t)| t.clone())
            .collect()
    }
}

impl Services for NotesBackend {
    fn layers() -> impl Layer<NotesBackend> {
        layers![NotesService]
    }
}

/// Mock backend: stateless, returns canned values. Same traits, same
/// bundle declaration, different runtime behavior. This is the
/// "swap the layer" testing pattern Effect users get from
/// `Service.DefaultWithoutDependencies` — in Rust it's just a
/// different backend struct that impls the same traits.
#[derive(Clone, Copy, Default, HasDispatcher)]
#[dispatch(CurrentThreadDispatcher)]
pub struct MockBackend;

impl Counter for MockBackend {
    fn increment(&self, _by: i64) -> i64 {
        0
    }
    fn current(&self) -> i64 {
        0
    }
}

impl Greeter for MockBackend {
    fn greet(&self, name: &str) -> String {
        format!("[mock] hi {name}")
    }
}

// ── Bundle declarations ───────────────────────────────────────────────
//
// `impl Services for B` declares the canonical service surface for
// backend `B`. The body is just a `layers![ … ]` list of service
// tokens — one ident per service. The return type `impl Layer<B>`
// recursively checks (at compile time) that every service token has
// a `Bind<B>` impl. Missing impls surface as compile errors at the
// `.provide(...)` call site, naming the missing trait.

impl Services for LiveBackend {
    fn layers() -> impl Layer<LiveBackend> {
        // The stream sibling is one more token — `layers![...]` mounts
        // the base service and its subscription service side by side.
        layers![
            CounterService,
            GreeterService,
            TickerService,
            TickerStreamService
        ]
    }
}

impl Services for MockBackend {
    fn layers() -> impl Layer<MockBackend> {
        layers![CounterService, GreeterService]
    }
}

// ── Walk through the shapes ───────────────────────────────────────────

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let live = LiveBackend::default();

    // ── 1. Direct sync ───────────────────────────────────────────────
    //
    // No router. No dispatcher. No async. The trait methods are
    // callable directly on the backend with their original sync
    // signatures — zero abstraction overhead beyond one virtual
    // call. This is the path daw-reaper's extension runtime uses
    // for REAPER's main-thread hot loop.
    info!("── 1. Direct sync ───────────────────────────────────────");
    let n = Counter::increment(&live, 5);
    let greeting = Greeter::greet(&live, "world");
    info!(counter = n, %greeting, "direct sync calls returned");

    // ── 2. Build a router via the canonical bundle ───────────────────
    //
    // `into_router()` is sugar for `Self::layers().provide(self)`.
    // Returns a `LayerRouter` — method-id-keyed dispatch table that
    // implements `vox::Handler<DriverReplySink>`. Plug it into any
    // vox transport and out-of-process clients can call every
    // service in the bundle.
    info!("── 2. Build router from canonical bundle ───────────────");
    let live_router = live.clone().into_router();
    info!(services = live_router.len(), "LiveBackend router built");

    // ── 3. Backend swap ──────────────────────────────────────────────
    //
    // Same trait, same bundle declaration, different backend type.
    // The router for `MockBackend` carries the mock impls of every
    // service. Test code that uses the live router in production
    // just swaps the construction line:
    //   let router = MockBackend.into_router();
    info!("── 3. Backend swap ─────────────────────────────────────");
    let mock_router = MockBackend.into_router();
    info!(services = mock_router.len(), "MockBackend router built");

    // ── 4. Per-service override (hybrid router) ──────────────────────
    //
    // Start from the live bundle, merge in a single service
    // pre-bound to a different backend. `LayerRouter` resolves
    // duplicate method IDs by **last merge wins**, so `Greeter`
    // calls on this router now route to MockBackend while
    // `Counter` calls keep hitting live state.
    //
    // This is the bolt-on pattern. Real use in daw-bridge:
    //   Reaper::layers()
    //       .merge(dock_host::layer(dock_host_backend))
    //       .provide(Reaper)
    // — the dock-host bolt-on uses a Dioxus-side backend that
    // Reaper itself doesn't impl.
    info!("── 4. Per-service override ─────────────────────────────");
    let hybrid_router = LiveBackend::layers()
        .merge(greeter_layer(MockBackend))
        .provide(live.clone());
    info!(
        services = hybrid_router.len(),
        "hybrid router (live counter + mock greeter) built"
    );

    // ── 5. Multi-backend app router ──────────────────────────────────
    //
    // An app's vox endpoint usually fronts many backends — one per
    // feature. `router![]` is the mount registry: each entry is a
    // backend, its whole canonical `Services` bundle mounts in one
    // line. Adding a feature to the app = adding one expression here
    // (the bundle itself is declared once, in the feature crate).
    info!("── 5. Multi-backend app router (router![]) ─────────────");
    let app_router = architect::router![
        live.clone(),             // Counter + Greeter + Ticker + TickerStream
        ExampleRepoMemory::new(), // ExampleRepo
    ];
    assert_eq!(app_router.len(), 5, "two bundles, five services");
    info!(
        services = app_router.len(),
        "app router stitched from two backends"
    );

    // Collision rule: later entries win (same as Layer::merge). A mock
    // mounted after the live backend takes over its method ids.
    let _overridden = architect::router![live.clone(), MockBackend];

    // ── 6. #[subscribe] stream sibling, end to end ───────────────────
    //
    // `#[subscribe] fn ticks(&self) -> TickEvent` emitted a vox sibling
    // service (`TickerStream`, mounted above as `TickerStreamService`).
    // Drive it the way a real client does: typed `TickerStreamClient`
    // over the in-process transport (`architect::local`), channel pair
    // bound by the RPC call. On the Dioxus side the same subscription
    // is `architect::use_stream`.
    info!("── 6. #[subscribe] stream sibling (end to end) ─────────");
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        use architect::{LocalServer, Scope};

        let scope = Scope::new();
        let local = LocalServer::serve(live.clone().into_router(), scope.clone());
        let stream: TickerStreamClient = local.establish().await.expect("local establish");

        let (tx, mut rx) = vox::channel::<TickEvent>();
        // vox scopes channels to their request, so the subscribe call stays
        // in flight for the life of the subscription — spawn it and keep the
        // handle; aborting it is the unsubscribe. (`architect::use_stream`
        // does exactly this race internally.)
        let subscription = tokio::spawn(async move { stream.ticks(tx).await });

        // Wait for the attach before publishing — the subscribe call is in
        // flight on another task, so give its attach a moment to land.
        // (`publish` returns the live subscriber count; the warm-up event
        // that lands post-attach is skipped below.)
        while live.ticks.publish(TickEvent {
            value: Counter::current(&live),
        }) == 0
        {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let a = Ticker::tick(&live); // publish-on-write
        let b = Ticker::tick(&live);

        // `SelfRef` lends the decoded event while the receive buffer is
        // alive — copy the payload out inside `map` (same pattern as
        // `architect::use_stream`). Replay + the warm-up publish may precede
        // `a`, so collect until `b` arrives and assert the tail.
        let mut received = Vec::new();
        while received.last() != Some(&b) {
            let event = rx.recv().await.expect("recv").expect("stream still open");
            let _ = event.map(|ev| received.push(ev.value));
        }
        assert_eq!(&received[received.len() - 2..], &[a, b]);
        info!(?received, "subscriber received publish-on-write events");
        subscription.abort(); // unsubscribe: cancel the in-flight call
    });

    // ── 7. Blocking facade (sync-client) ─────────────────────────────
    //
    // Sync code that talks to a *remote* backend (a plugin's action
    // handler, a CLI without an async main) uses the emitted
    // `<Trait>SyncClient`: same signatures, no .await, each call driven
    // on a background runtime. In-process callers skip this — the user
    // trait is already sync (section 1).
    info!("── 7. Blocking facade (<Trait>SyncClient) ──────────────");
    {
        use architect::{BlockingCaller, LocalServer, Scope};

        // Establish the async client on the runtime…
        let scope = Scope::new();
        let local = LocalServer::serve(live.clone().into_router(), scope.clone());
        let counter_async: CounterClient =
            rt.block_on(async { local.establish().await.expect("local establish") });

        // …then drive it from plain sync code on the main thread.
        let counter = CounterSyncClient::new(
            counter_async,
            BlockingCaller::from_handle(rt.handle().clone()),
        );
        let n = counter.increment(10).expect("increment over the wire");
        let now = counter.current().expect("current over the wire");
        assert_eq!(n, now);
        info!(n, "sync facade round-tripped without an async context");
    }

    // ── 8. Ambient context (context = Room) ──────────────────────────
    //
    // The scoped client binds the context once; every call rides it.
    // The raw client keeps the explicit first argument for callers
    // juggling several scopes.
    info!("── 8. Ambient context (scoped client) ──────────────────");
    rt.block_on(async {
        use architect::{LocalServer, Scope};
        let scope = Scope::new();
        let local = LocalServer::serve(NotesBackend::default().into_router(), scope.clone());
        let raw: NotesClient = local.establish().await.expect("local establish");

        let room_a = raw.clone().scoped(Room { id: 1 });
        let room_b = raw.scoped(Room { id: 2 });
        room_a.add("alpha".into()).await.expect("add");
        room_b.add("beta".into()).await.expect("add");
        room_a.add("gamma".into()).await.expect("add");

        let a = room_a.all().await.expect("all");
        let b = room_b.all().await.expect("all");
        assert_eq!(a, vec!["alpha".to_string(), "gamma".to_string()]);
        assert_eq!(b, vec!["beta".to_string()]);
        info!(?a, ?b, "context bound once per scope, isolated per room");
    });

    // ── Introspection ────────────────────────────────────────────────
    //
    // `Layer<B>` carries the `Descriptors` walker as a supertrait, so
    // you can list a bundle's services before binding a backend.
    // Useful for capability negotiation, schema export, generated
    // docs.
    let descriptors: Vec<_> = LiveBackend::layers()
        .descriptors()
        .iter()
        .map(|d| d.service_name)
        .collect();
    info!(?descriptors, "LiveBackend canonical surface");

    // ── What you'd do next ───────────────────────────────────────────
    //
    // To actually drive any of these routers from a client:
    //
    //   - Pair the router with a vox `Driver` and a transport. See
    //     `examples/custom-server/` for the axum WebSocket variant
    //     and `daw-bridge` for the Unix-socket variant.
    //
    //   - Async client types (`CounterClient`, `GreeterClient`) are
    //     emitted by the derive when the consumer crate enables its
    //     `vox` feature. They wrap a `vox::Caller` — a handle to
    //     the driver — and expose the trait's methods as `async fn`.
    //
    // The composition surface shown here doesn't change between
    // in-process and cross-process: the `LayerRouter` is the same
    // object, only the transport in front of it differs.

    // ── Entity repos are layers too ──────────────────────────────────
    //
    // Everything above used hand-written `#[architect::rpc]` traits. A
    // `#[derive(Entity)]` repo is the same shape — an all-async vox
    // service — so the derive emits an `ExampleRepoLayer` token and the
    // repo composes/swaps through the layer system identically.

    // Same call site, three interchangeable backends. Neither `mount`
    // nor any consumer code changes when the backend does.
    let _mem = mount(ExampleRepoMemory::new());
    let _stub = mount(StubBackend::with_seed_data());

    let mem_surface: Vec<_> = ExampleRepoMemory::layers()
        .descriptors()
        .iter()
        .map(|d| d.service_name)
        .collect();
    let stub_surface: Vec<_> = StubBackend::layers()
        .descriptors()
        .iter()
        .map(|d| d.service_name)
        .collect();
    info!(
        ?mem_surface,
        ?stub_surface,
        "same repo surface, different backend — zero consumer change"
    );

    // Per-service override: keep the in-memory bundle, but bind the repo
    // to the third-party stub instead (last-merge-wins on method id).
    let _hybrid = ExampleRepoMemory::layers()
        .merge(example_repo_layer(StubBackend::with_seed_data()))
        .provide(ExampleRepoMemory::new());
    info!("override: repo impl swapped at the call site, bundle unchanged");

    // ── Dependency planner ───────────────────────────────────────────
    //
    // For a multi-node backend graph, declare what each node requires +
    // provides and get a validated build order — or a precise diagnostic.
    // (The async construction side — building these with deps,
    // memoization, and scoped teardown — is `architect::Resource`; see
    // `examples/app/server` for the real graceful-shutdown wiring.)
    info!("── Layer planner ───────────────────────────────────────");
    let plan = LayerGraph::new()
        .add(LayerNode::new("config", [] as [&str; 0], ["config"]))
        .add(LayerNode::new("db", ["config"], ["db"]))
        .add(LayerNode::new("repo", ["db"], ["repo"]))
        .plan()
        .expect("valid graph");
    info!(build_order = ?plan.build_order, "planned backend build order");

    // A deliberate mistake surfaces a diagnostic instead of a panic far
    // away at wiring time.
    let broken = LayerGraph::new()
        .add(LayerNode::new("repo", ["db"], ["repo"])) // nothing provides "db"
        .plan();
    match broken {
        Err(e) => info!(diagnostic = %e, "planner caught a wiring mistake"),
        Ok(_) => unreachable!("graph should be missing a provider"),
    }
}

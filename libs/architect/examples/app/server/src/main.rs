//! Reference axum + vox server binary.
//!
//! A thin shell: it builds a backend via a `Resource` (architect's
//! construction layer) under a `Scope`, hands the backend to
//! `app_server::vox_router`, serves, and on shutdown closes the scope —
//! running any finalizers (e.g. closing the db pool) in reverse order.
//! All HTTP/vox wiring lives in the lib so the e2e tests mount the exact
//! same router.

use app_server::vox_router;
use example::architect::Scope;
use tracing::info;

/// Compact, readable logging. The default keeps our own logs at debug and
/// everything else at info, while muting the noisy neighbours: the
/// per-query sqlx/sea-orm chatter and — importantly — cranelift-jit, which
/// `info!`-logs the full CLIF IR of every function the vox JIT compiles.
/// Raise any of them with e.g. `RUST_LOG=sqlx=info`, or override the whole
/// filter with `RUST_LOG=...` for ad-hoc debugging.
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        [
            "info",
            "app_server=debug",
            "sqlx=warn",
            "sqlx::query=warn",
            "sea_orm=warn",
            // cranelift-jit dumps each compiled function's CLIF at info.
            "cranelift_jit=warn",
            "cranelift_codegen=warn",
        ]
        .join(",")
        .into()
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false) // drop the `sqlx::query:` / module prefix
        .compact()
        .init();
}

// ── Backend-specific construction ─────────────────────────────────────
//
// Each backend is built as a `Resource<Repo>`. The db backend wraps the
// pool in `acquire_release` so it's closed on shutdown; the in-memory
// backend is a trivial `succeed`.

#[cfg(feature = "backend-db")]
mod backend {
    use example::architect::Resource;
    use example::backend_db::{ExampleRepoStorage, Migrator};
    use sea_orm::{Database, DatabaseConnection};
    use sea_orm_migration::MigratorTrait;

    pub type Repo = ExampleRepoStorage<DatabaseConnection>;
    pub const LABEL: &str = "sqlite";

    pub fn resource() -> Resource<Repo> {
        // config → pool (acquire_release: closed on scope teardown) → repo
        Resource::from_fn(|_| async {
            Ok(std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://./example.db?mode=rwc".into()))
        })
        .and_then(|url| {
            Resource::acquire_release(
                Resource::from_fn(move |_| async move {
                    let db = Database::connect(&url).await?;
                    Migrator::up(&db, None).await?;
                    Ok(db)
                }),
                |db: DatabaseConnection| async move {
                    tracing::info!("closing database pool");
                    let _ = db.close().await;
                },
            )
        })
        .map(ExampleRepoStorage::new)
    }
}

// Picks backend-memory only when backend-db isn't also enabled — keeps
// `cargo build --all-features` resolvable.
#[cfg(all(not(feature = "backend-db"), feature = "backend-memory"))]
mod backend {
    use example::architect::Resource;
    use example::backend_memory::ExampleRepoMemory;

    pub type Repo = ExampleRepoMemory;
    pub const LABEL: &str = "in-memory";

    pub fn resource() -> Resource<Repo> {
        Resource::succeed(ExampleRepoMemory::new())
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    init_tracing();

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:4040".into());

    // Build the backend under a scope so resources (the db pool) get a
    // finalizer; the scope closes after the server stops.
    let scope = Scope::new();
    let repo = backend::resource().build(&scope).await?;
    info!(backend = backend::LABEL, %bind_addr, "starting example server");

    // The collaborative notes doc: file-persisted (set COLLAB_DATA_DIR
    // to relocate), compacted as it grows, served to every replica over
    // the same vox socket as the CRUD services.
    let collab_dir = std::env::var("COLLAB_DATA_DIR").unwrap_or_else(|_| "./collab-data".into());
    let collab = app_server::Collab::open(&collab_dir)
        .await
        .map_err(|e| eyre::eyre!("open collab doc: {e}"))?;
    info!(dir = %collab_dir, "collab doc ready");

    let app = vox_router(repo, collab);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("HTTP listening on http://{bind_addr}");
    info!("  Health:  http://{bind_addr}/api/health");
    info!("  Vox WS:  ws://{bind_addr}/vox");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("shutdown — running finalizers");
    scope.close().await;
    Ok(())
}

/// Resolve on Ctrl-C so the server can shut down gracefully and run the
/// scope's finalizers (close the db pool) before exiting.
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

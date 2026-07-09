//! Migration CLI. Re-uses sea-orm-migration's built-in CLI runner so
//! the same subcommands (`up`, `down`, `status`, `refresh`, `reset`,
//! `fresh`) work as in any other SeaORM project.
//!
//! Connection string comes from `DATABASE_URL`; defaults to a SQLite
//! file in the project root so `cargo run -p app-db -- up`
//! Just Works in a fresh checkout.

use sea_orm_migration::prelude::*;
use std::env;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sea_orm_migration=info".into()),
        )
        .init();

    if env::var("DATABASE_URL").is_err() {
        // Sensible default — matches what apps/server uses below.
        // SAFETY: single-threaded at process start, no env reader yet.
        unsafe { env::set_var("DATABASE_URL", "sqlite://./example.db?mode=rwc") };
    }

    cli::run_cli(example_db::Migrator).await;
    Ok(())
}

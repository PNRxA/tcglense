//! Shared fixtures for the crate's `#[cfg(test)]` modules: a canonical validated
//! [`Config`] and a migrated in-memory SQLite connection. Kept in one place so the
//! unit and integration tests build their state the same way; per-test tweaks use
//! struct-update syntax (`Config { field: …, ..test_config() }`).

use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

use crate::{config::Config, migrator::Migrator};

/// A canonical, fully-populated [`Config`] for tests. All thirteen fields carry sane,
/// offline-safe defaults (in-memory DB, no card sync); a test that cares about a
/// particular field overrides just that one via `Config { field: …, ..test_config() }`.
pub(crate) fn test_config() -> Config {
    Config {
        database_url: "sqlite::memory:".to_string(),
        jwt_secret: "integration-test-signing-secret-0123456789".to_string(),
        access_token_expiry_minutes: 15,
        refresh_token_expiry_days: 30,
        cookie_secure: false,
        host: "127.0.0.1".to_string(),
        port: 8080,
        public_site_url: "https://tcglense.example".to_string(),
        data_dir: std::path::PathBuf::from("./data"),
        scryfall_user_agent: "TCGLense/test".to_string(),
        sync_on_startup: false,
        sync_interval_hours: 24,
        seed_dummy_data: false,
    }
}

/// Connect to a fresh in-memory SQLite database and run all migrations.
///
/// The pool is pinned to a single connection. With `sqlite::memory:` every physical
/// connection is its own separate, empty database, so a multi-connection pool could
/// hand a caller an unmigrated DB; one connection keeps the migrated schema + data
/// consistent across every query (and any future concurrent one).
pub(crate) async fn migrated_memory_db() -> DatabaseConnection {
    let mut opts = ConnectOptions::new("sqlite::memory:");
    opts.max_connections(1).min_connections(1);
    let db = Database::connect(opts)
        .await
        .expect("connect in-memory sqlite");
    Migrator::up(&db, None).await.expect("run migrations");
    db
}

//! Database connection setup.
//!
//! Builds the SeaORM connect options, applying SQLite performance pragmas to every
//! pooled connection. Tuned for the read-heavy price/collection workloads this app
//! is built for (issue #11).

use sea_orm::{ConnectOptions, sqlx::sqlite::SqliteJournalMode};

/// Build [`ConnectOptions`] for `database_url` with SQLite performance pragmas.
///
/// - **WAL journal mode** (`journal_mode=WAL`) stops reads and writes from blocking
///   each other at the SQLite layer: commits append to a `-wal` file instead of
///   rewriting a rollback journal, cutting fsync/lock contention. WAL is a persistent,
///   database-level setting, but requesting it on every connection ensures a
///   freshly-created DB file starts in WAL mode. (Note: SeaORM defaults the SQLite
///   pool to a single connection, so exploiting WAL's *many concurrent readers* in
///   one process would additionally require raising `max_connections` — out of scope
///   here.)
/// - **`cache_size = -20000`** gives each connection a ~20 MB page cache (a negative
///   value is a size in KiB, not pages) so hot indexes/pages stay resident in RAM.
///   Unlike WAL, `cache_size` is a *per-connection* pragma that is not persisted to
///   the database file, so it must be applied to the options used for every
///   connection in the pool — which is exactly what `map_sqlx_sqlite_opts` does.
///
/// These pragmas are a no-op or harmless on a `:memory:` database (which stays in
/// `MEMORY` journal mode), so the same options work for tests and production.
pub fn connect_options(database_url: impl Into<String>) -> ConnectOptions {
    let mut options = ConnectOptions::new(database_url);
    options.map_sqlx_sqlite_opts(|opts| {
        opts.journal_mode(SqliteJournalMode::Wal)
            .pragma("cache_size", "-20000")
    });
    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};

    /// Run a one-column `PRAGMA` and return the row, or panic with context.
    async fn pragma_row(db: &DatabaseConnection, pragma: &str) -> sea_orm::QueryResult {
        db.query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("PRAGMA {pragma};"),
        ))
        .await
        .unwrap_or_else(|e| panic!("PRAGMA {pragma} query failed: {e}"))
        .unwrap_or_else(|| panic!("PRAGMA {pragma} returned no row"))
    }

    /// `cache_size` is per-connection and works on any database, including in-memory.
    #[tokio::test]
    async fn applies_cache_size_pragma() {
        let db = Database::connect(connect_options("sqlite::memory:"))
            .await
            .expect("connect in-memory");

        let row = pragma_row(&db, "cache_size").await;
        let cache_size: i64 = row.try_get("", "cache_size").expect("read cache_size");
        assert_eq!(cache_size, -20000, "cache_size pragma should be applied");
    }

    /// WAL is only meaningful for a file-backed database (an in-memory DB stays in
    /// `MEMORY` journal mode), so this connects to a temp file and cleans up after.
    #[tokio::test]
    async fn applies_wal_journal_mode() {
        let path =
            std::env::temp_dir().join(format!("tcglense-wal-test-{}.db", std::process::id()));
        // Clean any leftover from a previous crashed run before asserting.
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }

        let url = format!("sqlite://{}?mode=rwc", path.display());
        let result = async {
            let db = Database::connect(connect_options(&url))
                .await
                .expect("connect file-backed");
            let row = pragma_row(&db, "journal_mode").await;
            row.try_get::<String>("", "journal_mode")
                .expect("read journal_mode")
        }
        .await;

        // Remove the DB file and its WAL sidecars regardless of the assertion outcome.
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }

        assert_eq!(
            result, "wal",
            "journal_mode should be WAL for a file-backed DB"
        );
    }
}

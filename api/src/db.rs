//! Database connection setup.
//!
//! Builds the SeaORM connect options. The backend is selected at runtime by the
//! `DATABASE_URL` scheme (`sqlite://` — the default — or `postgres://`); sea-orm's
//! `Database::connect` dispatches on the scheme, so both drivers are compiled in and
//! no cargo feature gate is involved.
//!
//! - **SQLite** (incl. `sqlite::memory:`): WAL journal mode + a ~20 MB per-connection
//!   page cache + a registered REGEXP function (the Scryfall `/regex/` search filters).
//!   Byte-identical to the pre-Postgres tuning (issue #11); sea-orm force-pins the
//!   SQLite pool to a single connection.
//! - **Postgres**: an explicit connection pool (sizes/timeouts from `DB_*` env vars,
//!   with hard defaults), since sea-orm does not force a default there. The SQLite
//!   pragmas do not apply (and would be a runtime no-op) so they are skipped.

use std::borrow::Cow;
use std::time::Duration;

use sea_orm::{
    ConnectOptions, DatabaseBackend, IdenStatic, Iterable,
    sea_query::{Expr, SimpleExpr},
    sqlx::sqlite::SqliteJournalMode,
};

/// Build [`ConnectOptions`] for `database_url`, branching on the URL scheme.
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
/// The SQLite arm is a no-op or harmless on a `:memory:` database (which stays in
/// `MEMORY` journal mode), so the same options work for tests and production. The
/// Postgres arm leaves the SQLite closure unused, so `map_sqlx_sqlite_opts` never
/// runs on a Postgres connection.
pub fn connect_options(database_url: impl Into<String>) -> ConnectOptions {
    let url: String = database_url.into();
    let mut options = ConnectOptions::new(url.clone());

    if is_postgres_url(&url) {
        apply_postgres_pool(&mut options);
    } else {
        // SQLite (incl. `sqlite::memory:`). Byte-identical to the original tuning:
        // this closure only runs on the SQLite connect path.
        options.map_sqlx_sqlite_opts(|opts| {
            opts.journal_mode(SqliteJournalMode::Wal)
                .pragma("cache_size", "-20000")
                // Register a REGEXP function (sqlx `regexp` feature) on every connection
                // so the Scryfall regex filters (`o:/…/`, `name:/…/`, …) resolve.
                .with_regexp()
        });
    }
    options
}

/// Whether `url` selects the Postgres backend (matches sea-orm's own prefixing).
fn is_postgres_url(url: &str) -> bool {
    url.starts_with("postgres://") || url.starts_with("postgresql://")
}

/// Apply explicit pool sizing/timeouts for the Postgres backend from `DB_*` env vars
/// (hard defaults when unset/blank/unparseable). SQLite is left at sea-orm's forced
/// single-connection default, so this is never called on that arm.
fn apply_postgres_pool(options: &mut ConnectOptions) {
    use crate::config::env_parse;
    options
        .max_connections(env_parse::<u32>("DB_MAX_CONNECTIONS").unwrap_or(10))
        .min_connections(env_parse::<u32>("DB_MIN_CONNECTIONS").unwrap_or(0))
        .connect_timeout(Duration::from_secs(
            env_parse::<u64>("DB_CONNECT_TIMEOUT_SECS").unwrap_or(15),
        ))
        .acquire_timeout(Duration::from_secs(
            env_parse::<u64>("DB_ACQUIRE_TIMEOUT_SECS").unwrap_or(30),
        ));
}

/// Which SQL dialect a compiled fragment targets. `Copy`, so it threads cheaply.
/// Derived once per request from `state.db.get_database_backend()`. Kept here (the DB
/// module already owns connection concerns) so both the search compiler and the shared
/// sort helper can import `crate::db::Dialect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Sqlite,
    Postgres,
}

impl Dialect {
    /// SeaORM only ever hands us Sqlite or Postgres here (MySql is not compiled in);
    /// map anything non-Postgres to the SQLite-shaped fragments.
    pub fn from_backend(backend: DatabaseBackend) -> Self {
        match backend {
            DatabaseBackend::Postgres => Dialect::Postgres,
            _ => Dialect::Sqlite,
        }
    }

    // ---- placeholder normalisation (see §0.2) ----

    /// Rewrite a `?`-placeholder cust template to the backend's placeholder syntax.
    /// SQLite keeps `?`; Postgres gets `$1..$N` left-to-right. `?` inside single-quoted
    /// SQL string literals is left alone. Our templates never contain a literal `$`, so
    /// no `$$` escaping is needed. Apply this to EVERY value-binding cust fragment.
    pub fn placeholders<'a>(self, template: &'a str) -> Cow<'a, str> {
        if self != Dialect::Postgres {
            return Cow::Borrowed(template);
        }
        let mut out = String::with_capacity(template.len() + 8);
        let mut in_str = false;
        let mut n = 0usize;
        for c in template.chars() {
            match c {
                '\'' => {
                    in_str = !in_str;
                    out.push(c);
                } // '' escape: toggles twice, harmless
                '?' if !in_str => {
                    n += 1;
                    out.push('$');
                    out.push_str(&n.to_string());
                }
                _ => out.push(c),
            }
        }
        Cow::Owned(out)
    }

    // ---- SQL-function / operator divergences ----

    /// True iff text `col` holds a plain non-negative integer string (`^[0-9]+$`).
    pub fn integer_string_guard(self, col: &str) -> String {
        match self {
            // Unchanged from today's numeric_guard — identical semantics to `^[0-9]+$`.
            Dialect::Sqlite => {
                format!("{col} IS NOT NULL AND {col} GLOB '[0-9]*' AND {col} NOT GLOB '*[^0-9]*'")
            }
            Dialect::Postgres => format!("{col} IS NOT NULL AND {col} ~ '^[0-9]+$'"),
        }
    }

    /// Guard for CASTing a TEXT price column to REAL. SQLite keeps the historical
    /// null/empty check (its CAST coerces junk to 0.0, and behaviour must not change);
    /// Postgres additionally requires a decimal shape, because its CAST hard-errors on
    /// a non-numeric string and would 500 the whole request.
    pub fn decimal_string_guard(self, col: &str) -> String {
        match self {
            Dialect::Sqlite => format!("{col} IS NOT NULL AND {col} <> ''"),
            Dialect::Postgres => {
                format!("{col} IS NOT NULL AND {col} <> '' AND {col} ~ '^[0-9]+(\\.[0-9]+)?$'")
            }
        }
    }

    /// Case-insensitive regex-match infix operator.
    pub fn regex_operator(self) -> &'static str {
        match self {
            Dialect::Sqlite => "REGEXP",
            Dialect::Postgres => "~*",
        }
    }

    /// The bound pattern for a case-insensitive regex match.
    pub fn regex_pattern(self, pattern: &str) -> String {
        match self {
            Dialect::Sqlite => format!("(?i){pattern}"), // Rust-regex UDF: force CI
            Dialect::Postgres => pattern.to_string(),    // `~*` is already CI
        }
    }

    /// 1-based substring position of `needle_sql` in `hay_sql` (0 when absent).
    /// Args are already-rendered SQL fragments (column names / quoted literals).
    pub fn strpos(self, hay_sql: &str, needle_sql: &str) -> String {
        match self {
            Dialect::Sqlite => format!("INSTR({hay_sql}, {needle_sql})"),
            Dialect::Postgres => format!("STRPOS({hay_sql}, {needle_sql})"),
        }
    }

    /// Expression giving a format's legality status text ('' when absent). Contains a
    /// single `?` for the json key, renumbered by `placeholders`.
    pub fn legality_status_expr(self) -> &'static str {
        match self {
            Dialect::Sqlite => "COALESCE(json_extract(legalities, ?), '')",
            // NULLIF guards NULL/'' legalities so the ::jsonb cast can't error; CAST(? AS
            // text) disambiguates the `jsonb ->> text` operator from `jsonb ->> int`.
            Dialect::Postgres => "COALESCE(NULLIF(legalities, '')::jsonb ->> CAST(? AS text), '')",
        }
    }

    /// The bound json-key value for `legality_status_expr` (SQLite JSONPath vs bare key).
    pub fn legality_key(self, fmt: &str) -> String {
        match self {
            Dialect::Sqlite => format!("$.{fmt}"),
            Dialect::Postgres => fmt.to_string(),
        }
    }
}

/// Build the `WHERE` guard for an `ON CONFLICT … DO UPDATE` action that skips the write
/// when the incoming row is byte-identical to the stored one.
///
/// Emits `("<table>"."c1" IS DISTINCT FROM "excluded"."c1" OR …)` over every column of
/// `C` for which `skip` is false. `IS DISTINCT FROM` is null-safe (a nullable column
/// reads NULL↔NULL as equal, NULL↔value as changed) and is valid on both Postgres and
/// the bundled SQLite (≥ 3.39; we pin 3.46). Feed the result to
/// [`OnConflict::action_and_where`](sea_orm::sea_query::OnConflict::action_and_where): a
/// conflicting row whose compared columns all match is then left untouched — no new
/// tuple, no index maintenance, no WAL — which is the whole point on a re-sync where most
/// rows are unchanged. It carries **no** bound values, so an insert's `$1..$N` numbering
/// is unaffected.
///
/// `skip` must exclude the conflict/identity keys **and** any always-`now()` timestamp
/// (e.g. `updated_at`): comparing an always-fresh timestamp would make every row look
/// changed and defeat the guard. Keep such a timestamp in `update_columns` (so a *real*
/// change still bumps it) but out of this predicate. Building the list from
/// `C::iter()` rather than a hand-written literal means a newly-added column is compared
/// automatically instead of silently dropping out of the guard.
pub(crate) fn upsert_changed_guard<C>(table: &str, skip: impl Fn(&C) -> bool) -> SimpleExpr
where
    C: Iterable + IdenStatic,
{
    let pred = C::iter()
        .filter(|c| !skip(c))
        .map(|c| {
            let col = c.as_str();
            format!(r#"("{table}"."{col}" IS DISTINCT FROM "excluded"."{col}")"#)
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    // No comparable columns (keys/timestamps only) → nothing can differ; always update
    // (degenerate: our callers always have content columns, so this never fires).
    Expr::cust(if pred.is_empty() { "TRUE".to_string() } else { pred })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Statement};

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

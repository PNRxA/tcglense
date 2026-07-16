use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

/// Postgres-only trigram (`pg_trgm`) GIN indexes for the case-insensitive **substring**
/// search filters, surfaced by the same weak-prod index audit as `m..025`/`m..026`.
///
/// The catalog card search compiles `name:`, `t:`/`type:` and `o:`/`oracle:` (and the
/// bare-name search) to `LOWER(COALESCE(<col>, '')) LIKE '%needle%' ESCAPE '\'`
/// (`scryfall::search::compile::text::contains`). That predicate is doubly unindexable
/// by a b-tree: it wraps the column in a function *and* the pattern has a leading `%`.
/// So Postgres sequentially scans **every** card of the game — the `cards` table is very
/// wide (60+ columns) — recomputing `LOWER(COALESCE(...))` per row, then top-N sorts the
/// survivors. On the weak prod instance with a cold cache a single `t:` page was measured
/// at ~1.5 s (`sqlx` slow-statement warning); `o:` over the long `oracle_text` is worse.
///
/// A GIN index with the `pg_trgm` `gin_trgm_ops` operator class makes `LIKE '%needle%'`
/// (needle ≥ 3 non-wildcard chars) an index scan: Postgres breaks the pattern into
/// trigrams, the GIN index returns the candidate rows, and only those are heap-fetched
/// and rechecked. Selective filters (`t:planeswalker`, `o:proliferate`) become a handful
/// of heap fetches. Low-selectivity filters (`t:creature`, ~half the table) and needles
/// shorter than a trigram (`t:el`, 1–2 chars) are deliberately **not** accelerated — the
/// planner keeps the sequential scan, which is the right plan for those, so the index only
/// adds options without forcing a worse plan (measured on Postgres 17: `t:planeswalker`
/// 9.6 ms → 1.1 ms via a bitmap index scan; broad `t:creature` unchanged, still a seq scan).
///
/// The index is on the **exact** compiled expression `lower(coalesce(<col>, ''))` so the
/// planner can match it (an expression index is only used when the query's expression is
/// identical). One consequence: the `/regex/` form of these filters compiles to a
/// *different* expression — `coalesce(<col>, '') ~*` (no `lower()` wrapper; see
/// `compile::text::regex_expr`) — so it is **not** served by these indexes and still
/// scans. Only the `LIKE` substring form is accelerated.
///
/// **Postgres only.** `pg_trgm` is a Postgres extension; SQLite has no equivalent, and
/// the dev/test SQLite DB is tiny (the scan is instant and invisible there), so the
/// SQLite arm is a deliberate no-op — the `LIKE` still runs, byte-identically, via a
/// scan. This mirrors `m..025`'s "invisible on the tiny dev SQLite" framing and the
/// backend-gated raw-SQL idiom of `m..001`.
///
/// Notes / trade-offs (see `docs/tradeoffs.md`):
/// - **`CREATE EXTENSION pg_trgm` needs the connecting role to be allowed to create it.**
///   Migrations run on boot and the app refuses to start if any migration fails, so on a
///   managed Postgres that restricts `CREATE EXTENSION` this line would block the whole
///   API from starting — not just leave search slow. Pre-provision it once as an admin
///   (`CREATE EXTENSION pg_trgm;`) before deploying and the `IF NOT EXISTS` here no-ops.
///   `pg_trgm` ships with the standard `postgres` image (the self-hosted default) and is
///   allow-listed on the mainstream managed providers.
/// - Plain (non-`CONCURRENTLY`) `CREATE INDEX` — `CONCURRENTLY` cannot run inside the
///   migration's transaction. It holds a `SHARE` lock (reads never blocked; writes are)
///   for the whole build, which for a GIN trigram index over the long `oracle_text` on a
///   weak, cold instance is not instant. Card writes happen only during the periodic sync,
///   so the lock is usually uncontended — but a rolling deploy can boot this while the old
///   instance is mid-sync, so the build may wait on (or briefly stall) that sync's writes.
/// - `up()` issues `SET LOCAL statement_timeout = 0` first: sea-orm runs the whole pending
///   batch in **one** transaction, so a server/role-default `statement_timeout` killing a
///   slow index build would roll the *entire* batch back and fail boot. This disables that
///   for the build. It cannot override a *pooler*-level query timeout — run deploy
///   migrations on a direct connection, not through a statement-timeout-capped pooler.
/// - GIN trigram indexes add write cost to the bulk card upsert (each changed row
///   maintains three more indexes); GIN's pending-list (`fastupdate`) absorbs most of it
///   and the sync is a 6 h background job, so read latency wins over the write cost.
/// - The name autocomplete (`name_suggestions_query`) originally compiled a *bare*
///   `LOWER(name)` (the column is `NOT NULL`, no `COALESCE`) — a different expression
///   these indexes do not serve, so every per-keystroke suggestion request seq-scanned
///   the wide `cards` table. It now compiles the indexed `LOWER(COALESCE(name, ''))`
///   verbatim (issue #413), so `idx_cards_name_trgm` serves it. The sealed-`products`
///   name search still compiles the bare form — deliberate: the `products` table is
///   small enough that its scan is cheap.
#[derive(DeriveMigrationName)]
pub struct Migration;

/// The card text columns whose case-insensitive substring search is worth a trigram
/// index, paired with the index name. Each must match the `LOWER(COALESCE(col, ''))`
/// expression that `compile::text::contains` emits, verbatim.
const TRGM_COLUMNS: [(&str, &str); 3] = [
    ("name", "idx_cards_name_trgm"),
    ("type_line", "idx_cards_type_line_trgm"),
    ("oracle_text", "idx_cards_oracle_text_trgm"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite has no pg_trgm; the scan on the tiny dev DB is instant. No-op there.
        if manager.get_database_backend() != DatabaseBackend::Postgres {
            return Ok(());
        }
        let conn = manager.get_connection();
        // The whole pending batch runs in one transaction, so a server/role-default
        // statement_timeout killing a slow GIN build would roll the batch back and fail
        // boot. Disable it for this transaction's builds (SET LOCAL is transaction-scoped).
        conn.execute_unprepared("SET LOCAL statement_timeout = 0")
            .await?;
        conn.execute_unprepared("CREATE EXTENSION IF NOT EXISTS pg_trgm")
            .await?;
        for (col, index) in TRGM_COLUMNS {
            conn.execute_unprepared(&format!(
                "CREATE INDEX IF NOT EXISTS \"{index}\" ON \"cards\" \
                 USING gin (LOWER(COALESCE(\"{col}\", '')) gin_trgm_ops)"
            ))
            .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() != DatabaseBackend::Postgres {
            return Ok(());
        }
        let conn = manager.get_connection();
        // Drop the indexes only; leave `pg_trgm` in place (other objects may rely on it,
        // and dropping an extension is not this migration's to own).
        for (_, index) in TRGM_COLUMNS {
            conn.execute_unprepared(&format!("DROP INDEX IF EXISTS \"{index}\""))
                .await?;
        }
        Ok(())
    }
}

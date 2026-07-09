use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

/// A **covering** index on `card_price_history` for the collection value-over-time query
/// (`handlers::collection::value_history`), surfaced by the same weak-prod index audit as
/// `m..025`–`m..027`.
///
/// That endpoint reconstructs a user's total collection value per day by fetching every
/// snapshot of every owned card — `WHERE game = ? AND card_id IN (…owned…)` (optionally
/// `AND as_of_date >= cutoff`), ordered by `(card_id, as_of_date)`. The existing UNIQUE
/// index `idx_card_price_history_game_card_date (game, card_id, as_of_date)` already serves
/// the filter and the ordering, but it does **not** carry the price columns — so on
/// Postgres every matched index entry needs a **heap fetch** to read `price_usd` /
/// `price_usd_foil`. For a large collection over a wide range that is the whole cost:
/// measured on Postgres 17 against a synthetic 3.65M-row table with a 1500-card
/// collection, the widest window (`all`, ~1.095M rows) did ~1.1M heap fetches (~273 ms
/// warm, and a cold-cache cliff on the weak prod box), while the same query over this
/// covering index is a pure **index-only scan** (0 heap fetches, ~124 ms). The common
/// default `30d` window is cheap either way; this only removes the tail's heap-fetch cost.
///
/// The index duplicates the `(game, card_id, as_of_date)` key of the unique index and adds
/// the two USD price columns. On Postgres they ride as a non-key `INCLUDE` payload (present
/// in the leaf pages for index-only scans, but not part of the ordered key). SQLite has no
/// `INCLUDE`, so there the price columns are appended to the key instead — the leading
/// three columns keep the same seek/scan behaviour and the trailing two make the read
/// covering all the same. The unique index is left in place: it enforces the daily
/// snapshot's `ON CONFLICT (game, card_id, as_of_date)` upsert target, which this
/// non-unique index must not.
///
/// Trade-off (see `docs/tradeoffs.md`): `card_price_history` grows by one row per card per
/// day and is never pruned, so this is a second large, unbounded-growth index — extra disk
/// and a little write amplification on the periodic snapshot batch. It's accepted because
/// the read it accelerates is a heap-fetch-heavy scan on a weak, cold instance, and the
/// snapshot is a background job where read latency wins.
///
/// Notes:
/// - Plain (non-`CONCURRENTLY`) `CREATE INDEX`: `CONCURRENTLY` cannot run inside the
///   migration's transaction (mirrors `m..027`). Card-price writes happen only during the
///   periodic sync, so the build's `SHARE` lock is usually uncontended.
/// - `up()` issues `SET LOCAL statement_timeout = 0` on Postgres first: the whole pending
///   batch runs in one transaction, so a server/role-default `statement_timeout` killing a
///   slow build over the large table would roll the entire batch back and fail boot.
const INDEX_NAME: &str = "idx_card_price_history_covering";
const TABLE: &str = "card_price_history";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        match manager.get_database_backend() {
            DatabaseBackend::Postgres => {
                // One transaction for the whole batch, so a slow build must not hit a
                // server/role-default statement_timeout and roll everything back.
                conn.execute_unprepared("SET LOCAL statement_timeout = 0")
                    .await?;
                conn.execute_unprepared(&format!(
                    "CREATE INDEX IF NOT EXISTS \"{INDEX_NAME}\" ON \"{TABLE}\" \
                     (\"game\", \"card_id\", \"as_of_date\") \
                     INCLUDE (\"price_usd\", \"price_usd_foil\")"
                ))
                .await?;
            }
            // SQLite has no INCLUDE, so the price columns go in the key: the leading three
            // still seek/scan as before and the trailing two make the read covering. The
            // dev/test SQLite DB is tiny, so this is essentially free there.
            _ => {
                conn.execute_unprepared(&format!(
                    "CREATE INDEX IF NOT EXISTS \"{INDEX_NAME}\" ON \"{TABLE}\" \
                     (\"game\", \"card_id\", \"as_of_date\", \"price_usd\", \"price_usd_foil\")"
                ))
                .await?;
            }
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(&format!("DROP INDEX IF EXISTS \"{INDEX_NAME}\""))
            .await?;
        Ok(())
    }
}

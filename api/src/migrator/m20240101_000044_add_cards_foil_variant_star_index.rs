use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;

/// A tiny **partial** index over the foil-★ variant cards, to drive the per-sync-tick foil-price
/// enrichment (`scryfall::foil_variants::enrich_foil_variant_prices`), surfaced by a weak-prod
/// slow-query log.
///
/// Some sets (Secret Lair especially) model a card's foil printing as a *separate* Scryfall
/// object whose collector number is the nonfoil's plus a star — `sld` `741` (nonfoil) and `741★`
/// (foil). The enrichment copies each such star's `price_usd_foil` onto its nonfoil base so a
/// folded foil holding values correctly. In the real catalog there are ~1,851 of these foil-only
/// `…★` stars against ~40k nonfoil bases and ~106k cards total.
///
/// The enrichment `UPDATE` is **star-driven** — it starts from the tiny star set and joins each
/// back to its base — so it needs to *find* the stars cheaply. The distinguishing predicate is
/// `finishes = 'foil' AND collector_number LIKE '%★'`, but the trailing-`★` `LIKE` is unindexable
/// by a b-tree (`m..026`'s note) and `finishes = 'foil'` alone still matches ~12k foil-only rows
/// whose `collector_number` is not in any existing index — so without help the star scan reads
/// every foil row and heap-fetches each to test the `LIKE`. On the weak, cold prod box the whole
/// enrichment measured at ~8.2 s (a slow-query WARN at elapsed=8.24 s).
///
/// A **partial** index whose `WHERE` is exactly the star predicate stores only those ~1,851 rows
/// and carries the join columns, so the query provably implies the index predicate and the
/// planner serves the star scan as a bounded index scan of ~1,851 entries — then point-seeks each
/// base through the existing `idx_cards_game_set_code_collector_number`. Measured on the
/// 106,520-row Postgres reproduction: the enrichment's shared-buffer touch drops from **34,805**
/// (two full wide-heap scans + a correlated guard) to **~9,000** (~3.9×), off a **96 KB** index.
/// Crucially this is **robust to visibility-map state** — the star scan is bounded by the tiny
/// star count and the base lookups are point seeks that touch the heap anyway, so nothing here
/// depends on an index-only scan the way a full-table covering index would. That is why this is a
/// safe win on the never-`VACUUM`ed `cards` table (same "tiny → robust" reasoning as `m..034`),
/// where a covering index for the sibling snapshot read was rejected — see `docs/tradeoffs.md`
/// §Price history.
///
/// **Lock-step:** the `WHERE` here is byte-identical to the `star.finishes = 'foil' AND
/// star.collector_number LIKE '%★'` predicate in `enrich_foil_variant_prices`'s `ENRICH_SQL`. If
/// they drift, the planner silently falls back to a full foil scan (the enrichment's own tests
/// still pass on correctness — only the plan degrades).
///
/// Like `m..034`, sea-query has no partial-`WHERE` builder, so this goes through raw
/// `execute_unprepared`; and no `db::Dialect` gate is needed — a partial index and every arm
/// (`=`, `LIKE`, the `★` literal) render identically on SQLite and Postgres. Plain (non-
/// `CONCURRENTLY`) `CREATE INDEX` takes a brief `SHARE` lock on `cards`; card writes happen only
/// during the periodic sync, so it is normally uncontended (same caveat as `m..027`/`m..034`).
/// The index is minuscule, so no `statement_timeout` guard is needed.
const INDEX_NAME: &str = "idx_cards_foil_variant_star";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(&format!(
                "CREATE INDEX IF NOT EXISTS \"{INDEX_NAME}\" ON \"cards\" \
                 (\"game\", \"set_code\", \"oracle_id\", \"collector_number\") \
                 WHERE \"finishes\" = 'foil' AND \"collector_number\" LIKE '%★'"
            ))
            .await?;
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

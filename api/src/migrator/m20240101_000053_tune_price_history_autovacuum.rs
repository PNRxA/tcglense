use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

/// Per-table **autovacuum thresholds** on the two price-history tables (Postgres only),
/// the standing backstop behind the per-tick `catalog::maintain_price_history`
/// `VACUUM (ANALYZE)`.
///
/// The daily capture inserts one row per priced entity into `card_price_history` /
/// `product_price_history` — tables that grow unbounded and are never pruned. Postgres's
/// *default* autovacuum triggers are **scale-factor** based
/// (`autovacuum_analyze_scale_factor` 0.1, `autovacuum_vacuum_insert_scale_factor` 0.2),
/// so on a table of N rows they only fire after ~0.1–0.2·N changes. On a multi-million-row
/// history table growing by a few tens/hundreds of thousands of rows a day that is
/// **months** between autovacuums — and until one runs, two things bite the collection
/// analytics reads (value-history / movers / the per-card chart):
///
/// * **Stale planner stats** make the planner mis-cost `{card,product}_id IN (…owned…)` and
///   demote it from a per-entity index seek to an in-memory *filter* that scans the whole
///   game's date window. Measured on a faithful 18M-row Postgres 16 repro (956-card
///   collection, 30-day window): the value-history fetch scanned 620k index entries to
///   return 30k rows — **6.7 s**, versus **0.16 s** once `ANALYZE` had run.
/// * A **stale visibility map** turns the covering index's index-only scan into per-row
///   heap-visibility fetches (the churned-VM cliff `m…031` documents): the same repro's
///   30-day read paid ~30k heap fetches until a `VACUUM` reset the all-visible bits.
///
/// The per-tick explicit `VACUUM (ANALYZE)` is the primary fix (deterministic, right after
/// the only writer, before user traffic). This migration is the **backstop**: switch these
/// two tables to *absolute* thresholds (scale-factor 0) so autovacuum still keeps their
/// stats and visibility map fresh even if the explicit pass is disabled
/// (`SYNC_INTERVAL_HOURS=0` with no tick) or fails. Storage parameters are cheap metadata
/// (a brief `SHARE UPDATE EXCLUSIVE` lock, no table rewrite) and `as_of_date` grows
/// monotonically so each autovacuum scans only the freshly-appended tail.
///
/// SQLite has no autovacuum-worker / visibility-map machinery and its planner stats are a
/// deliberate non-goal (the schema never runs `ANALYZE` there — see
/// `handlers::catalog::products`), so this is a Postgres-only no-op on that backend.
const CARD_TABLE: &str = "card_price_history";
const PRODUCT_TABLE: &str = "product_price_history";

/// Absolute row-change thresholds (scale-factor pinned to 0 so table size is irrelevant).
/// The card table takes on ~100k rows/capture, so a 50k threshold fires it about once per
/// capture; the far smaller product table uses a lower 5k threshold so it still fires.
const CARD_THRESHOLD: i64 = 50_000;
const PRODUCT_THRESHOLD: i64 = 5_000;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() != DatabaseBackend::Postgres {
            return Ok(());
        }
        let conn = manager.get_connection();
        for (table, threshold) in [
            (CARD_TABLE, CARD_THRESHOLD),
            (PRODUCT_TABLE, PRODUCT_THRESHOLD),
        ] {
            conn.execute_unprepared(&format!(
                "ALTER TABLE \"{table}\" SET (\
                 autovacuum_vacuum_scale_factor = 0, \
                 autovacuum_vacuum_threshold = {threshold}, \
                 autovacuum_analyze_scale_factor = 0, \
                 autovacuum_analyze_threshold = {threshold}, \
                 autovacuum_vacuum_insert_scale_factor = 0, \
                 autovacuum_vacuum_insert_threshold = {threshold})"
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
        for table in [CARD_TABLE, PRODUCT_TABLE] {
            conn.execute_unprepared(&format!(
                "ALTER TABLE \"{table}\" RESET (\
                 autovacuum_vacuum_scale_factor, \
                 autovacuum_vacuum_threshold, \
                 autovacuum_analyze_scale_factor, \
                 autovacuum_analyze_threshold, \
                 autovacuum_vacuum_insert_scale_factor, \
                 autovacuum_vacuum_insert_threshold)"
            ))
            .await?;
        }
        Ok(())
    }
}

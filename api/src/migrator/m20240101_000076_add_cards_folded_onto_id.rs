use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

/// `cards.folded_onto_id` — which base card a foil-★ variant has been folded onto for
/// display, plus the partial index its non-`NULL` side is probed through.
///
/// Scryfall models some printings' foil as a *separate* card object one star along
/// (`sld` `1587` / `1587★`), and `scryfall::enrich_foil_variant_prices` (#209) copies the
/// star's foil price onto the base — so on the surfaces that *list* cards the star is a
/// second, near-identical tile for one card. Which stars are safe to fold that way is a
/// question about the two rows' **printed attributes**, not just their finishes (a `9ed`
/// foil is black-bordered where its nonfoil is white — a genuinely different card), and it
/// is far too expensive to re-derive per row inside a listing query. So
/// `scryfall::foil_variants::refresh_foil_variant_folds` decides it once per sync tick and
/// records the answer here: non-`NULL` = "this row is a folded duplicate of card N".
///
/// Nullable and orphan-tolerant by construction — it is a *derived display* pointer, not a
/// foreign key. No FK constraint on purpose (the call `price_alerts.card_id` and the life
/// counter's `commander_card_id` make): a re-import that removes a base row must leave the
/// star listable again, which the refresh pass does on its next run, not fail a delete.
///
/// **Two upsert couplings, both load-bearing** (`scryfall::ingest::flush_cards`):
/// the card upsert builds its `update_columns` from `card::Column::iter()` minus a
/// deny-list, so without an entry there every sync would set this column from the
/// *incoming* row — i.e. wipe it — and it builds `upsert_changed_guard` the same way, so
/// without a second entry every folded row would compare as "changed" on every tick,
/// defeating the skip-unchanged guard and mass-bumping `updated_at` (the cursor the
/// price-alert evaluator's change-narrowing keys on). Both entries ship with this column.
///
/// **No backfill here.** The refresh pass runs on every sync tick and once at boot on the
/// no-sync path (`tasks::spawn_foil_price_enrichment`), so the column populates itself
/// within a tick of deploy. Leaving it `NULL` until then is the safe direction: a `NULL`
/// folds nothing, so the listings simply behave as they did before this migration rather
/// than hiding a row the pass hasn't validated yet.
///
/// **The index is `PARTIAL`, on `folded_onto_id IS NOT NULL`.** Every consumer probes the
/// non-`NULL` side: `has_folded_foil_variant`'s semi-join (`fv.folded_onto_id IS NOT NULL
/// AND fv.folded_onto_id = cards.id` — the `IS NOT NULL` is spelled out there precisely so
/// the query predicate provably implies this one), `foil_variants::stale_clear_chunks`, and
/// `folded_counts`. The listings' own `folded_onto_id IS NULL` matches ~99.5% of rows and
/// must **never** ride an index; a partial index on the opposite predicate cannot serve it,
/// which is half the point. The other half is size: a plain b-tree here would carry all
/// ~106k rows, because **both** backends index `NULL`s (omitting them is Oracle's
/// behaviour, not Postgres' or SQLite's) — 105k dead entries, and a cheap-looking access
/// path the planner can cost for a column nothing wants a `NULL` from. Partial, it holds
/// the ~500 folded rows: a few pages, and the same "tiny → robust regardless of
/// visibility-map state" reasoning as `m..034`/`m..044` on the never-`VACUUM`ed `cards`.
///
/// Like `m..034`/`m..044`, sea-query's `IndexCreateStatement` has no partial-`WHERE`
/// builder, so the create goes through raw `execute_unprepared` — and no `db::Dialect` gate
/// is needed: partial indexes (SQLite ≥ 3.8.0) and `IS NOT NULL` render identically, so the
/// statement is byte-identical on both backends.
const INDEX: &str = "idx_cards_folded_onto_id";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Postgres runs the whole pending batch in one transaction, so `SET LOCAL` holds
        // for the rest of this migration — the index build *and* the `ANALYZE` below. A
        // server/role-default `statement_timeout` killing either would roll the batch back
        // and fail startup. Same guard as `m..068`/`m..066`/`m..050`/`m..031`.
        let postgres = manager.get_database_backend() == DatabaseBackend::Postgres;
        if postgres {
            manager
                .get_connection()
                .execute_unprepared("SET LOCAL statement_timeout = 0")
                .await?;
        }

        manager
            .alter_table(
                Table::alter()
                    .table(Cards::Table)
                    .add_column_if_not_exists(ColumnDef::new(Cards::FoldedOntoId).integer().null())
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(&format!(
                "CREATE INDEX IF NOT EXISTS \"{INDEX}\" ON \"cards\" (\"folded_onto_id\") \
                 WHERE \"folded_onto_id\" IS NOT NULL"
            ))
            .await?;

        if postgres {
            // A freshly added column has **no** `pg_statistic` row, so until autoanalyze
            // fires Postgres falls back to `DEFAULT_UNK_SEL`: it estimates the listings'
            // `folded_onto_id IS NULL` at 0.5% selective when it is really ~99.5% — i.e. it
            // believes the one predicate every catalog grid carries is a near-perfect
            // filter. And autoanalyze may not fire for a long while: the fold pass touches
            // only ~500 of ~106k rows, far under the 10% analyze threshold, and a
            // version-gated sync writes nothing at all on an unchanged tick. One `ANALYZE`
            // makes the estimate right from boot. It is legal inside the batch transaction
            // (unlike `VACUUM`) and only reads the table.
            manager
                .get_connection()
                .execute_unprepared("ANALYZE \"cards\"")
                .await?;
            // Deliberately not mirrored on SQLite: it consults statistics only where a
            // `sqlite_stat1` row exists, and nothing in this schema ever runs `ANALYZE`
            // (`m..068` leans on exactly that — every index gets the same estimate there),
            // so seeding stats for `cards` alone would change one table's planning
            // asymmetrically. On the empty database this migration usually meets it would
            // record nothing useful anyway.
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(&format!("DROP INDEX IF EXISTS \"{INDEX}\""))
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Cards::Table)
                    .drop_column(Cards::FoldedOntoId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    FoldedOntoId,
}

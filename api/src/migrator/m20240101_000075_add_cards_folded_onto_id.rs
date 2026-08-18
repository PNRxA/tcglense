use sea_orm::DatabaseBackend;
use sea_orm_migration::prelude::*;

/// `cards.folded_onto_id` — which base card a foil-★ variant has been folded onto for
/// display, plus the index the catalog listings filter on.
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
const INDEX: &str = "idx_cards_folded_onto_id";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() == DatabaseBackend::Postgres {
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

        // The listings filter `folded_onto_id IS NULL`, which is ~98% of rows and needs no
        // index. This one serves the *other* direction — `is:foil` and the foil-treatment
        // `is:` leaves ask "does a folded star hang off this base", a semi-join whose probed
        // side is exactly this column. Tiny: only the ~550 folded rows are non-NULL, and
        // Postgres omits NULLs from a partial index, so it stays a few pages either way.
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name(INDEX)
                    .table(Cards::Table)
                    .col(Cards::FoldedOntoId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(Index::drop().name(INDEX).table(Cards::Table).to_owned())
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

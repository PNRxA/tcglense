use sea_orm_migration::prelude::*;

/// Adds the composite `cards (game, set_code, collector_number)` index and drops the now-redundant
/// `idx_cards_game_set_code`.
///
/// A collection import resolves catalog cards by a row-value `(set_code, collector_number) IN
/// (…up to 450 tuples…)` filtered by `game` — `collection_import::mod`'s Moxfield/CSV resolve and
/// `collection_import::consolidate::load_foil_variant_pairs`. With only `(game, set_code)` to lean
/// on, Postgres bitmap-ORs one index scan per *distinct set code*: a real multi-set import names
/// hundreds of sets, matches ~every card of each (tens of thousands of rows), then filters — or the
/// planner tips to a full sequential scan of the wide `cards` table. On the weak production instance
/// with a cold cache that is seconds per chunk (a 450-tuple resolve measured at ~0.6–6 s). The
/// composite turns each tuple into a direct index seek (≤1 row per pair).
///
/// Its `(game, set_code)` prefix fully subsumes the dropped index — the set-browse listings
/// (`handlers::catalog::sets`), the search `e:`/`st:` filter, and the mtgjson `set_code IN` resolve
/// all use exactly that prefix — so the drop is non-regressing, and no `ON CONFLICT` depends on it
/// (the card upsert conflict-targets `(game, external_id)`). `enrich_foil_variant_prices` (the
/// per-sync-tick foil-★ self-join) also becomes a seek.
///
/// Non-unique on purpose: `(set_code, collector_number)` is unique in practice (the foil-★ variant
/// carries a distinct `…★` number) but is not DB-enforced elsewhere, and enforcing it here would only
/// add an upsert failure mode for no query benefit. It carries the **text** `collector_number` (not
/// `collector_number_int`) because the tuple compares the raw text column.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create the wider index first so there is never a window without a `(game, set_code)` index.
        manager
            .create_index(
                Index::create()
                    .name("idx_cards_game_set_code_collector_number")
                    .table(Cards::Table)
                    .col(Cards::Game)
                    .col(Cards::SetCode)
                    .col(Cards::CollectorNumber)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_cards_game_set_code")
                    .table(Cards::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx_cards_game_set_code")
                    .table(Cards::Table)
                    .col(Cards::Game)
                    .col(Cards::SetCode)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_cards_game_set_code_collector_number")
                    .table(Cards::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    Game,
    SetCode,
    CollectorNumber,
}

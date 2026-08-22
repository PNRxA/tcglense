use sea_orm_migration::prelude::*;

/// `precon_decks.price_cents` — the estimated USD value of the deck proper (commander +
/// mainboard, never the sideboard), in integer cents, so the precon browse can show and
/// **sort by** what a decklist is worth without paying a per-row card scan on a public,
/// CDN-cacheable read.
///
/// Derived, never ingested: card prices move every sync tick while the precon tables are
/// rebuilt only when MTGJSON's document changes, so a value folded once at rebuild would go
/// stale between rebuilds. `catalog::precon_values::refresh_precon_values` recomputes it
/// from the live card prices on every sync tick (after the sealed-contents sync, so a fresh
/// rebuild's rows are priced in the same pass) and once at boot on the no-sync path
/// (`tasks::spawn_derived_price_passes`) — the same decided-once-per-tick stance as
/// `cards.folded_onto_id` (`m..076`). The rebuild itself writes `NULL`, which is also why
/// **no backfill ships here**: the column populates itself within a tick of deploy, and an
/// unpriced deck is exactly what `NULL` already means on the wire (sorted last, shown as no
/// price — never `$0.00`).
///
/// Stored in integer cents (not a decimal string like the provider-fed `products.price_usd`)
/// because this is our own derived datum: the shared `Valuation` fold that computes it works
/// in cents natively, and a plain integer column sorts with `ORDER BY` + `NULLS LAST` on
/// both backends with none of the dialect-guarded `CAST` machinery the product price sort
/// needs for its strings.
///
/// No index: the browse filters to one game's ~3k header rows before it sorts, the same
/// scale every existing precon ordering (`released_at`, `name`) already walks unindexed.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(PreconDecks::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(PreconDecks::PriceCents).big_integer().null(),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(PreconDecks::Table)
                    .drop_column(PreconDecks::PriceCents)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum PreconDecks {
    Table,
    PriceCents,
}

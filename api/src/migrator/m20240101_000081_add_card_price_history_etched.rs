use sea_orm_migration::prelude::*;

/// `card_price_history.price_usd_etched` — the daily capture of a card's **etched-foil**
/// price (issue #676), beside the regular and foil columns the table has carried since
/// `m..008`.
///
/// `cards.price_usd_etched` has been ingested by the Scryfall map all along, but the history
/// table had no column for it, so the third finish some sets ship (Commander Legends, LOTR,
/// Double Masters) could be searched (`is:etched`) and alerted on, yet never charted. The
/// snapshot writer (`scryfall::price_history::snapshot_prices`) fills it from the next tick
/// on; rows captured before this column stay `NULL`, which the price chart gaps over rather
/// than drawing as zero — the same stance every other nullable price column takes.
///
/// **No backfill**: the daily snapshot is a copy of the live column on that day, and the
/// etched price a card had on a past day is not recoverable from anything we store. USD only
/// — Scryfall publishes no `eur_etched`, so no sibling column is invented.
///
/// No index: the series read (`GET .../cards/{id}/prices`) seeks one card's rows through the
/// existing unique `(game, card_id, as_of_date)` index and reads every price column off the
/// row; the covering index of `m..031` serves the collection value query, which sums regular
/// and foil holdings only (an etched copy is held as foil, issue #594) and so never reads
/// this column.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(CardPriceHistory::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(CardPriceHistory::PriceUsdEtched)
                            .string()
                            .null(),
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
                    .table(CardPriceHistory::Table)
                    .drop_column(CardPriceHistory::PriceUsdEtched)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CardPriceHistory {
    Table,
    PriceUsdEtched,
}

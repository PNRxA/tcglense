use sea_orm_migration::prelude::*;

/// Adds the remaining Scryfall external ids to `cards` (issue #686): the Gatherer
/// `multiverse_ids` (comma-joined, one per face), the Magic Online `mtgo_id` /
/// `mtgo_foil_id`, the `arena_id`, and the Cardmarket `cardmarket_id`. All nullable
/// (default NULL) so the ADD COLUMNs are valid on SQLite and the next card sync backfills
/// them from Scryfall — they are provider data, so `ingest::flush_cards`' allow-by-default
/// column lists pick them up without a change. That first sync after the migration
/// rewrites every row (and its `updated_at`, the cursor the price-alert narrowing reads)
/// once; a known, one-off full pass.
///
/// They give the Archidekt export the `Multiverse Id` / `MTGO ID` values it used to
/// write as `0`, and the card page a deep link by id (TCGplayer, Cardmarket, Gatherer)
/// instead of a name search.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite allows only one ALTER option per statement, so add each column in its
        // own `ALTER TABLE`.
        for column in [
            ColumnDef::new(Cards::MultiverseIds)
                .text()
                .null()
                .to_owned(),
            ColumnDef::new(Cards::MtgoId).integer().null().to_owned(),
            ColumnDef::new(Cards::MtgoFoilId)
                .integer()
                .null()
                .to_owned(),
            ColumnDef::new(Cards::ArenaId).integer().null().to_owned(),
            ColumnDef::new(Cards::CardmarketId)
                .integer()
                .null()
                .to_owned(),
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Cards::Table)
                        .add_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for column in [
            Cards::MultiverseIds,
            Cards::MtgoId,
            Cards::MtgoFoilId,
            Cards::ArenaId,
            Cards::CardmarketId,
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Cards::Table)
                        .drop_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    MultiverseIds,
    MtgoId,
    MtgoFoilId,
    ArenaId,
    CardmarketId,
}

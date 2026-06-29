use sea_orm_migration::prelude::*;

/// Adds the gameplay-text and creature-stat columns that the Scryfall search
/// syntax filters on (`o:`, `pow`, `tou`, `loy`). All nullable (default NULL) so
/// the ADD COLUMNs are valid on SQLite and the next card import backfills them.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite allows only one ALTER option per statement, so add each column
        // in its own `ALTER TABLE`.
        for column in [
            ColumnDef::new(Cards::OracleText).text().null().to_owned(),
            ColumnDef::new(Cards::Power).string().null().to_owned(),
            ColumnDef::new(Cards::Toughness).string().null().to_owned(),
            ColumnDef::new(Cards::Loyalty).string().null().to_owned(),
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
            Cards::OracleText,
            Cards::Power,
            Cards::Toughness,
            Cards::Loyalty,
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
    OracleText,
    Power,
    Toughness,
    Loyalty,
}

use sea_orm::DbErr;
use sea_orm_migration::prelude::*;

/// Adds `cards.token_parts` — the JSON list of tokens and emblems a printing makes, folded
/// out of Scryfall's `all_parts` at ingest (`crate::scryfall::map::token_parts`) so a deck
/// page can answer "what do I need to bring besides the cards".
///
/// A **column on `cards`**, not a mapping table, for the same reason `card_faces` and
/// `legalities` are columns: it is a per-printing attribute the provider ships inside the
/// card object, it is read only alongside the row itself, and the catalog upsert's changed
/// guard then keeps it correct with no second write path and no rows to garbage-collect.
///
/// **Nullable, and the NULL means something.** Existing rows keep NULL until the next bulk
/// import rewrites them (Scryfall publishes `default_cards` daily, and the import is
/// version-gated on that file's `updated_at`, so it self-heals within a day rather than on
/// deploy). The read words a NULL as "not checked yet" and an empty array as "makes none" —
/// a backfill that guessed `[]` here would make every deck report, with confidence, that it
/// makes no tokens.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Cards::Table)
                    .add_column(ColumnDef::new(Cards::TokenParts).text().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Cards::Table)
                    .drop_column(Cards::TokenParts)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    TokenParts,
}

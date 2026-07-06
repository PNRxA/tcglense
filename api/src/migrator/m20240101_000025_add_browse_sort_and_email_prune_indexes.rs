use sea_orm_migration::prelude::*;

/// Two indexes surfaced by a Postgres index audit of every query pattern (weak prod instance, cold
/// cache), where a full-partition scan/sort that is invisible on the tiny dev SQLite becomes real
/// request latency.
///
/// - `cards (game, set_code, collector_number_int)`: the default set-browse read
///   (`handlers::catalog::sets::list_set_cards`, `SortField::Number`) filters `(game, set_code)`
///   then orders by `collector_number_int`. The `(game, set_code, collector_number)` composite
///   (`m..024`) seeks the set but carries the **text** number, so it cannot supply the **numeric**
///   sort order — Postgres heap-fetches every (wide) row of the set and sorts, even for page 1, and
///   spills to an on-disk sort on deep pages. This index lets it walk the set pre-ordered and stop
///   at the page (measured ~6 ms → ~0.07 ms for a page-1 read of a 16k-card set; a deep page drops
///   an 18 MB on-disk sort). It is a separate index from `m..024` because the tuple-IN resolve
///   needs the text column while this sort needs the int column.
/// - `email_tokens (expires_at)`: `auth::email_token::prune_expired` (the 6h maintenance loop)
///   deletes `WHERE expires_at <= now` and otherwise full-scans the table. This is the twin of the
///   `refresh_tokens (expires_at)` prune index added in `m..022` — the email-token table was
///   overlooked there. `expires_at` is set once at insert and never updated, so the index is
///   insert-only.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx_cards_game_set_code_collector_number_int")
                    .table(Cards::Table)
                    .col(Cards::Game)
                    .col(Cards::SetCode)
                    .col(Cards::CollectorNumberInt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_email_tokens_expires_at")
                    .table(EmailTokens::Table)
                    .col(EmailTokens::ExpiresAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_email_tokens_expires_at")
                    .table(EmailTokens::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_cards_game_set_code_collector_number_int")
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
    CollectorNumberInt,
}

#[derive(DeriveIden)]
enum EmailTokens {
    Table,
    ExpiresAt,
}

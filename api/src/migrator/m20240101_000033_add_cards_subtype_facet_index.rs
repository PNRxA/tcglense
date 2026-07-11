use sea_orm_migration::prelude::*;

/// Adds a covering composite index `(game, set_code, frame_effects, border_color, full_art)`
/// on `cards` for the by-treatment set gate.
///
/// `scryfall::subtypes::sets_with_subtypes` (fills the `has_subtypes` flag on every tile of the
/// CDN-cached set list) runs `SELECT DISTINCT set_code FROM cards WHERE game = ? AND (...)` where
/// the `(...)` is `has_subtype_condition`: a leading-wildcard `frame_effects LIKE '%,showcase,%'`
/// (unindexable as a seek) OR'd with `border_color`/`full_art` checks. The predicate can only be
/// evaluated by a scan, and without a covering index Postgres heap-fetches every (60-column-wide)
/// row of the game partition just to read the four predicate columns and the DISTINCT target —
/// measured at ~1.9 s per request on the weak prod instance.
///
/// This index carries exactly the columns the query touches (`game` seeks the partition,
/// `set_code` is the DISTINCT target, `frame_effects`/`border_color`/`full_art` supply the
/// predicate), so the planner answers the whole query from an **index-only scan** — no heap
/// access — and, because the index is ordered by `(game, set_code)`, streams the DISTINCT without
/// a sort. Same covering-index tradeoff as `m..025`/`m..031`.
///
/// Pure query-builder, so it renders identically on SQLite and Postgres (no `db::Dialect`).
/// `frame_effects`/`border_color` are nullable — a b-tree indexes NULL entries fine.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_cards_game_subtype_facet")
                    .table(Cards::Table)
                    .col(Cards::Game)
                    .col(Cards::SetCode)
                    .col(Cards::FrameEffects)
                    .col(Cards::BorderColor)
                    .col(Cards::FullArt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_cards_game_subtype_facet")
                    .table(Cards::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Cards {
    Table,
    Game,
    SetCode,
    FrameEffects,
    BorderColor,
    FullArt,
}

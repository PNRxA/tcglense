use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // The card-detail page's second read shape: "every tag on THIS artwork"
        // (`/api/games/{game}/cards/{id}/art-tags`). `m..063`'s index leads with
        // `tag_slug` — perfect for the search's `art:` probe, useless here, so this
        // lookup would otherwise seq-scan the ~1M-row mapping table on every card view.
        // Not unique: one artwork legitimately carries many tags (that's the point);
        // `(game, tag_slug, illustration_id)` still enforces pair uniqueness.
        manager
            .create_index(
                Index::create()
                    .name("idx_card_art_tags_game_illustration")
                    .table(CardArtTags::Table)
                    .col(CardArtTags::Game)
                    .col(CardArtTags::IllustrationId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_card_art_tags_game_illustration")
                    .table(CardArtTags::Table)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum CardArtTags {
    Table,
    Game,
    IllustrationId,
}

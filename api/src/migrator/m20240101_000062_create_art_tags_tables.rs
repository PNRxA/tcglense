use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Tag metadata: one row per Tagger art tag (issue #140). Powers the art-tag
        // autocomplete; the search itself hits `card_art_tags` below.
        manager
            .create_table(
                Table::create()
                    .table(ArtTags::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ArtTags::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ArtTags::Game).string().not_null())
                    // The tag's stable Tagger UUID — slugs may drift as the community
                    // renames tags; this is the durable identity across refreshes.
                    .col(ColumnDef::new(ArtTags::ScryfallId).string().not_null())
                    .col(ColumnDef::new(ArtTags::Slug).string().not_null())
                    .col(ColumnDef::new(ArtTags::Label).string().not_null())
                    // Community descriptions can be long, so `text` rather than VARCHAR.
                    .col(ColumnDef::new(ArtTags::Description).text())
                    .col(ColumnDef::new(ArtTags::TaggingsCount).integer().not_null())
                    .col(
                        ColumnDef::new(ArtTags::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // The one read shape: autocomplete lookup by `(game, slug)`. Unique as defence
        // in depth — the ingest already drops duplicate slugs while parsing, so this
        // only trips (failing the swap transaction and keeping the previous good tag
        // set) if a dupe slips in through some other path.
        manager
            .create_index(
                Index::create()
                    .name("idx_art_tags_game_slug")
                    .table(ArtTags::Table)
                    .col(ArtTags::Game)
                    .col(ArtTags::Slug)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Tag → artwork mapping, hierarchy-expanded at ingest (an artwork tagged
        // `squirrel` also gets rows for `rodent`, `animal`, …) so `art:` compiles to a
        // single indexed EXISTS probe. `id` is big_integer: the daily wholesale rebuild
        // re-inserts ~1M rows per refresh, which would exhaust a 32-bit Postgres
        // sequence within a few years.
        manager
            .create_table(
                Table::create()
                    .table(CardArtTags::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CardArtTags::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(CardArtTags::Game).string().not_null())
                    // Denormalized from `art_tags.slug` so the search EXISTS needs no
                    // join; the wholesale rebuild self-corrects slug drift.
                    .col(ColumnDef::new(CardArtTags::TagSlug).string().not_null())
                    // Joins to `cards.illustration_id`. Not a foreign key — like
                    // `card_rulings`, the table is separately refreshed and a card row
                    // may be absent momentarily during a card re-import.
                    .col(
                        ColumnDef::new(CardArtTags::IllustrationId)
                            .string()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // The search's probe shape: `EXISTS (… WHERE game = ? AND tag_slug = ? AND
        // illustration_id = cards.illustration_id)` — a point lookup on this index.
        // Unique doubles as a defence against double-inserted pairs.
        manager
            .create_index(
                Index::create()
                    .name("idx_card_art_tags_game_slug_illustration")
                    .table(CardArtTags::Table)
                    .col(CardArtTags::Game)
                    .col(CardArtTags::TagSlug)
                    .col(CardArtTags::IllustrationId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CardArtTags::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ArtTags::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum ArtTags {
    Table,
    Id,
    Game,
    ScryfallId,
    Slug,
    Label,
    Description,
    TaggingsCount,
    CreatedAt,
}

#[derive(DeriveIden)]
enum CardArtTags {
    Table,
    Id,
    Game,
    TagSlug,
    IllustrationId,
}

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Decks::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Decks::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Decks::UserId).integer().not_null())
                    .col(ColumnDef::new(Decks::Game).string().not_null())
                    // Nullable: a deck not filed under a folder. `SET NULL` on folder
                    // delete ungroups the deck rather than deleting it.
                    .col(ColumnDef::new(Decks::FolderId).integer().null())
                    .col(ColumnDef::new(Decks::Name).string().not_null())
                    .col(ColumnDef::new(Decks::Description).string().null())
                    .col(ColumnDef::new(Decks::Format).string().null())
                    .col(
                        ColumnDef::new(Decks::IsPublic)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Decks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Decks::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Deleting a user removes their decks (and, via cascade, sections + cards).
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_decks_user_id")
                            .from(Decks::Table, Decks::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    // Deleting a folder ungroups its decks (folder_id -> NULL), never deletes them.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_decks_folder_id")
                            .from(Decks::Table, Decks::FolderId)
                            .to(DeckFolders::Table, DeckFolders::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        // The deck-list ordering key: newest-updated first per (user, game), with a
        // stable id tiebreak for deterministic paging.
        manager
            .create_index(
                Index::create()
                    .name("idx_decks_user_game_updated_at_id")
                    .table(Decks::Table)
                    .col(Decks::UserId)
                    .col(Decks::Game)
                    .col(Decks::UpdatedAt)
                    .col(Decks::Id)
                    .to_owned(),
            )
            .await?;

        // Backs the folder-delete `SET NULL` child-side lookup (`WHERE folder_id = ?`) and
        // the per-folder deck-count aggregate, so neither seq-scans the `decks` table.
        manager
            .create_index(
                Index::create()
                    .name("idx_decks_folder_id")
                    .table(Decks::Table)
                    .col(Decks::FolderId)
                    .to_owned(),
            )
            .await?;

        // Backs the cross-game public deck list (`WHERE user_id = ? AND is_public`
        // ordered by `updated_at DESC, id DESC`) — the `/api/u/{handle}/decks` read.
        manager
            .create_index(
                Index::create()
                    .name("idx_decks_user_public_updated_at_id")
                    .table(Decks::Table)
                    .col(Decks::UserId)
                    .col(Decks::IsPublic)
                    .col(Decks::UpdatedAt)
                    .col(Decks::Id)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Decks::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Decks {
    Table,
    Id,
    UserId,
    Game,
    FolderId,
    Name,
    Description,
    Format,
    IsPublic,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum DeckFolders {
    Table,
    Id,
}

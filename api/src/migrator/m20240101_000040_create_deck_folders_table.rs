use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DeckFolders::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DeckFolders::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(DeckFolders::UserId).integer().not_null())
                    .col(ColumnDef::new(DeckFolders::Game).string().not_null())
                    .col(ColumnDef::new(DeckFolders::Name).string().not_null())
                    .col(
                        ColumnDef::new(DeckFolders::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(DeckFolders::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Deleting a user removes their deck folders.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_deck_folders_user_id")
                            .from(DeckFolders::Table, DeckFolders::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // One folder per (user, game, name): a folder name is unique within a game.
        manager
            .create_index(
                Index::create()
                    .name("idx_deck_folders_user_game_name")
                    .table(DeckFolders::Table)
                    .col(DeckFolders::UserId)
                    .col(DeckFolders::Game)
                    .col(DeckFolders::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DeckFolders::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum DeckFolders {
    Table,
    Id,
    UserId,
    Game,
    Name,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

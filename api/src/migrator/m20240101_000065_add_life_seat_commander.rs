use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Let a seat name the *commander* the player brought instead of one of the owner's
        // decks. The existing `deck_id` only works for decks you built in TCGLense, which is
        // exactly the wrong shape for the people you actually play against: you know what their
        // commander was, you'll never have their deck. The two are alternatives, enforced in the
        // handler (a seat with both would leave the record ambiguous).
        //
        // FK-less and orphan-tolerant for the same reason `deck_id` is, plus one of its own: a
        // catalog re-import can remove a `cards` row, and a game already played must not be
        // deleted or made unreadable by that. Reads resolve the reference and report it absent
        // when it no longer looks up.
        manager
            .alter_table(
                Table::alter()
                    .table(LifeSessionPlayers::Table)
                    .add_column(
                        ColumnDef::new(LifeSessionPlayers::CommanderCardId)
                            .integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Backs resolving a page of seats' commanders in one indexed lookup, the mirror of
        // `idx_life_session_players_deck_id`.
        manager
            .create_index(
                Index::create()
                    .name("idx_life_session_players_commander_card_id")
                    .table(LifeSessionPlayers::Table)
                    .col(LifeSessionPlayers::CommanderCardId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_life_session_players_commander_card_id")
                    .table(LifeSessionPlayers::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(LifeSessionPlayers::Table)
                    .drop_column(LifeSessionPlayers::CommanderCardId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum LifeSessionPlayers {
    Table,
    CommanderCardId,
}

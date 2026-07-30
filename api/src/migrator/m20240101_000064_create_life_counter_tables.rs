use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ---- life_sessions: one tracked game ----
        manager
            .create_table(
                Table::create()
                    .table(LifeSessions::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(LifeSessions::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(LifeSessions::UserId).integer().not_null())
                    .col(ColumnDef::new(LifeSessions::Game).string().not_null())
                    .col(ColumnDef::new(LifeSessions::Name).string().null())
                    .col(ColumnDef::new(LifeSessions::Format).string().null())
                    .col(
                        ColumnDef::new(LifeSessions::StartingLife)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(LifeSessions::Layout).string().not_null())
                    .col(ColumnDef::new(LifeSessions::Status).string().not_null())
                    .col(
                        ColumnDef::new(LifeSessions::StartedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(LifeSessions::FinishedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(LifeSessions::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(LifeSessions::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Deleting a user removes their tracked games (and, through the
                    // cascades below, every seat and life event in them).
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_life_sessions_user_id")
                            .from(LifeSessions::Table, LifeSessions::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // The session-list ordering key: a user's games for one game, newest-started
        // first with an id tiebreak.
        manager
            .create_index(
                Index::create()
                    .name("idx_life_sessions_user_game_started_at_id")
                    .table(LifeSessions::Table)
                    .col(LifeSessions::UserId)
                    .col(LifeSessions::Game)
                    .col(LifeSessions::StartedAt)
                    .col(LifeSessions::Id)
                    .to_owned(),
            )
            .await?;

        // Backs the "do I have a game in progress" filter the tool's landing opens with.
        manager
            .create_index(
                Index::create()
                    .name("idx_life_sessions_user_game_status")
                    .table(LifeSessions::Table)
                    .col(LifeSessions::UserId)
                    .col(LifeSessions::Game)
                    .col(LifeSessions::Status)
                    .to_owned(),
            )
            .await?;

        // ---- life_session_players: the seats ----
        manager
            .create_table(
                Table::create()
                    .table(LifeSessionPlayers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(LifeSessionPlayers::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(LifeSessionPlayers::SessionId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(LifeSessionPlayers::Position)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(LifeSessionPlayers::Name).string().not_null())
                    // Deliberately FK-less and orphan-tolerant (like `price_alerts.card_id`):
                    // deleting a played deck must not fail or take the game's history with
                    // it, so the stats read inner-joins `decks` and simply stops counting a
                    // deck that's gone.
                    .col(ColumnDef::new(LifeSessionPlayers::DeckId).integer().null())
                    .col(
                        ColumnDef::new(LifeSessionPlayers::StartingLife)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(LifeSessionPlayers::Life)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(LifeSessionPlayers::Rotation)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(LifeSessionPlayers::Result)
                            .string()
                            .not_null()
                            .default("none"),
                    )
                    .col(
                        ColumnDef::new(LifeSessionPlayers::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(LifeSessionPlayers::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_life_session_players_session_id")
                            .from(LifeSessionPlayers::Table, LifeSessionPlayers::SessionId)
                            .to(LifeSessions::Table, LifeSessions::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Seats are always read in seat order for one session.
        manager
            .create_index(
                Index::create()
                    .name("idx_life_session_players_session_position")
                    .table(LifeSessionPlayers::Table)
                    .col(LifeSessionPlayers::SessionId)
                    .col(LifeSessionPlayers::Position)
                    .to_owned(),
            )
            .await?;

        // Backs the per-deck record: every seat that played a given deck.
        manager
            .create_index(
                Index::create()
                    .name("idx_life_session_players_deck_id")
                    .table(LifeSessionPlayers::Table)
                    .col(LifeSessionPlayers::DeckId)
                    .to_owned(),
            )
            .await?;

        // ---- life_events: the gain/loss history ----
        manager
            .create_table(
                Table::create()
                    .table(LifeEvents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(LifeEvents::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(LifeEvents::SessionId).integer().not_null())
                    .col(ColumnDef::new(LifeEvents::PlayerId).integer().not_null())
                    .col(ColumnDef::new(LifeEvents::Delta).integer().not_null())
                    .col(ColumnDef::new(LifeEvents::LifeAfter).integer().not_null())
                    .col(ColumnDef::new(LifeEvents::Kind).string().not_null())
                    .col(
                        ColumnDef::new(LifeEvents::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_life_events_session_id")
                            .from(LifeEvents::Table, LifeEvents::SessionId)
                            .to(LifeSessions::Table, LifeSessions::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_life_events_player_id")
                            .from(LifeEvents::Table, LifeEvents::PlayerId)
                            .to(LifeSessionPlayers::Table, LifeSessionPlayers::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // The session timeline: every seat's changes for one game, in the order they
        // happened (the id is monotonic, so it doubles as the tiebreak within a second).
        manager
            .create_index(
                Index::create()
                    .name("idx_life_events_session_id_id")
                    .table(LifeEvents::Table)
                    .col(LifeEvents::SessionId)
                    .col(LifeEvents::Id)
                    .to_owned(),
            )
            .await?;

        // The per-seat replay an undo performs, and the seat's own sparkline.
        manager
            .create_index(
                Index::create()
                    .name("idx_life_events_player_id_id")
                    .table(LifeEvents::Table)
                    .col(LifeEvents::PlayerId)
                    .col(LifeEvents::Id)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Children first — the FKs point up at the session.
        manager
            .drop_table(Table::drop().table(LifeEvents::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(LifeSessionPlayers::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(LifeSessions::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum LifeSessions {
    Table,
    Id,
    UserId,
    Game,
    Name,
    Format,
    StartingLife,
    Layout,
    Status,
    StartedAt,
    FinishedAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum LifeSessionPlayers {
    Table,
    Id,
    SessionId,
    Position,
    Name,
    DeckId,
    StartingLife,
    Life,
    Rotation,
    Result,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum LifeEvents {
    Table,
    Id,
    SessionId,
    PlayerId,
    Delta,
    LifeAfter,
    Kind,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

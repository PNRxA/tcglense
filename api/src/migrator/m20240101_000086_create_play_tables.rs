use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ---- play_rooms: one online manual table ----
        manager
            .create_table(
                Table::create()
                    .table(PlayRooms::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PlayRooms::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // The invite code: six characters from an unambiguous alphabet. It is the
                    // room's public identity (every guest-facing route is keyed by it), so it
                    // carries the uniqueness constraint the minting loop retries against.
                    .col(ColumnDef::new(PlayRooms::Code).string().not_null())
                    .col(ColumnDef::new(PlayRooms::Game).string().not_null())
                    // FK-less and orphan-tolerant like `life_session_players.deck_id`: the host
                    // is the account that opened the table, and a deleted account must not take
                    // a game in progress down with it mid-turn.
                    .col(ColumnDef::new(PlayRooms::HostUserId).integer().not_null())
                    .col(
                        ColumnDef::new(PlayRooms::Label)
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .col(ColumnDef::new(PlayRooms::Format).string().not_null())
                    .col(ColumnDef::new(PlayRooms::StartingLife).integer().not_null())
                    .col(ColumnDef::new(PlayRooms::MaxPlayers).integer().not_null())
                    .col(ColumnDef::new(PlayRooms::Status).string().not_null())
                    // The whole `RoomState` as JSON, written by the registry's sweeper. NULL
                    // while the room is still a lobby (there is no table yet).
                    .col(ColumnDef::new(PlayRooms::State).text().null())
                    .col(
                        ColumnDef::new(PlayRooms::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(PlayRooms::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // The code is the identity every guest-facing route resolves through, and the minting
        // loop treats a violation here as "try another code" — so it must be unique, not just
        // indexed.
        manager
            .create_index(
                Index::create()
                    .name("idx_play_rooms_code")
                    .table(PlayRooms::Table)
                    .col(PlayRooms::Code)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // The host's own room list, and the per-host open-room cap the create path checks.
        manager
            .create_index(
                Index::create()
                    .name("idx_play_rooms_host_user_id")
                    .table(PlayRooms::Table)
                    .col(PlayRooms::HostUserId)
                    .to_owned(),
            )
            .await?;

        // The listing sort (most recently active first) and the sweeper's eviction scan.
        manager
            .create_index(
                Index::create()
                    .name("idx_play_rooms_updated_at")
                    .table(PlayRooms::Table)
                    .col(PlayRooms::UpdatedAt)
                    .to_owned(),
            )
            .await?;

        // ---- play_seats: the seats at a table ----
        manager
            .create_table(
                Table::create()
                    .table(PlaySeats::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PlaySeats::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PlaySeats::RoomId).integer().not_null())
                    .col(ColumnDef::new(PlaySeats::SeatIndex).integer().not_null())
                    // NULL for a guest seat (someone who joined with only a name).
                    .col(ColumnDef::new(PlaySeats::UserId).integer().null())
                    .col(ColumnDef::new(PlaySeats::DisplayName).string().not_null())
                    // Only the SHA-256 hex of the seat token, never the token itself — the same
                    // posture as `api_keys` / `refresh_tokens` (see `auth::secret`).
                    .col(ColumnDef::new(PlaySeats::TokenHash).string().not_null())
                    .col(ColumnDef::new(PlaySeats::DeckSource).string().null())
                    .col(ColumnDef::new(PlaySeats::DeckRef).string().null())
                    .col(ColumnDef::new(PlaySeats::DeckName).string().null())
                    .col(ColumnDef::new(PlaySeats::DeckJson).text().null())
                    .col(
                        ColumnDef::new(PlaySeats::Ready)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(PlaySeats::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // Unlike the host link, a seat *is* a child of its room: deleting the room
                    // deletes the table it seated.
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_play_seats_room_id")
                            .from(PlaySeats::Table, PlaySeats::RoomId)
                            .to(PlayRooms::Table, PlayRooms::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Seats are always read in seat order for one room, and a seat index is claimed at
        // most once — the unique index is what makes two simultaneous joins race safely.
        manager
            .create_index(
                Index::create()
                    .name("idx_play_seats_room_seat_index")
                    .table(PlaySeats::Table)
                    .col(PlaySeats::RoomId)
                    .col(PlaySeats::SeatIndex)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // "Which rooms am I sitting in" — the signed-in half of the room list.
        manager
            .create_index(
                Index::create()
                    .name("idx_play_seats_user_id")
                    .table(PlaySeats::Table)
                    .col(PlaySeats::UserId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PlaySeats::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PlayRooms::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum PlayRooms {
    Table,
    Id,
    Code,
    Game,
    HostUserId,
    Label,
    Format,
    StartingLife,
    MaxPlayers,
    Status,
    State,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum PlaySeats {
    Table,
    Id,
    RoomId,
    SeatIndex,
    UserId,
    DisplayName,
    TokenHash,
    DeckSource,
    DeckRef,
    DeckName,
    DeckJson,
    Ready,
    CreatedAt,
}

use sea_orm::entity::prelude::*;

/// SeaORM entity for the `play_rooms` table.
///
/// One row per **online manual table** in the play tool — the container behind
/// `/api/tools/{game}/play/rooms`. Its seats live in `play_seats` and cascade away with it.
///
/// Unlike every other container surface in the app, a room is **not** addressed by its id:
/// the `code` is its public identity, because the guest-facing half of the feature (join, the
/// lobby read, the socket) has no account to scope by. Authorization therefore hangs off two
/// other things — `host_user_id` for the host-only routes (a non-host's DELETE is a 404, not a
/// 403, so the code isn't an existence oracle), and the per-seat token in `play_seats` for
/// everything a seat does.
///
/// `state` is the whole `play::types::RoomState` as JSON, written by the in-memory registry's
/// periodic sweeper rather than by any request path: the socket is the hot path and a table
/// action must not wait on a database round trip. It is NULL while `status` is `lobby` (there
/// is no table yet — the lobby *is* the seat rows), and the row is the only thing that
/// survives a restart, so a hydrated room resumes from the last swept snapshot.
///
/// `Eq` is derivable — every column is an integer, string, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "play_rooms")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// The invite code — six characters from `handlers::tools::play::tokens::CODE_ALPHABET`,
    /// unique across every room. Stored (and compared) uppercase.
    pub code: String,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// The account that opened the table. FK-less and orphan-tolerant on purpose (the same
    /// call `life_session_players.deck_id` makes): the room outlives the account row.
    pub host_user_id: i32,
    /// Optional table label ("Friday pod"); `""` when the host didn't name it.
    pub label: String,
    /// `commander` / `constructed` — see `play::types::FORMATS`.
    pub format: String,
    /// Life every seat starts on (the format's default unless the host overrode it).
    pub starting_life: i32,
    /// Seat cap, 2..=6.
    pub max_players: i32,
    /// `lobby` / `playing` / `finished` — mirrors `play::types::RoomStatus::as_str`.
    pub status: String,
    /// The persisted `RoomState` as JSON, or NULL while the room is a lobby.
    pub state: Option<String>,
    pub created_at: DateTimeUtc,
    /// Last time anything about the room changed — the listing's sort key and what the
    /// registry's eviction sweep ages rooms by.
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// The seats at this table.
    #[sea_orm(has_many = "super::play_seat::Entity")]
    Seats,
}

impl Related<super::play_seat::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Seats.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

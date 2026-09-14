use sea_orm::entity::prelude::*;

/// SeaORM entity for the `play_seats` table.
///
/// One row per **seat** at an online table: who is sitting there, the deck they loaded, and
/// the hash of the token that proves they hold the seat.
///
/// A seat has **no `user_id` requirement** — a guest joins with nothing but a display name —
/// so the seat token, not the session, is the credential for everything a seat does
/// (`X-Play-Seat`). Only its SHA-256 hex digest is stored, exactly like a refresh token or an
/// API key (`auth::secret`): the plaintext is shown once, by the join response, and is what
/// the socket's `hello` presents. `user_id` is still recorded when the joiner *is* signed in,
/// because that is what lets them re-take their seat from another device (the join resolves a
/// seat by account and rotates the token) and what puts the room in their room list.
///
/// The seat is a child of its room (`room_id`, FK `ON DELETE CASCADE`) and carries no
/// ownership of its own, so every seat-scoped route loads the room first and a seat belonging
/// to another room is a **404**, exactly like a `deck_card` or a life-counter seat.
///
/// `deck_json` is the resolved `Vec<play::types::CardDef>` — the catalog lookup happens once,
/// when the deck is loaded, so starting the game is a pure in-memory build with no card query
/// on the path. `deck_source`/`deck_ref` record *where* it came from (`deck` + a deck id,
/// `precon` + a slug, `text` + nothing) for the lobby's "Bob loaded Krenko" line.
///
/// `Eq` is derivable — every column is an integer, string, bool, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "play_seats")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Parent `play_rooms.id`.
    pub room_id: i32,
    /// Seat order at the table, 0-based. Unique within the room — the index is what makes
    /// two simultaneous joins claim different chairs.
    pub seat_index: i32,
    /// The account holding the seat, or NULL for a guest.
    pub user_id: Option<i32>,
    /// 1..=32 characters after trimming, profanity-checked like a username.
    pub display_name: String,
    /// SHA-256 hex of the seat token (never the token itself).
    pub token_hash: String,
    /// `deck` / `precon` / `text`, or NULL while nothing is loaded.
    pub deck_source: Option<String>,
    /// The deck id or precon slug the deck came from; NULL for a pasted list.
    pub deck_ref: Option<String>,
    pub deck_name: Option<String>,
    /// The resolved decklist as a JSON `Vec<play::types::CardDef>`.
    pub deck_json: Option<String>,
    /// The seat has said it's ready to start (cleared whenever a new deck is loaded).
    pub ready: bool,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::play_room::Entity",
        from = "Column::RoomId",
        to = "super::play_room::Column::Id"
    )]
    Room,
}

impl Related<super::play_room::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Room.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

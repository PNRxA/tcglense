use sea_orm::entity::prelude::*;

/// SeaORM entity for the `life_events` table.
///
/// One row per **life change** in a tracked game — the gain/loss history the life-counter
/// tool charts and lets you undo. Cascades away with its session.
///
/// `session_id` is carried alongside `player_id` so the whole game's history is one indexed
/// scan (the timeline reads every seat's changes interleaved), and so an undo can be scoped
/// to a session the caller has already been proved to own.
///
/// The pair of columns is deliberate: `delta` is what the change *was* (what the history row
/// reads as, `-3`), `life_after` is what it *left the seat on*. Storing the total means the
/// chart and the seat's current life need no fold, and an undo of a mid-history row replays
/// the seat's chain rather than trusting an incrementally-maintained number.
///
/// `kind` is `"adjust"` (a relative tap, the common case) or `"set"` (an absolute
/// correction, where `life_after` is the number the user typed and `delta` is only how far
/// that moved them). A replay must honour that difference — a `set` pins the total, an
/// `adjust` adds to it.
///
/// `Eq` is derivable — every column is an integer, string, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "life_events")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Parent `life_sessions.id` (denormalised from the seat so the session timeline is one
    /// indexed scan).
    pub session_id: i32,
    /// The `life_session_players.id` this change applied to.
    pub player_id: i32,
    /// Signed change in life (`-3`, `+2`). For a `set` event, how far the correction moved
    /// the seat.
    pub delta: i32,
    /// The seat's life total *after* this change.
    pub life_after: i32,
    /// `"adjust"` (relative) or `"set"` (absolute correction).
    pub kind: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

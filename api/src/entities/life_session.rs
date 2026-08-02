use sea_orm::entity::prelude::*;

/// SeaORM entity for the `life_sessions` table.
///
/// One row per **tracked game** in the life-counter tool — the container surface behind
/// `/api/tools/{game}/life/sessions`. Its seats live in
/// `life_session_players` and every life change in `life_events`, both cascading away
/// with the session.
///
/// Like a deck (and unlike a collection / wish list, which is one implicit list per
/// `(user, game)`) a user has **many** sessions per game, so every session-scoped route
/// first proves `life_session.user_id == caller`; a session that isn't theirs is a `404`
/// (never `403` — no existence oracle over session ids).
///
/// `starting_life` is the session default a new seat inherits (each seat also stores its
/// own, so an Archenemy-style game can start seats at different totals). `layout` is a slug
/// from the fixed vocabulary in [`crate::handlers::tools::life`] describing how the seats
/// are placed on screen; per-seat `rotation` refines it. `status` is `active` until the game
/// is finished, at which point `finished_at` is stamped and every seat carries a result —
/// the only sessions the per-deck stats read counts.
///
/// `Eq` is derivable — every column is an integer, string, bool, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "life_sessions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning user (`users.id`).
    pub user_id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// Optional label for the game ("Friday pod", "Round 2"), or null.
    pub name: Option<String>,
    /// Free-form format label (e.g. `"commander"`), mirroring `decks.format`, or null.
    pub format: Option<String>,
    /// Life total a new seat in this session starts on (20 / 30 / 40 / custom).
    pub starting_life: i32,
    /// Seat-placement layout slug — see `handlers::tools::life::LAYOUTS`.
    pub layout: String,
    /// Which counters beyond life this game tracks, as a CSV of slugs from
    /// `handlers::tools::life::counters::OPTIONAL_COUNTERS` (`""` for none). Per-session
    /// because a Standard pod has no business seeing a commander-damage matrix — the same
    /// reason `layout` is a session column rather than a user preference. `life` is implicit
    /// and never listed.
    pub counters: String,
    /// `"active"` while the game is being played, `"finished"` once a result is recorded.
    pub status: String,
    pub started_at: DateTimeUtc,
    /// When the result was recorded; null while the session is active.
    pub finished_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

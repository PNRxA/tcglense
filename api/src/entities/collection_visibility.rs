use sea_orm::entity::prelude::*;

/// SeaORM entity for the `collection_visibility` table.
///
/// One row per `(user, game)` carrying that user's public-sharing state for a
/// single game's collection. `is_public = true` exposes a read-only view of that
/// user's owned cards for `game` at `/api/u/{handle}/{game}` to anyone
/// (unauthenticated); `false` — or no row at all — keeps it private. Visibility is
/// **per game**, so a user can share MTG without sharing another game.
///
/// The row is retained when `is_public` is flipped back to `false` (rather than
/// deleted) so future per-collection display preferences — hiding values or
/// quantities, a custom title — added as extra columns survive a
/// private -> public -> private toggle. Deleting the user cascades this row away.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "collection_visibility")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning user (`users.id`).
    pub user_id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// Whether this user's `game` collection is publicly shareable.
    pub is_public: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

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
/// deleted) so the per-collection display preferences below survive a
/// private -> public -> private toggle. Deleting the user cascades this row away.
///
/// `show_value_chart` / `show_movers` are those display preferences (issue #381): the
/// owner's collection-landing settings menu hides the value-over-time chart and/or the
/// biggest-movers panel per game. Both default `true`, so no row (or a legacy row) means
/// both sections show. They are the owner's own view only — the public read surface never
/// reads them.
///
/// `wishlist_is_public` (issue #493) is the independent sharing flag for that same
/// `(user, game)` wish list — the collection's `is_public` twin. `true` exposes a read-only
/// view of the user's wanted cards + products for `game` at `/api/u/{handle}/wishlist/{game}`;
/// `false` (the default, and every legacy row) keeps it private. Collection and wish-list
/// sharing are toggled independently.
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
    /// Whether the value-over-time chart shows on the owner's collection landing.
    pub show_value_chart: bool,
    /// Whether the biggest-movers (gainers/losers) panel shows on the owner's landing.
    pub show_movers: bool,
    /// Whether this user's `game` wish list is publicly shareable (issue #493).
    pub wishlist_is_public: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

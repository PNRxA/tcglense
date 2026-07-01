use sea_orm::entity::prelude::*;

/// SeaORM entity for the `collection_sources` table.
///
/// One saved external collection link per `(user, game)`: which provider (e.g.
/// `"archidekt"`) and the provider-side collection id, so the user can re-sync on
/// demand without re-entering the URL. `last_synced_at` records the last successful
/// sync (a mirror/replace). There is at most one row per user per game — saving a new
/// link upserts onto it.
///
/// `user_id` references `users.id` (cascade-deleted with the user). `Eq` is derivable
/// — every column is an integer, string, timestamp, or nullable thereof.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "collection_sources")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning user (`users.id`).
    pub user_id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// Collection provider id, e.g. `"archidekt"`.
    pub provider: String,
    /// The provider-side collection id (e.g. an Archidekt numeric id).
    pub external_id: String,
    /// When this link was last successfully synced, if ever.
    pub last_synced_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

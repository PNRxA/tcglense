use sea_orm::entity::prelude::*;

/// SeaORM entity for the `deck_folders` table.
///
/// One row per user-created folder used to organise a game's decks (issue #363).
/// A folder is a lightweight label — unique per `(user, game, name)` — that decks
/// point at via their nullable `decks.folder_id`; deleting a folder **ungroups** its
/// decks (their `folder_id` is set null) rather than deleting them. Deleting the user
/// cascades the folder away.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "deck_folders")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning user (`users.id`).
    pub user_id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// Display name, unique per `(user, game)`.
    pub name: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

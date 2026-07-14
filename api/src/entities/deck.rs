use sea_orm::entity::prelude::*;

/// SeaORM entity for the `decks` table.
///
/// One row per user-built deck for a game (issue #363). A deck is a first-class,
/// named container of cards — its cards live in `deck_cards`, grouped into
/// `deck_sections`. Unlike a collection / wish list (one implicit list per
/// `(user, game)`), a user has **many** decks per game, so every deck-scoped query
/// first proves `deck.user_id == caller`; a deck that isn't the caller's is a `404`
/// (never `403` — no existence oracle).
///
/// `folder_id` optionally files the deck under a `deck_folders` row (null = loose).
/// `is_public` independently exposes a read-only view at `/api/u/{handle}/decks/{id}`
/// — the per-collection sharing model of issue #361, but **per deck**, so the flag
/// lives on the deck row itself (no separate visibility table needed). Deleting the
/// user cascades the deck — and its sections + cards — away.
///
/// `Eq` is derivable — every column is an integer, string, bool, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "decks")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning user (`users.id`).
    pub user_id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// Optional `deck_folders.id` this deck is filed under (null = not in a folder).
    pub folder_id: Option<i32>,
    pub name: String,
    pub description: Option<String>,
    /// Free-form format label (e.g. `"commander"`, `"standard"`), or null.
    pub format: Option<String>,
    /// Whether this deck is publicly shareable by handle.
    pub is_public: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

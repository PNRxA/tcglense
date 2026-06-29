use sea_orm::entity::prelude::*;

/// SeaORM entity for the `card_sets` table.
///
/// Generic across trading-card games: every row carries a `game` discriminator
/// (e.g. `"mtg"`) so additional TCGs can share the table without a schema change.
/// For MTG these rows are sourced from Scryfall's `/sets` endpoint (paper sets
/// only). `code` is unique *within* a game, not globally.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "card_sets")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// Set code, unique within a game (e.g. `"blb"`). Stored lowercase.
    pub code: String,
    pub name: String,
    /// Provider set classification (Scryfall `set_type`, e.g. `"expansion"`).
    pub set_type: Option<String>,
    /// Release date as an ISO `YYYY-MM-DD` string (Scryfall `released_at`).
    pub released_at: Option<String>,
    /// Number of cards the provider reports for the set.
    pub card_count: i32,
    /// Whether the set is digital-only. Paper-only ingestion stores `false`.
    pub digital: bool,
    /// URL of the set's SVG icon (Scryfall `icon_svg_uri`).
    pub icon_svg_uri: Option<String>,
    /// Parent set code for tokens / promos that hang off a main set.
    pub parent_set_code: Option<String>,
    /// External provider id (Scryfall set id).
    pub external_id: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

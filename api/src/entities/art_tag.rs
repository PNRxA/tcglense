use sea_orm::entity::prelude::*;

/// SeaORM entity for the `art_tags` table.
///
/// One row per Scryfall Tagger **art tag** — a community label describing what a card's
/// artwork depicts (e.g. `squirrel`, `mountain-range`), sourced from Scryfall's `art_tags`
/// bulk data (issue #140). This is the tag *metadata* table (the search itself hits the
/// `card_art_tags` mapping): it powers the art-tag autocomplete in the advanced-search
/// panel. Generic across games via the `game` discriminator; refreshed wholesale by
/// `scryfall::art_tags::refresh` alongside the mapping table.
///
/// `Eq` is derivable — every column is an integer or string.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "art_tags")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// The tag's stable Tagger UUID. Slugs may drift as the community renames tags; this
    /// id is the durable identity across refreshes.
    pub scryfall_id: String,
    /// URL-safe tag name, e.g. `squirrel` — the value the `art:` search filter matches.
    pub slug: String,
    /// Human-readable tag name, e.g. `Squirrel`.
    pub label: String,
    /// Optional community description of what the tag represents.
    #[sea_orm(column_type = "Text", nullable)]
    pub description: Option<String>,
    /// Distinct stored artworks carrying this tag after hierarchy expansion (a parent tag
    /// counts its descendants' taggings) — the autocomplete's relevance-ranking signal.
    pub taggings_count: i32,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

use sea_orm::entity::prelude::*;

/// SeaORM entity for the `card_art_tags` table.
///
/// The art-tag → artwork mapping behind the `art:`/`arttag:`/`atag:` search filters
/// (issue #140): one row per `(tag_slug, illustration_id)` pair, joined to
/// `cards.illustration_id`. Rows are **hierarchy-expanded at ingest** — an artwork
/// directly tagged `squirrel` also gets a row for every ancestor tag (`rodent`,
/// `animal`, …) — so the search is a single indexed `EXISTS` lookup with no
/// query-time tag-tree traversal. The tag slug is denormalized onto each row for the
/// same reason (no join against `art_tags` on the hot path); the whole game's rows are
/// rebuilt wholesale by `scryfall::art_tags::refresh`, so slug drift self-corrects.
///
/// `id` is `i64`: the daily wholesale rebuild re-inserts ~1M rows per refresh, which
/// would exhaust an `i32` sequence on Postgres within a few years.
///
/// `Eq` is derivable — every column is an integer or string.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "card_art_tags")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// The (canonical) tag slug, e.g. `squirrel` — denormalized from `art_tags.slug`.
    pub tag_slug: String,
    /// The tagged artwork (Scryfall `illustration_id`); joins to `cards.illustration_id`.
    /// Not a foreign key — like `card_rulings`, this is a separately-refreshed,
    /// wholesale-rebuilt table and a card row may be absent during a card re-import.
    pub illustration_id: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

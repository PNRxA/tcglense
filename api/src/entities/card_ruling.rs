use sea_orm::entity::prelude::*;

/// SeaORM entity for the `card_rulings` table.
///
/// One row per Scryfall ruling — the "Notes and Rules Information" shown on a card: an
/// official clarification of how the card works, sourced from Scryfall's `rulings` bulk
/// data (issue #522). Rulings key on `oracle_id` (the gameplay identity `cards.oracle_id`
/// shares across every printing), so a card's rulings are all rows whose `oracle_id`
/// matches — every printing of the same card shows the same list. Generic across games
/// via the `game` discriminator; refreshed wholesale by `scryfall::rulings::refresh`.
///
/// `Eq` is derivable — every column is an integer or string.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "card_rulings")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// Gameplay identity the ruling applies to (Scryfall `oracle_id`); joins to
    /// `cards.oracle_id`.
    pub oracle_id: String,
    /// Who published the ruling — `"wotc"` (Wizards of the Coast) or `"scryfall"`.
    pub source: String,
    /// Publication date as `"YYYY-MM-DD"` (mirrors how `released_at` is stored).
    pub published_at: String,
    /// The ruling text itself.
    pub comment: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

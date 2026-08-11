use sea_orm::entity::prelude::*;

/// SeaORM entity for the `precon_deck_cards` table.
///
/// One row per `(precon deck, board, card, finish)`: how many copies of a printing the
/// published deck list contains. Unlike a user's [`deck_card`](super::deck_card) — which
/// carries a regular **and** a foil count on one row — a precon row is a single finish,
/// because that's how upstream states it (`isFoil` per entry) and a precon is never edited.
///
/// `card_id` is the internal `cards.id` (like `deck_cards`), so a precon card survives a
/// catalog re-import; there is deliberately no FK to `cards`, and the reads LEFT-join and
/// skip a card whose row is gone.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "precon_deck_cards")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning precon (`precon_decks.id`).
    pub precon_deck_id: i32,
    /// `cards.id` this row is for (internal integer id).
    pub card_id: i32,
    /// Which board the copies sit on — one of [`PreconBoard`]'s string values.
    pub board: String,
    /// Copies of this printing on the board, in this finish.
    pub quantity: i32,
    /// Whether those copies are foil.
    pub foil: bool,
    /// Upstream's listing order within the board (a Secret Lair drop's card order).
    pub position: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// The card this row is for (`card_id` -> `cards.id`), so a precon's card list joins
    /// `cards` with `find_also_related` exactly as a deck's does.
    #[sea_orm(
        belongs_to = "super::card::Entity",
        from = "Column::CardId",
        to = "super::card::Column::Id"
    )]
    Card,
    /// The precon this card belongs to.
    #[sea_orm(
        belongs_to = "super::precon_deck::Entity",
        from = "Column::PreconDeckId",
        to = "super::precon_deck::Column::Id"
    )]
    PreconDeck,
}

impl Related<super::card::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Card.def()
    }
}

impl Related<super::precon_deck::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PreconDeck.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

/// The three boards a published deck list is stated in. Stored as the string values below
/// (not an enum column) so the vocabulary is greppable in the DB, matching how
/// `sealed_content::Membership` is stored.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PreconBoard {
    /// The deck proper.
    Main,
    /// The command zone (a Commander deck's commander(s), an Oathbreaker pair).
    Commander,
    /// The sideboard, counted apart from the deck.
    Side,
}

impl PreconBoard {
    pub fn as_str(self) -> &'static str {
        match self {
            PreconBoard::Main => "main",
            PreconBoard::Commander => "commander",
            PreconBoard::Side => "side",
        }
    }
}

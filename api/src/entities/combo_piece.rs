use sea_orm::entity::prelude::*;

/// SeaORM entity for the `combo_pieces` table: one **card** a combo needs (issue #683).
///
/// Keyed by `oracle_id` — the gameplay identity every printing of a card shares — because
/// a combo is a fact about the card, not a printing. Joins to `cards.oracle_id` without a
/// foreign key: the table is rebuilt wholesale with its parent, and a piece the catalog
/// doesn't hold (a spoiler, a digital-only card) still describes the combo truthfully.
///
/// The deck read's one scan is "which combos does this deck touch" — this table by the
/// deck's oracle ids, **joined to the parent** for the combo's size and popularity — so it
/// decides *complete / one short / further* per combo from that scan alone rather than
/// loading tens of thousands of parent rows one by one.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "combo_pieces")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    pub combo_id: i32,
    /// The card's gameplay identity (Scryfall `oracle_id`); joins to `cards.oracle_id`.
    pub oracle_id: String,
    /// The card's name as upstream spells it (for display when the catalog lacks it).
    pub name: String,
    /// Copies of it the combo needs (almost always 1; Relentless Rats-style combos want more).
    pub quantity: i32,
    /// Whether the card has to be in the command zone for the combo to work.
    pub must_be_commander: bool,
    /// Where the card starts, as upstream's zone letters comma-joined (`"B"` battlefield,
    /// `"H"` hand, `"G"` graveyard, `"L"` library, `"E"` exile, `"C"` command zone).
    pub zone_locations: String,
    /// The card's position in the combo's own piece order (upstream lists the most
    /// important first).
    pub position: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::combo::Entity",
        from = "Column::ComboId",
        to = "super::combo::Column::Id"
    )]
    Combo,
}

impl Related<super::combo::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Combo.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

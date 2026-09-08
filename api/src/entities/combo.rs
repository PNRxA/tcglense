use sea_orm::entity::prelude::*;

/// SeaORM entity for the `combos` table: one published Commander Spellbook **variant** — a
/// specific set of cards that together produce an effect (issue #683).
///
/// Combos are a **dataset, not a grammar**: nothing in a card's rules text says which other
/// card it goes infinite with, so the rows come from Commander Spellbook's bulk export
/// (`spellbook::ingest`), and the table is **rebuilt wholesale** on every changed version.
/// Its `id` is therefore not stable and never reaches the wire; `external_id` (Spellbook's
/// own variant id, `"245-2034-6705"`) is the identity a client links by.
///
/// The pieces live in [`super::combo_piece`], keyed by **`oracle_id`** — a combo is a
/// gameplay-level fact, so every printing of a card participates.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "combos")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// Commander Spellbook's variant id — the URL identity (`commanderspellbook.com/combo/{id}`).
    pub external_id: String,
    /// The combo's colour identity as WUBRG letters, comma-joined like `cards.color_identity`
    /// (`"W,U"`; `""` for colourless).
    pub color_identity: String,
    /// Mana needed to start the combo, Scryfall-style (`"{6}"`); empty when none.
    pub mana_needed: String,
    /// Mana value of `mana_needed`, when upstream states one.
    pub mana_value_needed: Option<i32>,
    /// Prerequisites beyond having the pieces (upstream's easy + notable prerequisites,
    /// newline-joined); empty when none.
    pub prerequisites: String,
    /// The step-by-step description of how the combo works.
    pub description: String,
    /// Upstream's free-text notes; empty when none.
    pub notes: String,
    /// Upstream's popularity counter (how many published decks run it); higher = more common.
    pub popularity: i32,
    /// Upstream's bracket tag for the combo (`C` casual, `R` ruthless, …); empty when unset.
    pub bracket_tag: String,
    /// How many **card** pieces the combo needs ([`super::combo_piece`] rows).
    pub piece_count: i32,
    /// How many **template** pieces it needs — "any sac outlet"-style wildcards this app
    /// can't evaluate, so a combo with any counts as at least one card short.
    pub template_count: i32,
    /// The template names, as a JSON list of strings (`["A free sacrifice outlet"]`).
    pub templates: String,
    /// What the combo produces, as a JSON list of `{name, status}` (`"S"` standalone /
    /// `"C"` contextual / `"H"` helper / `"HU"`,`"PU"` utility).
    pub produces: String,
    /// Whether every piece is Commander-legal per upstream.
    pub commander_legal: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::combo_piece::Entity")]
    ComboPiece,
}

impl Related<super::combo_piece::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ComboPiece.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

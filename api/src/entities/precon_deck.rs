use sea_orm::entity::prelude::*;

/// SeaORM entity for the `precon_decks` table.
///
/// One row per **preconstructed** deck a publisher shipped with a set: a Commander deck,
/// a Planeswalker / Challenger / Starter deck, a Jumpstart theme, a Secret Lair drop, …
/// Unlike [`deck`](super::deck) — which is a user's own container — this is *catalog*
/// data, derived from MTGJSON's per-set `decks[]` during the sealed-contents sync
/// ([`crate::mtgjson::precons`]) and rebuilt wholesale on every run.
///
/// Because the rebuild re-mints primary keys, `id` is **not** the stable identity:
/// `(game, slug)` is, and every URL, read and copy addresses a precon by its slug. A user
/// who wants one of these in their own decks *copies* it (a new `decks` row), so nothing
/// user-owned ever points at this table.
///
/// The derived facets (`color_identity`, `card_count`, `face_card_id`) are computed once at
/// ingest rather than folded per request the way [`deck` facets](crate::handlers::decks)
/// are: a precon is immutable between syncs, and the browse list is a public,
/// CDN-cacheable read that must not pay a per-row card scan.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "precon_decks")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game id slug (`mtg`).
    pub game: String,
    /// URL identity, unique per game — `slugify(name)-set_code`, e.g. `turtle-power-tmc`.
    pub slug: String,
    /// Upstream's deck name ("Turtle Power!").
    pub name: String,
    /// The set the deck ships with, lowercased to match `cards.set_code`.
    pub set_code: String,
    /// Upstream's category: "Commander Deck", "Secret Lair Drop", "Jumpstart", …
    pub deck_type: String,
    /// ISO release date (`2026-03-06`), as `products.released_at` stores it.
    pub released_at: Option<String>,
    /// WUBRG-ordered colour letters (`"WUB"`), `""` for a colourless deck, `None` when
    /// there was nothing to read a colour off — the same three-way distinction the deck
    /// list's `color_identity` makes.
    pub color_identity: Option<String>,
    /// Total copies in the deck proper (mainboard + command zone).
    pub card_count: i32,
    /// Total copies in the sideboard, counted apart so `card_count` means "the deck".
    pub sideboard_count: i32,
    /// `cards.id` of the card that fronts the deck (its first commander, else the first
    /// card upstream lists). Orphan-tolerant — reads LEFT-join it.
    pub face_card_id: Option<i32>,
    /// `products.id` of the sealed product that ships this deck, when MTGJSON linked one
    /// and we carry it. Orphan-tolerant for the same reason.
    pub product_id: Option<i32>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// The deck's cards (`precon_deck_cards.precon_deck_id`).
    #[sea_orm(has_many = "super::precon_deck_card::Entity")]
    Cards,
}

impl Related<super::precon_deck_card::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Cards.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

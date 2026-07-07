use sea_orm::entity::prelude::*;

/// SeaORM entity for the `sealed_components` table: the **structural composition** of a
/// sealed product — "what's in the box" — sourced from [MTGJSON](https://mtgjson.com)'s
/// sealed-product `contents` (see [`crate::mtgjson`]).
///
/// Where [`super::sealed_content`] flattens a product down to the individual *cards* it
/// contains or can yield, this table keeps the product's *packaging* intact: one row per
/// component — a nested pack/box (`sealed`), a precon deck (`deck`), a fixed promo card
/// (`card`), or a physical extra like a die or storage box (`other`) — each with a
/// quantity, so the SPA can render "9× Play Booster, 1× Collector Booster, 1× Spindown, …"
/// and link the sub-products the box contains.
///
/// `product_id` (the parent) and the optional `child_product_id` / `child_card_id` (the
/// link target when a component *is* another catalog product or card) are the **internal**
/// integer ids, not the providers' external ids — so a row survives a catalog / product
/// re-import, mirroring how [`super::sealed_content`] links. `position` fixes the display
/// order (MTGJSON's order, kinds grouped). The whole table is rebuilt per game on each
/// sync, so stale composition never lingers.
///
/// `Eq` is derivable — every column is an integer, string, bool, option, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "sealed_components")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// `products.id` of the product this is a component of (internal integer id).
    pub product_id: i32,
    /// Display order within the product (0-based, over the ordered component list).
    pub position: i32,
    /// The component kind: `"sealed"` (a nested pack/box sub-product), `"deck"` (a precon
    /// deck), `"card"` (a fixed promo card), or `"other"` (a physical extra — a die,
    /// storage box, land pack, …). See [`ComponentKind`].
    pub kind: String,
    /// Human-readable label from MTGJSON (e.g. `"…Play Booster Pack"`, `"…Spindown"`). The
    /// handler prefers a linked child's catalog name when one resolves, falling back to this.
    pub name: String,
    /// How many of this component the product contains (a booster count, etc.); `>= 1`.
    pub quantity: i32,
    /// `products.id` of the sub-product this component *is*, when a `sealed` component
    /// resolves to a product in our catalog (the link target). `None` otherwise.
    pub child_product_id: Option<i32>,
    /// `cards.id` of the card this component *is*, when a `card` component resolves to a
    /// card in our catalog (the link target). `None` otherwise.
    pub child_card_id: Option<i32>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

/// The component kinds a `sealed_components` row can carry, as stored in the `kind`
/// column. The single source of truth for the string values, shared by the ingest (which
/// writes them) and the handler (which groups/orders by them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
    /// A nested sealed sub-product — a booster pack or box the product bundles (the only
    /// kind that links to another catalog product). Surfaced as the linkable line items.
    Sealed,
    /// A preconstructed deck the product includes.
    Deck,
    /// A fixed promo card the product includes (links to the card, when in our catalog).
    Card,
    /// A physical extra — a spindown die, a storage box, a basic-land pack, …
    Other,
}

impl ComponentKind {
    /// The stored string value.
    ///
    /// The composition builder emits components in a fixed display order — nested
    /// packs/boxes (the headline "9× Play Booster") first, then decks, promo cards, and
    /// physical extras — by iterating the kinds in that sequence, so `position` captures it.
    pub fn as_str(self) -> &'static str {
        match self {
            ComponentKind::Sealed => "sealed",
            ComponentKind::Deck => "deck",
            ComponentKind::Card => "card",
            ComponentKind::Other => "other",
        }
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

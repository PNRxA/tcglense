//! **Preconstructed decks**: the decklists a publisher shipped — Commander decks,
//! Planeswalker / Challenger / Starter decks, Jumpstart themes, intro packs.
//!
//! Where [`decks`](crate::handlers::decks) is a *user's* container surface, this is the
//! **catalog** side of the same idea: rows derived from MTGJSON's per-set `decks[]` during
//! the sealed sync ([`crate::mtgjson::precons`]), the same public game data a card or a
//! sealed product is. So the reads live in the router's `public` group beside
//! `/api/games/{game}/products` — anonymous, CDN-cacheable, `ETag`-validated — rather than
//! in the authed deck group, and a precon is addressed by its **slug**, never its id (the
//! tables are rebuilt wholesale on every sync, so ids are re-minted; see the ingest note).
//!
//! The one write, [`copy`], is the bridge back to the user's own surface: it turns a precon
//! into a real deck of theirs. It's the *same* operation
//! [`decks::copy`](crate::handlers::decks) performs on a shared public deck — both hold
//! internal card ids already — so both go through that module's `insert_deck_with_cards`
//! seam and only differ in where the sections come from.
//!
//! Three couplings worth keeping straight:
//!
//! * **Board -> section is decided here, once.** A precon states `commander` / `main` /
//!   `side`; a deck has named sections. The mapping lands the command zone in `Commander`
//!   and the sideboard in `Sideboard` **because those exact spellings are what
//!   `decks::analysis::rules` reads a deck's zones off** — a copied Commander precon that
//!   filed its commander anywhere else would come back "illegal: no commander".
//! * **The mainboard is auto-filed** through `deck_import::categorize::preset_section`, the
//!   same type-line -> bucket table a deck import uses, so a copied precon looks like an
//!   imported one rather than a single undifferentiated pile.
//! * **Nothing user-owned points at a precon.** A copy duplicates rows; it stores no
//!   reference. That's what lets the sync replace these tables wholesale.

use serde::{Deserialize, Serialize};

use crate::entities::precon_deck;
use crate::error::AppError;
use crate::handlers::shared::valuation::format_cents;
use crate::handlers::shared::{CardResponse, CollectionSummary, ProductResponse};
use crate::state::AppState;

mod analysis;
mod copy;
mod read;

pub use analysis::{precon_bracket, precon_goldfish, precon_legality, precon_stats, precon_tokens};
pub use copy::copy_precon_deck;
pub use read::{card_precons, get_precon, list_precon_groups, list_precons, precon_facets};

pub use analysis::{
    __path_precon_bracket, __path_precon_goldfish, __path_precon_legality, __path_precon_stats,
    __path_precon_tokens,
};
pub use copy::__path_copy_precon_deck;
pub use read::{
    __path_card_precons, __path_get_precon, __path_list_precon_groups, __path_list_precons,
    __path_precon_facets,
};

// ---------- Response DTOs ----------

/// A precon deck header, for the browse grid: what it is, when it came out, how big it is,
/// and the card that fronts it.
///
/// `color_identity` follows the deck list's three-way convention exactly (`["W","U"]`,
/// `[]` for colourless, **`null`** for "nothing to read a colour off"), because it is folded
/// by the same rule — the command zone's colours when the deck has one, the union over its
/// mainboard otherwise.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PreconDeck"))]
pub struct PreconDeckResponse {
    /// URL identity — stable across syncs, unlike the row's id (which is not on the wire).
    pub slug: String,
    pub game: String,
    pub name: String,
    /// The set the deck ships with, lowercased (`tmc`).
    pub set_code: String,
    /// That set's display name, when the catalog holds it.
    pub set_name: Option<String>,
    /// Upstream's category: "Commander Deck", "Secret Lair Drop", "Jumpstart", …
    pub deck_type: String,
    pub released_at: Option<String>,
    pub color_identity: Option<Vec<String>>,
    /// Copies in the deck proper (mainboard + command zone).
    pub card_count: i32,
    /// Copies in the sideboard, counted apart from `card_count`.
    pub sideboard_count: i32,
    /// Estimated USD value of the deck proper (regular copies at `usd`, foil copies at
    /// `usd_foil`, sideboard excluded — the same grain as `card_count`), a 2-dp decimal
    /// string. `null` when none of its cards are priced — never `"0.00"`. Folded from the
    /// live card prices once per sync tick (`catalog::precon_values`), through the same
    /// valuation the detail page's `summary.total_value_usd` uses, so the tile and the
    /// page it opens agree.
    pub price_usd: Option<String>,
    /// The card that fronts the deck — its commander, else the first card upstream lists.
    /// `None` when that card is no longer in the catalog.
    pub face_card: Option<PreconFaceCard>,
}

/// Just enough of the face card to render a tile: the **external** card id (so the SPA
/// builds the image + link URLs it already builds for a card), its name, and whether an
/// image exists. Deliberately not a whole [`CardResponse`] — a page of 60 tiles would
/// otherwise carry 60 full card payloads to draw 60 thumbnails.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PreconFaceCard"))]
pub struct PreconFaceCard {
    /// The provider **external** card id (a Scryfall UUID for MTG).
    pub card_id: String,
    pub name: String,
    /// Whether an image is available through the image proxy for this card.
    pub has_image: bool,
}

/// One card of a precon: the full public card payload plus which board it sits on and how
/// many copies, in which finish.
///
/// A precon row is a **single finish** (`foil`), unlike a deck card's regular+foil pair,
/// because that is how a published decklist states it: a foil commander is a different line
/// from the same card in the 99.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PreconCardEntry {
    pub card: CardResponse,
    /// `"commander"`, `"main"`, or `"side"` — see
    /// [`crate::entities::precon_deck_card::PreconBoard`].
    pub board: String,
    pub quantity: i32,
    pub foil: bool,
}

/// The full single-precon view: the header, the value summary, every card in board order,
/// and the sealed product that ships it (when the catalog holds one).
///
/// Like a deck's detail this is returned whole — a precon is bounded — and the SPA groups
/// `cards` by `board`. `summary` covers the deck **proper**, matching the header's
/// `card_count`; the sideboard is summarised separately so it can't inflate the deck's value.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PreconDeckDetail {
    #[serde(flatten)]
    #[schema(inline)]
    pub deck: PreconDeckResponse,
    /// The deck format its *type* states (`Commander Deck` -> `commander`), or `None` when the
    /// type states none — the same mapping the copy writes onto the deck it creates, so the
    /// page judges the list against exactly what a copy of it would be judged against. The SPA
    /// needs it to know whether to ask for a bracket at all: `deck_type` itself doesn't
    /// normalise to a format key, so passing that through would silently never ask.
    pub format: Option<String>,
    /// Value / copy aggregates over the deck proper (commander + mainboard).
    pub summary: CollectionSummary,
    /// The same aggregates over the sideboard alone; all-zero when there isn't one.
    pub sideboard_summary: CollectionSummary,
    pub cards: Vec<PreconCardEntry>,
    /// The sealed product this deck ships in, for the "buy it" link + its price.
    pub product: Option<ProductResponse>,
}

/// One preconstructed deck containing a card, for the card page's "appears in" list: the
/// deck's browse header plus how the card sits in it.
///
/// Containment is at **gameplay identity** (any printing of the card counts, the stance
/// `/prints` and the needed-cards list take), so `quantity` sums every matching row —
/// across boards, printings and finishes. `foil` follows the sealed-membership rule:
/// `true` only when *every* copy is foil, a foil-only inclusion.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "CardPreconRef"))]
pub struct CardPreconRef {
    pub precon: PreconDeckResponse,
    /// Total copies of the card in the deck — any board, any printing, any finish.
    pub quantity: i64,
    /// `true` when every copy is foil (a foil-only inclusion).
    pub foil: bool,
    /// `true` when a copy sits in the deck's command zone — the card *leads* this deck.
    pub commander: bool,
}

/// How a grouped listing buckets its decks.
///
/// The two questions a precon browser gets asked: "what came in this set" and "show me the
/// Commander decks". They're one endpoint rather than two because everything else about the
/// response — the filters, the per-group pagination, the shaping — is identical; only the key
/// function differs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PreconGrouping {
    /// By the set that published them (the default).
    #[default]
    Set,
    /// By upstream's deck category — "Commander Deck", "Jumpstart", "Secret Lair Drop", …
    Type,
}

/// One group of preconstructed decks — a set, or a deck type — for the grouped views.
///
/// The precon mirror of the card catalog's by-drop grouping, and paginated the same way: a page
/// is a page of **groups**, so a group's decks are never split across a boundary. It carries
/// the drop/sub-type group shape (`slug` + `title` + a count + the items) for the same reason
/// those two do: one DTO the client renders through one section component, whichever grouping
/// produced it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PreconGroup"))]
pub struct PreconGroup {
    /// Stable key for anchors/links: the set code (`tmc`) when grouping by set, a slugified
    /// deck type (`commander-deck`) when grouping by type.
    pub slug: String,
    /// Heading text: the set's catalog name (falling back to its upper-cased code), or the
    /// deck type verbatim.
    pub title: String,
    /// The **set's** release date when grouping by set (not its decks' — a Secret Lair deck
    /// published years after the `sld` set still belongs to `sld`), falling back to the newest
    /// deck in the group. Always `null` when grouping by type, which has no date.
    pub released_at: Option<String>,
    /// The set code this group links to, when grouping by set — the group heading's own page.
    /// `null` for a type group, which has no set page.
    pub set_code: Option<String>,
    /// How many decks are in this group (`decks.len()`, carried so a client can label the
    /// heading without counting).
    pub deck_count: usize,
    pub decks: Vec<PreconDeckResponse>,
}

/// A deck type that actually occurs, with how many decks carry it — the browse filter's
/// vocabulary, published rather than hardcoded (upstream adds categories).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PreconTypeRef"))]
pub struct PreconTypeRef {
    #[serde(rename = "type")]
    pub deck_type: String,
    pub count: i64,
}

/// A set that has precon decks (code + resolved name + count), for the set filter.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PreconSetRef"))]
pub struct PreconSetRef {
    pub code: String,
    pub name: Option<String>,
    pub count: i64,
    /// The set's release date, so the SPA can order the filter newest-first.
    pub released_at: Option<String>,
}

/// The filter vocabulary for a game's precons: every type and every set that has one.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PreconFacets"))]
pub struct PreconFacets {
    /// Deck types, most decks first (so "Commander Deck" leads, not "Advanced Deck").
    pub types: Vec<PreconTypeRef>,
    /// Sets that have precons, newest release first.
    pub sets: Vec<PreconSetRef>,
    /// Total precon decks for the game — the browse header's count before any filter.
    pub total: i64,
}

// ---------- Request DTOs ----------

/// Query for `GET /api/games/{game}/precons`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct PreconListParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    /// Name substring; every whitespace-separated word must match (the sealed list's rule).
    pub q: Option<String>,
    pub set: Option<String>,
    /// With `set`, span that set's whole catalog **group** (its top-level root plus every
    /// related sub-set) instead of the one code — the precon mirror of the card listing's
    /// own `include_related`, and what the landing's grouped "All decks" link rides.
    /// Ignored without a `set`, which already spans everything.
    pub include_related: Option<bool>,
    #[serde(rename = "type")]
    pub deck_type: Option<String>,
    /// `released` (default, newest first), `name`, or `price` (most valuable first).
    pub sort: Option<String>,
    /// Grouped listings only: what to bucket by (`set`, the default, or `type`).
    #[serde(default)]
    pub group: PreconGrouping,
}

/// Query for `GET /api/games/{game}/cards/{id}/precons` — pagination only; the card in the
/// path is the filter.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct CardPreconsParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

// ---------- Shared helpers ----------

/// Shape a stored row + its set name + face card into the wire header.
pub(crate) fn precon_response(
    model: &precon_deck::Model,
    set_name: Option<String>,
    face_card: Option<PreconFaceCard>,
) -> PreconDeckResponse {
    PreconDeckResponse {
        slug: model.slug.clone(),
        game: model.game.clone(),
        name: model.name.clone(),
        set_code: model.set_code.clone(),
        set_name,
        deck_type: model.deck_type.clone(),
        released_at: model.released_at.clone(),
        // Stored as contiguous WUBRG letters (`"WUB"`), on the wire as the per-letter array
        // every other colour field uses — and `None` stays `None`, which is the "no cards to
        // read a colour off" case, not "colourless".
        color_identity: model
            .color_identity
            .as_ref()
            .map(|letters| letters.chars().map(|c| c.to_string()).collect()),
        card_count: model.card_count,
        sideboard_count: model.sideboard_count,
        // Cents -> the 2-dp decimal string every USD value rides the wire as; NULL stays
        // `null` (an unpriced deck, as distinct from one worth $0.00).
        price_usd: model.price_cents.map(|cents| format_cents(cents.into())),
        face_card,
    }
}

/// Load a precon by its slug for a game, or `404`. The slug is the identity, so this is the
/// one lookup every read and the copy go through.
pub(crate) async fn load_precon(
    state: &AppState,
    game: &str,
    slug: &str,
) -> Result<precon_deck::Model, AppError> {
    use crate::entities::prelude::PreconDeck;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    PreconDeck::find()
        .filter(precon_deck::Column::Game.eq(game))
        .filter(precon_deck::Column::Slug.eq(slug))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("preconstructed deck not found".to_string()))
}

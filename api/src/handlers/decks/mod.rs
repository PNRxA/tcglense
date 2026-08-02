//! Authenticated, per-user **decks** (issues #363 and #389).
//!
//! A deck is a first-class, named container of cards for a game
//! (`/api/decks/{game}/...`), organised into user-orderable **sections** (Archidekt-style
//! categories — Commander / Lands / Ramp / Removal / …) and, at the deck level, into
//! **folders**. Unlike the collection / wish list (one implicit list per `(user, game)`),
//! a user has **many** decks, so every deck-scoped route first proves the deck belongs to
//! the caller ([`load_deck`]); a deck that isn't theirs is a `404` (never `403` — no
//! existence oracle, matching the public-sharing surface).
//!
//! A deck card is the same two-count shape as a holding, so `deck_card::Model` implements
//! [`HoldingCounts`](crate::handlers::shared::holdings) and the deck reads reuse the
//! shared card payload, valuation, and summary machinery (`handlers::shared`). What's new
//! versus the twin holdings surfaces: the parent deck + folder + section entities and
//! their CRUD, and a per-deck `is_public` flag for handle-addressed public sharing (the
//! per-collection model of #361, but per deck — see `handlers::sharing::decks`).
//! Whole-deck provider import/export is a sibling pipeline: explicit provider
//! categories/boards become sections, generic Mainboard rows may be auto-filed by type,
//! and the new deck is inserted atomically without collection reconciliation.
//!
//! Every route is in the router's `private` group (authenticated via
//! [`AuthUser`](crate::auth::extractor::AuthUser) / [`WritableUser`](crate::auth::extractor::WritableUser),
//! `Cache-Control: no-store`, per-user rate limited). Card ids in a path are the provider
//! **external** id, resolved to the internal `cards.id` before storage (so a deck card
//! survives a catalog re-import).

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};

use crate::entities::prelude::{Deck, DeckFolder, DeckSection};
use crate::entities::{deck, deck_folder, deck_section};
use crate::error::AppError;
use crate::handlers::shared::{CardResponse, CollectionSummary};
// The name/optional-text validators now live in `handlers::shared::validate` (the
// life-counter tool wants the same two rules); re-exported here so the deck submodules
// keep importing them from `super` as before.
pub(crate) use crate::handlers::shared::{validate_name, validate_optional};
use crate::state::AppState;

use sea_orm::prelude::DateTimeUtc;

mod analysis;
mod cards;
mod copy;
mod export;
mod folders;
mod import;
mod needed;
mod read;
mod sections;
mod write;

pub use analysis::{deck_goldfish, deck_legality, deck_stats, list_deck_formats};
pub use cards::{change_deck_card_printing, move_deck_card, set_deck_card};
pub use copy::copy_public_deck;
pub use export::export_deck;
pub use folders::{create_folder, delete_folder, list_folders, update_folder};
pub use import::{MAX_DECK_UPLOAD_BYTES, import_deck};
pub use needed::needed_cards;
pub use read::{get_deck, list_decks};
pub use sections::{create_section, delete_section, reorder_sections, update_section};
pub use write::{create_deck, delete_deck, move_deck_to_folder, set_deck_visibility, update_deck};

// The `#[utoipa::path]`-generated route metadata structs, re-exported so
// `crate::openapi::ApiDoc` can name them at `crate::handlers::decks::__path_<fn>`.
pub use analysis::{
    __path_deck_goldfish, __path_deck_legality, __path_deck_stats, __path_list_deck_formats,
};
pub use cards::{__path_change_deck_card_printing, __path_move_deck_card, __path_set_deck_card};
pub use copy::__path_copy_public_deck;
pub use export::__path_export_deck;
pub use folders::{
    __path_create_folder, __path_delete_folder, __path_list_folders, __path_update_folder,
};
pub use import::__path_import_deck;
pub use needed::__path_needed_cards;
pub use read::{__path_get_deck, __path_list_decks};
pub use sections::{
    __path_create_section, __path_delete_section, __path_reorder_sections, __path_update_section,
};
pub use write::{
    __path_create_deck, __path_delete_deck, __path_move_deck_to_folder, __path_set_deck_visibility,
    __path_update_deck,
};

// The analysis entry points + loaders, reused by the public-sharing mirrors
// (`crate::handlers::sharing::decks`) so a shared deck's analysis is the identical
// computation its owner sees.
pub(crate) use analysis::{
    DeckAnalytics, DeckLegality, GoldfishHand, GoldfishParams, StatsParams, analyse_goldfish,
    analyse_legality, analyse_stats, load_analysis, load_analysis_with_cards,
};

// The `deck_id`-parameterised detail core, reused by the public sharing handler
// (`crate::handlers::sharing::decks`) so a public deck read shares the exact query/shaping.
pub(crate) use read::deck_detail;

// ---------- Limits + defaults ----------

/// The default sections seeded into a new deck (Archidekt-flavoured): the common
/// type buckets first (so a client can auto-file a new card by its type), then the
/// functional categories a user sorts cards into by hand, then `Maybeboard` — the one
/// default seeded with `is_maybeboard` set (issue #570), so a fresh deck's card count
/// and analytics ignore it out of the box.
pub(crate) const DEFAULT_SECTIONS: &[(&str, bool)] = &[
    ("Commander", false),
    ("Creatures", false),
    ("Artifacts", false),
    ("Enchantments", false),
    ("Instants", false),
    ("Sorceries", false),
    ("Planeswalkers", false),
    ("Lands", false),
    ("Ramp", false),
    ("Card Draw", false),
    ("Removal", false),
    ("Counters", false),
    ("Protection", false),
    ("Recursion", false),
    ("Tutor", false),
    ("Sac Outlet", false),
    ("Discard", false),
    ("Mill", false),
    (MAYBEBOARD_SECTION, true),
];

/// The name the seeded maybeboard section carries — and the one the deck importer maps
/// provider maybeboard / "considering" boards onto ([`crate::deck_import::parser`]), so
/// the two agree on which imported section gets the flag.
pub(crate) const MAYBEBOARD_SECTION: &str = "Maybeboard";

/// Whether a section *name* means "outside the deck proper" — the spellings an import or a
/// hand-typed section header can arrive as. This is a seeding rule only: it decides
/// `is_maybeboard` when a section is first created from an untyped name (deck import, and
/// migration 62's backfill of pre-flag decks). Afterwards the **column** is the source of
/// truth, so renaming a maybeboard doesn't quietly fold it back into the deck and naming a
/// normal section "Considering" doesn't quietly remove it.
pub(crate) fn is_maybeboard_section_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "maybeboard" | "maybe board" | "considering"
    )
}

/// Generous per-`(user, game)` deck cap — far above any real user, but bounded so the
/// list stays cheap and a single account can't create unbounded rows.
const MAX_DECKS_PER_GAME: u64 = 1_000;
/// Cap on sections in one deck (defaults seed ~19; users add custom ones).
const MAX_SECTIONS_PER_DECK: u64 = 200;
/// Cap on deck folders per `(user, game)`.
const MAX_FOLDERS_PER_GAME: u64 = 500;

const MAX_DECK_NAME: usize = 200;
const MAX_DECK_DESCRIPTION: usize = 4_000;
const MAX_FORMAT: usize = 50;
const MAX_SECTION_NAME: usize = 100;
const MAX_FOLDER_NAME: usize = 100;

// ---------- Response DTOs ----------

/// A deck header, for the deck list. `card_count` is the total copies (regular + foil)
/// across every section — computed with one grouped aggregate, so the list stays cheap.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "Deck"))]
pub struct DeckResponse {
    pub id: i32,
    /// Game slug — carried so the (cross-game) public deck list can build per-deck links;
    /// redundant but harmless on the per-game authed list.
    pub game: String,
    pub name: String,
    pub description: Option<String>,
    pub format: Option<String>,
    /// The folder this deck is filed under, or null when loose.
    pub folder_id: Option<i32>,
    pub is_public: bool,
    /// Total copies (regular + foil) across all sections.
    pub card_count: i64,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTimeUtc,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: DateTimeUtc,
}

impl DeckResponse {
    pub(crate) fn from_model(d: &deck::Model, card_count: i64) -> Self {
        Self {
            id: d.id,
            game: d.game.clone(),
            name: d.name.clone(),
            description: d.description.clone(),
            format: d.format.clone(),
            folder_id: d.folder_id,
            is_public: d.is_public,
            card_count,
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

/// A deck folder (organises decks), with how many decks are filed under it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "DeckFolder"))]
pub struct DeckFolderResponse {
    pub id: i32,
    pub name: String,
    pub deck_count: i64,
}

/// One section (category) of a deck, in display order.
#[derive(Clone, Debug, Serialize, PartialEq, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "DeckSection"))]
pub struct DeckSectionResponse {
    pub id: i32,
    pub name: String,
    pub position: i32,
    /// Whether this section sits outside the deck proper — its cards are excluded from
    /// `summary`, legality, analytics, and the needed list (issue #570).
    pub is_maybeboard: bool,
}

impl From<deck_section::Model> for DeckSectionResponse {
    fn from(s: deck_section::Model) -> Self {
        Self {
            id: s.id,
            name: s.name,
            position: s.position,
            is_maybeboard: s.is_maybeboard,
        }
    }
}

/// One card in a deck: the full public card payload plus which section it sits in and
/// how many copies. Deck-specific (it carries `section_id`), so a distinct DTO rather
/// than the shared `CollectionEntry`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckCardEntry {
    pub card: CardResponse,
    pub section_id: i32,
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// The full single-deck view: metadata, the owner handle (for the share URL / author
/// link — null until a username is set), the aggregate value summary, every section in
/// order, and every card. A deck is bounded, so this is returned whole (no pagination);
/// the SPA groups `cards` by `section_id`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckDetail {
    pub id: i32,
    /// Game slug — carried so the (game-agnostic) public deck URL can render its cards.
    pub game: String,
    pub name: String,
    pub description: Option<String>,
    pub format: Option<String>,
    pub folder_id: Option<i32>,
    pub is_public: bool,
    /// The owner's public handle (`alice-0001`), or null until they set a username.
    pub handle: Option<String>,
    /// Value / copy aggregates over the deck **proper** — every card outside a
    /// maybeboard section (issue #570). Reuses the shared summary shape; the
    /// `bulk_value_usd` field is unused by the deck UI.
    pub summary: CollectionSummary,
    /// The same aggregates over the maybeboard sections alone, so the UI can show what's
    /// being considered without it inflating the deck's own totals. All-zero when the
    /// deck has no maybeboard cards.
    pub maybeboard_summary: CollectionSummary,
    pub sections: Vec<DeckSectionResponse>,
    /// Every card in the deck, maybeboard included — each entry's `section_id` says
    /// which side of the line it falls on.
    pub cards: Vec<DeckCardEntry>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTimeUtc,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: DateTimeUtc,
}

/// The current sharing state of a deck: whether it's public plus the owner's handle
/// (null until a username is set), for the share-URL control.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckVisibility {
    pub public: bool,
    pub handle: Option<String>,
}

// ---------- Request DTOs ----------

/// Body of `POST /api/decks/{game}`: create a deck. `folder_id`, when present, must be
/// one of the caller's folders for the game.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CreateDeckRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub folder_id: Option<i32>,
}

/// Body of `PUT /api/decks/{game}/{deck_id}`: replace the deck's editable metadata
/// (name is required; description/format are optional, blank = cleared). Folder and
/// sharing are their own endpoints.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct UpdateDeckRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
}

/// Body of `PUT /api/decks/{game}/{deck_id}/folder`: move the deck to a folder, or
/// `null` to loosen it. A non-null id must be one of the caller's folders.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct MoveDeckFolderRequest {
    pub folder_id: Option<i32>,
}

/// Body of `PUT /api/decks/{game}/{deck_id}/visibility`: enable/disable public sharing.
/// Enabling requires a username first (a public deck is addressed by handle) — else `409`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SetDeckVisibilityRequest {
    pub public: bool,
}

/// Body of `POST/PUT` on a folder: its name.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct FolderNameRequest {
    pub name: String,
}

/// Body of `POST /api/decks/{game}/{deck_id}/sections`: create a custom section,
/// optionally as a maybeboard (defaults to part of the deck).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CreateSectionRequest {
    pub name: String,
    #[serde(default)]
    pub is_maybeboard: bool,
}

/// Body of `PUT /api/decks/{game}/{deck_id}/sections/{section_id}`: rename, reposition,
/// and/or flip the maybeboard flag (each field optional — absent leaves it unchanged).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct UpdateSectionRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub position: Option<i32>,
    #[serde(default)]
    pub is_maybeboard: Option<bool>,
}

/// Body of `PUT /api/decks/{game}/{deck_id}/sections/reorder`: the section ids in the
/// desired display order (must be exactly the deck's sections).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ReorderSectionsRequest {
    pub section_ids: Vec<i32>,
}

/// Body of `PUT /api/decks/{game}/{deck_id}/cards/{id}`: set the absolute counts for a
/// card in one section (both zero removes it from that section). `section_id` must be
/// one of the deck's sections.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SetDeckCardRequest {
    pub quantity: i32,
    pub foil_quantity: i32,
    pub section_id: i32,
}

/// Body of `PUT /api/decks/{game}/{deck_id}/cards/{id}/move`: move a card from one of the
/// deck's sections to another (merging counts if the target already holds the card).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct MoveDeckCardRequest {
    pub from_section_id: i32,
    pub to_section_id: i32,
}

/// Body of `PUT /api/decks/{game}/{deck_id}/cards/{id}/printing`: replace one card
/// printing in a section with another printing of the same gameplay card. Counts and
/// finish buckets are preserved (or merged if the target printing is already present).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ChangeDeckCardPrintingRequest {
    pub new_card_id: String,
    pub section_id: i32,
}

/// Body of `POST /api/decks/{game}/import`. Exactly one of `source` (a provider deck
/// URL/id) or `contents` (an uploaded file read as text) must be present. Uploaded files
/// also carry their `format` and an optional deck `name`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckImportRequest {
    pub provider: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub contents: Option<String>,
    #[serde(default)]
    pub format: Option<crate::deck_import::DeckImportFileFormat>,
    #[serde(default)]
    pub name: Option<String>,
    /// When true (the default), cards in a generic `Mainboard` section are filed into
    /// the matching preset type section. Explicit provider categories are preserved.
    #[serde(default = "default_auto_categorize")]
    pub auto_categorize: bool,
}

const fn default_auto_categorize() -> bool {
    true
}

/// How [`needed_cards`] matches a wanted card against the collection (issue #499).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum NeedMode {
    /// Aggregate by gameplay identity: any printing you own satisfies any printing a deck
    /// wants (the "one Command Tower covers a Command Tower" default).
    #[default]
    Card,
    /// Match a deck's exact printing against that same printing in the collection, so the
    /// list names the precise printing you're short of.
    Printing,
}

/// Query for `GET /api/decks/{game}/needed`: the matching mode (defaults to
/// [`NeedMode::Card`]).
#[derive(Debug, Default, Deserialize)]
pub(crate) struct NeededParams {
    #[serde(default)]
    pub mode: NeedMode,
}

/// Result of a deck import: a lightweight header for the newly created deck plus match
/// feedback for rows skipped because their printing/name was absent from the catalog.
/// Full sections/cards are loaded separately from the normal deck-detail endpoint.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckImportResponse {
    pub deck: DeckResponse,
    pub provider: String,
    pub total_rows: usize,
    pub matched_cards: usize,
    pub unmatched_cards: usize,
    pub unmatched_sample: Vec<String>,
}

/// One of the caller's decks that wants a [`NeededCard`], for the "which decks does this
/// affect" breakdown (issue #499).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct NeededCardDeck {
    pub id: i32,
    pub name: String,
}

/// A card the caller's decks collectively want more copies of than their collection holds
/// — the deck "shopping list" (issue #499). `needed` is the shortfall, always positive
/// (fully-covered cards are omitted). In [`NeedMode::Card`] the counts aggregate every
/// printing of the gameplay card and `card` is a representative printing the decks use; in
/// [`NeedMode::Printing`] they're for one exact printing and `card` is that printing.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct NeededCard {
    pub card: CardResponse,
    /// Copies still to acquire: `max(0, required - owned)`, always &gt; 0.
    pub needed: i64,
    /// Total copies (regular + foil) the caller's decks want, summed across decks/sections.
    pub required: i64,
    /// Copies owned in the caller's collection — any printing of the card in `card` mode,
    /// this exact printing in `printing` mode.
    pub owned: i64,
    /// The caller's decks that want this card, by name.
    pub decks: Vec<NeededCardDeck>,
}

// ---------- Shared helpers ----------

/// Load a deck by id, proving it belongs to `user_id` for `game`. A deck that doesn't
/// exist, belongs to another user, or is for another game is a **404** (never 403), so
/// the surface is not an existence oracle over deck ids.
pub(crate) async fn load_deck(
    state: &AppState,
    user_id: i32,
    game: &str,
    deck_id: i32,
) -> Result<deck::Model, AppError> {
    Deck::find_by_id(deck_id)
        .filter(deck::Column::UserId.eq(user_id))
        .filter(deck::Column::Game.eq(game))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("deck not found".to_string()))
}

/// Load a section by id, proving it belongs to `deck_id`. A section that doesn't exist or
/// belongs to another deck is a **404**.
pub(crate) async fn load_section(
    state: &AppState,
    deck_id: i32,
    section_id: i32,
) -> Result<deck_section::Model, AppError> {
    DeckSection::find_by_id(section_id)
        .filter(deck_section::Column::DeckId.eq(deck_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("section not found".to_string()))
}

/// Resolve a folder reference on a deck body: `None` stays `None`; a `Some(id)` must be
/// one of the caller's folders for the game (else 404). Returns the validated id.
pub(crate) async fn resolve_folder_ref(
    state: &AppState,
    user_id: i32,
    game: &str,
    folder_id: Option<i32>,
) -> Result<Option<i32>, AppError> {
    let Some(id) = folder_id else {
        return Ok(None);
    };
    let exists = DeckFolder::find_by_id(id)
        .filter(deck_folder::Column::UserId.eq(user_id))
        .filter(deck_folder::Column::Game.eq(game))
        .one(&state.db)
        .await?
        .is_some();
    if !exists {
        return Err(AppError::NotFound("folder not found".to_string()));
    }
    Ok(Some(id))
}

/// Bump a deck's `updated_at` so an edit to its cards or sections bubbles it to the top of
/// the recency-sorted deck list. One cheap indexed UPDATE (the caller has already proved
/// ownership via [`load_deck`]).
pub(crate) async fn touch_deck<C: sea_orm::ConnectionTrait>(
    db: &C,
    deck_id: i32,
    now: DateTimeUtc,
) -> Result<(), AppError> {
    use sea_orm::sea_query::Expr;
    Deck::update_many()
        .col_expr(deck::Column::UpdatedAt, Expr::value(now))
        .filter(deck::Column::Id.eq(deck_id))
        .exec(db)
        .await?;
    Ok(())
}

/// Total copies (regular + foil) held across a set of decks, keyed by deck id — one
/// grouped aggregate so the deck list doesn't fetch every card. Decks with no cards are
/// simply absent (the caller defaults them to `0`).
///
/// **Inner-joins `cards`** so a holding whose catalog row is gone (a re-import) is skipped —
/// matching `deck_detail`'s LEFT-join-then-skip fold, so the list `card_count` and the
/// detail `summary.total_cards` agree for the same deck. For the same reason it also
/// **excludes maybeboard sections** (issue #570): the detail summary counts the deck
/// proper, and a list card count that disagreed with the deck page's own header would
/// read as a bug.
pub(crate) async fn card_counts_by_deck(
    db: &sea_orm::DatabaseConnection,
    deck_ids: &[i32],
) -> Result<std::collections::HashMap<i32, i64>, AppError> {
    use crate::entities::deck_card;
    use crate::entities::prelude::{Card, DeckCard};
    use sea_orm::sea_query::Expr;
    use sea_orm::{QueryOrder, QuerySelect};

    if deck_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows: Vec<(i32, i64)> = DeckCard::find()
        .select_only()
        .column(deck_card::Column::DeckId)
        .column_as(
            Expr::cust("SUM(deck_cards.quantity + deck_cards.foil_quantity)"),
            "copies",
        )
        .inner_join(Card)
        .filter(deck_card::Column::DeckId.is_in(deck_ids.iter().copied()))
        .filter(
            deck_card::Column::SectionId.not_in_subquery(maybeboard_section_ids(deck_ids.to_vec())),
        )
        .group_by(deck_card::Column::DeckId)
        .order_by_asc(deck_card::Column::DeckId)
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows.into_iter().collect())
}

/// Sub-select of the maybeboard `deck_sections.id` belonging to `deck_ids` — the seam the
/// deck-proper scans (`card_counts_by_deck`, the needed list) filter against, so "outside
/// the deck" is expressed once in SQL rather than re-derived per caller. Built through the
/// SeaORM query API (parameterised, dialect-neutral) like every other query here.
pub(crate) fn maybeboard_section_ids(deck_ids: Vec<i32>) -> sea_orm::sea_query::SelectStatement {
    use sea_orm::{QuerySelect, QueryTrait};
    DeckSection::find()
        .select_only()
        .column(deck_section::Column::Id)
        .filter(deck_section::Column::DeckId.is_in(deck_ids))
        .filter(deck_section::Column::IsMaybeboard.eq(true))
        .into_query()
}

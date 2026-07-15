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
//! Whole-deck provider import/export is a sibling pipeline: provider categories/boards
//! become exact sections and the new deck is inserted atomically, without collection
//! reconciliation.
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
use crate::state::AppState;

use sea_orm::prelude::DateTimeUtc;

mod cards;
mod export;
mod folders;
mod import;
mod read;
mod sections;
mod write;

pub use cards::{move_deck_card, set_deck_card};
pub use export::export_deck;
pub use folders::{create_folder, delete_folder, list_folders, update_folder};
pub use import::{MAX_DECK_UPLOAD_BYTES, import_deck};
pub use read::{get_deck, list_decks};
pub use sections::{create_section, delete_section, reorder_sections, update_section};
pub use write::{create_deck, delete_deck, move_deck_to_folder, set_deck_visibility, update_deck};

// The `#[utoipa::path]`-generated route metadata structs, re-exported so
// `crate::openapi::ApiDoc` can name them at `crate::handlers::decks::__path_<fn>`.
pub use cards::{__path_move_deck_card, __path_set_deck_card};
pub use folders::{
    __path_create_folder, __path_delete_folder, __path_list_folders, __path_update_folder,
};
pub use export::__path_export_deck;
pub use import::__path_import_deck;
pub use read::{__path_get_deck, __path_list_decks};
pub use sections::{
    __path_create_section, __path_delete_section, __path_reorder_sections, __path_update_section,
};
pub use write::{
    __path_create_deck, __path_delete_deck, __path_move_deck_to_folder,
    __path_set_deck_visibility, __path_update_deck,
};

// The `deck_id`-parameterised detail core, reused by the public sharing handler
// (`crate::handlers::sharing::decks`) so a public deck read shares the exact query/shaping.
pub(crate) use read::deck_detail;

// ---------- Limits + defaults ----------

/// The default sections seeded into a new deck (Archidekt-flavoured): the common
/// type buckets first (so a client can auto-file a new card by its type), then the
/// functional categories a user sorts cards into by hand, then `Maybeboard`.
pub(crate) const DEFAULT_SECTIONS: &[&str] = &[
    "Commander",
    "Creatures",
    "Artifacts",
    "Enchantments",
    "Instants",
    "Sorceries",
    "Planeswalkers",
    "Lands",
    "Ramp",
    "Card Draw",
    "Removal",
    "Counters",
    "Protection",
    "Recursion",
    "Tutor",
    "Sac Outlet",
    "Discard",
    "Mill",
    "Maybeboard",
];

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
#[derive(Debug, Serialize, PartialEq, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "DeckSection"))]
pub struct DeckSectionResponse {
    pub id: i32,
    pub name: String,
    pub position: i32,
}

impl From<deck_section::Model> for DeckSectionResponse {
    fn from(s: deck_section::Model) -> Self {
        Self {
            id: s.id,
            name: s.name,
            position: s.position,
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
    /// Value / copy aggregates over the deck's cards (reuses the shared summary shape;
    /// the `bulk_value_usd` field is unused by the deck UI).
    pub summary: CollectionSummary,
    pub sections: Vec<DeckSectionResponse>,
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

/// Body of `POST /api/decks/{game}/{deck_id}/sections`: create a custom section.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CreateSectionRequest {
    pub name: String,
}

/// Body of `PUT /api/decks/{game}/{deck_id}/sections/{section_id}`: rename and/or
/// reposition a section (each field optional — absent leaves it unchanged).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct UpdateSectionRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub position: Option<i32>,
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
}

/// Result of a deck import: the newly created deck plus match feedback for rows skipped
/// because their printing/name was absent from the local catalog.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckImportResponse {
    pub deck: DeckDetail,
    pub provider: String,
    pub total_rows: usize,
    pub matched_cards: usize,
    pub unmatched_cards: usize,
    pub unmatched_sample: Vec<String>,
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

/// Trim + validate a required name field (non-empty, at most `max` characters).
pub(crate) fn validate_name(value: &str, field: &str, max: usize) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation(format!("{field} must not be empty")));
    }
    if trimmed.chars().count() > max {
        return Err(AppError::Validation(format!(
            "{field} must be at most {max} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Trim + validate an optional text field: blank collapses to `None`; over `max`
/// characters is a 422.
pub(crate) fn validate_optional(
    value: Option<String>,
    field: &str,
    max: usize,
) -> Result<Option<String>, AppError> {
    match value {
        Some(v) => {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            if trimmed.chars().count() > max {
                return Err(AppError::Validation(format!(
                    "{field} must be at most {max} characters"
                )));
            }
            Ok(Some(trimmed.to_string()))
        }
        None => Ok(None),
    }
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
/// detail `summary.total_cards` agree for the same deck.
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
        .group_by(deck_card::Column::DeckId)
        .order_by_asc(deck_card::Column::DeckId)
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows.into_iter().collect())
}

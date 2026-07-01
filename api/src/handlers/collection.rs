//! Authenticated, per-user card-collection endpoints.
//!
//! A collection records how many copies of each card a signed-in user owns, per
//! game (`/api/collection/{game}/...`). Every route requires a valid access token
//! (via [`AuthUser`]) and is wired into the router's `private` group, so responses
//! are `Cache-Control: no-store` — per-user data must never be shared-cached.
//!
//! Card ids in the path are the provider's **external** id (the same id the public
//! catalog exposes); each is resolved to the internal `cards.id` before storage,
//! so a holding survives a catalog re-import and the stored `card_id` matches
//! `card_price_history`. Ownership is always scoped by `user.id` from the token, so
//! one user can never read or mutate another's collection.

use std::collections::{HashMap, HashSet};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
    SqlErr,
};
use serde::{Deserialize, Serialize};

use crate::auth::extractor::AuthUser;
use crate::catalog::{self, Game};
use crate::collection_import::jobs::{self, JobStatus};
use crate::collection_import::{self, ImportSummary, Provider, ReconcileMode};
use crate::entities::prelude::{Card, CollectionItem, CollectionSource};
use crate::entities::{card, collection_item, collection_source};
use crate::error::AppError;
use crate::extract::JsonBody;
use crate::handlers::catalog::CardResponse;
use crate::state::AppState;

const DEFAULT_PAGE_SIZE: u64 = 60;
const MAX_PAGE_SIZE: u64 = 200;
/// A generous per-card holding cap: far above any real collection, but bounded so a
/// single count can't overflow the valuation arithmetic or be abused to store a
/// pathological value.
const MAX_QUANTITY: i32 = 1_000_000;
/// Cap on how many card ids one batch owned-counts lookup may request. A browse page
/// shows at most a few hundred cards, so this bounds the two `IN (...)` queries well
/// above any real page while staying under SQLite's bound-variable limit and refusing
/// an abusive request.
const MAX_OWNED_IDS: usize = 500;
/// Batch size for `IN (...)` card lookups, kept under SQLite's per-statement
/// bind-variable limit (as few as 999 on old builds) — a collection can hold far more
/// distinct cards than that once imported in bulk.
const CARD_LOOKUP_CHUNK: usize = 900;

// ---------- Response / request DTOs ----------

/// One owned card: the full public card payload plus how many copies are owned.
#[derive(Debug, Serialize)]
pub struct CollectionEntry {
    pub card: CardResponse,
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// Just the owned counts for one card — what the card-detail controls read and write.
#[derive(Debug, Serialize)]
pub struct CollectionQuantities {
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// Batch owned-counts response: external card id -> owned counts, for owned cards
/// only. Cards the user doesn't own are simply absent (never a zero entry), so a page
/// with nothing owned serialises to `{ "data": {} }`.
#[derive(Debug, Serialize)]
pub struct OwnedCountsResponse {
    pub data: HashMap<String, CollectionQuantities>,
}

/// Aggregate stats for a user's per-game collection (the collection landing header).
#[derive(Debug, Serialize)]
pub struct CollectionSummary {
    /// Distinct cards owned (one per collection row).
    pub unique_cards: i64,
    /// Total copies owned (regular + foil) across every card.
    pub total_cards: i64,
    /// Estimated USD value: regular copies at the card's `usd`, foil copies at
    /// `usd_foil`, as a 2-dp decimal string. `null` when nothing owned is priced.
    pub total_value_usd: Option<String>,
}

/// A page of results plus the cursor metadata the SPA paginates with (mirrors the
/// catalog's page shape, kept local so the two modules stay decoupled).
#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub data: Vec<T>,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
    pub has_more: bool,
}

/// Body of `PUT .../cards/{id}`: the desired absolute counts (not a delta). Setting
/// both to zero removes the card from the collection.
#[derive(Debug, Deserialize)]
pub struct SetQuantitiesRequest {
    pub quantity: i32,
    pub foil_quantity: i32,
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

/// Body of `POST .../owned`: the external card ids to look up owned counts for. Sent
/// as a POST body rather than a GET query so a browse page's (potentially few-hundred)
/// id list can't blow the request-line length behind a proxy.
#[derive(Debug, Deserialize)]
pub struct OwnedCountsRequest {
    pub ids: Vec<String>,
}

impl ListParams {
    /// Resolve the requested 1-based page and clamp the page size to `[1, MAX]`.
    fn page_and_size(&self) -> (u64, u64) {
        let page = self.page.unwrap_or(1).max(1);
        let page_size = self
            .page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE);
        (page, page_size)
    }
}

// ---------- Handlers ----------

/// `GET /api/collection/{game}` -> the signed-in user's owned cards for a game,
/// most-recently-updated first, paginated. Each entry carries the full card payload
/// plus the owned counts.
pub async fn list_collection(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionEntry>>, AppError> {
    require_game(&game)?;
    let (page, page_size) = params.page_and_size();

    let paginator = CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user.id))
        .filter(collection_item::Column::Game.eq(game.as_str()))
        // Newest change first, with a stable id tiebreaker for deterministic paging.
        .order_by_desc(collection_item::Column::UpdatedAt)
        .order_by_desc(collection_item::Column::Id)
        .paginate(&state.db, page_size);

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;
    if rows.is_empty() {
        return Ok(Json(build_page(Vec::new(), page, page_size, total)));
    }

    // Load the referenced cards in one query, then assemble entries in row order.
    let card_ids: Vec<i32> = rows.iter().map(|r| r.card_id).collect();
    let mut by_id: HashMap<i32, card::Model> = Card::find()
        .filter(card::Column::Id.is_in(card_ids))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|c| (c.id, c))
        .collect();

    let data: Vec<CollectionEntry> = rows
        .into_iter()
        .filter_map(|r| {
            by_id.remove(&r.card_id).map(|c| CollectionEntry {
                card: CardResponse::from(c),
                quantity: r.quantity,
                foil_quantity: r.foil_quantity,
            })
        })
        .collect();

    Ok(Json(build_page(data, page, page_size, total)))
}

/// `GET /api/collection/{game}/summary` -> aggregate stats (distinct cards, total
/// copies, estimated USD value) for the signed-in user's collection in a game.
pub async fn collection_summary(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<CollectionSummary>, AppError> {
    require_game(&game)?;

    // A collection is bounded by how many distinct cards a user owns, so we load the
    // rows and their cards and total copies + value in Rust (never trusting the
    // stored decimal price strings to SQL arithmetic).
    let rows = CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user.id))
        .filter(collection_item::Column::Game.eq(game.as_str()))
        .all(&state.db)
        .await?;
    if rows.is_empty() {
        return Ok(Json(CollectionSummary {
            unique_cards: 0,
            total_cards: 0,
            total_value_usd: None,
        }));
    }

    let unique_cards = rows.len() as i64;
    let total_cards: i64 = rows
        .iter()
        .map(|r| i64::from(r.quantity) + i64::from(r.foil_quantity))
        .sum();

    // Load the owned cards' prices, chunked so a large collection (now reachable via
    // import) can't exceed SQLite's per-statement bind-variable limit.
    let card_ids: Vec<i32> = rows.iter().map(|r| r.card_id).collect();
    let mut by_id: HashMap<i32, card::Model> = HashMap::new();
    for chunk in card_ids.chunks(CARD_LOOKUP_CHUNK) {
        let cards = Card::find()
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .all(&state.db)
            .await?;
        for c in cards {
            by_id.insert(c.id, c);
        }
    }

    let mut total_cents: i128 = 0;
    let mut any_priced = false;
    for r in &rows {
        let Some(card) = by_id.get(&r.card_id) else {
            continue;
        };
        if let Some(cents) = price_cents(card.price_usd.as_deref()) {
            total_cents += cents * i128::from(r.quantity);
            any_priced = true;
        }
        if let Some(cents) = price_cents(card.price_usd_foil.as_deref()) {
            total_cents += cents * i128::from(r.foil_quantity);
            any_priced = true;
        }
    }

    Ok(Json(CollectionSummary {
        unique_cards,
        total_cards,
        total_value_usd: any_priced.then(|| format_cents(total_cents)),
    }))
}

/// `GET /api/collection/{game}/cards/{id}` -> how many copies of one card the user
/// owns (zeros when the card isn't in their collection). `id` is the external card
/// id; a `404` means the game or card is unknown.
pub async fn get_collection_entry(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<CollectionQuantities>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;
    let row = find_row(&state, user.id, &game, card.id).await?;
    Ok(Json(match row {
        Some(r) => CollectionQuantities {
            quantity: r.quantity,
            foil_quantity: r.foil_quantity,
        },
        None => CollectionQuantities {
            quantity: 0,
            foil_quantity: 0,
        },
    }))
}

/// `POST /api/collection/{game}/owned` -> the owned counts for the subset of the
/// given external card ids that the signed-in user actually owns, keyed by external
/// id. Cards the user doesn't own are absent from the map (so an all-unowned page
/// returns `{ "data": {} }`). This backs the owned-count badges overlaid on the
/// public browse grids without an N+1 of per-card lookups. `422` if more than
/// [`MAX_OWNED_IDS`] ids are requested at once.
pub async fn owned_counts(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<OwnedCountsRequest>,
) -> Result<Json<OwnedCountsResponse>, AppError> {
    require_game(&game)?;

    let external_ids = dedupe_ids(payload.ids);
    if external_ids.is_empty() {
        return Ok(Json(OwnedCountsResponse {
            data: HashMap::new(),
        }));
    }
    if external_ids.len() > MAX_OWNED_IDS {
        return Err(AppError::Validation(format!(
            "at most {MAX_OWNED_IDS} card ids may be looked up at once"
        )));
    }

    // Resolve external -> internal ids for this game, keeping the reverse map so the
    // response can be keyed by the external id the client sent. Unknown ids just don't
    // appear here (and so never in the result).
    let external_by_internal: HashMap<i32, String> = Card::find()
        .filter(card::Column::Game.eq(game.as_str()))
        .filter(card::Column::ExternalId.is_in(external_ids))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|c| (c.id, c.external_id))
        .collect();
    if external_by_internal.is_empty() {
        return Ok(Json(OwnedCountsResponse {
            data: HashMap::new(),
        }));
    }

    // One query for the user's holdings among those cards; a card with no row is
    // simply not owned and contributes nothing to the map.
    let internal_ids: Vec<i32> = external_by_internal.keys().copied().collect();
    let rows = CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user.id))
        .filter(collection_item::Column::Game.eq(game.as_str()))
        .filter(collection_item::Column::CardId.is_in(internal_ids))
        .all(&state.db)
        .await?;

    let data: HashMap<String, CollectionQuantities> = rows
        .into_iter()
        .filter_map(|r| {
            external_by_internal.get(&r.card_id).map(|external_id| {
                (
                    external_id.clone(),
                    CollectionQuantities {
                        quantity: r.quantity,
                        foil_quantity: r.foil_quantity,
                    },
                )
            })
        })
        .collect();

    Ok(Json(OwnedCountsResponse { data }))
}

/// `PUT /api/collection/{game}/cards/{id}` -> set the owned counts for one card
/// (absolute values, not a delta). Both zero removes the card from the collection.
/// Returns the resulting counts. `404` for an unknown game/card, `422` for a
/// negative or oversized count.
pub async fn set_collection_entry(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, id)): Path<(String, String)>,
    JsonBody(payload): JsonBody<SetQuantitiesRequest>,
) -> Result<Json<CollectionQuantities>, AppError> {
    require_game(&game)?;
    let quantity = validate_quantity(payload.quantity, "quantity")?;
    let foil_quantity = validate_quantity(payload.foil_quantity, "foil_quantity")?;
    let card = load_card(&state, &game, &id).await?;

    let existing = find_row(&state, user.id, &game, card.id).await?;
    let now = Utc::now();

    // Owning zero of both is "not in the collection": drop the row if present.
    if quantity == 0 && foil_quantity == 0 {
        if let Some(row) = existing {
            CollectionItem::delete_by_id(row.id)
                .exec(&state.db)
                .await?;
        }
        return Ok(Json(CollectionQuantities {
            quantity: 0,
            foil_quantity: 0,
        }));
    }

    match existing {
        Some(row) => {
            let mut active: collection_item::ActiveModel = row.into();
            active.quantity = Set(quantity);
            active.foil_quantity = Set(foil_quantity);
            active.updated_at = Set(now);
            active.update(&state.db).await?;
        }
        None => {
            let active = collection_item::ActiveModel {
                user_id: Set(user.id),
                game: Set(game.clone()),
                card_id: Set(card.id),
                quantity: Set(quantity),
                foil_quantity: Set(foil_quantity),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            };
            // The unique (user, game, card) index is the real source of truth: two
            // concurrent first-adds can both see `None`, so a unique violation means
            // we lost the race — fall back to updating the row that won.
            if let Err(err) = active.insert(&state.db).await {
                if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
                    if let Some(row) = find_row(&state, user.id, &game, card.id).await? {
                        let mut active: collection_item::ActiveModel = row.into();
                        active.quantity = Set(quantity);
                        active.foil_quantity = Set(foil_quantity);
                        active.updated_at = Set(now);
                        active.update(&state.db).await?;
                    }
                } else {
                    return Err(err.into());
                }
            }
        }
    }

    Ok(Json(CollectionQuantities {
        quantity,
        foil_quantity,
    }))
}

// ---------- Import / sync from an external collection provider ----------

/// Body of `POST .../import`: which provider, the source URL/id, and how to reconcile.
#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub provider: String,
    pub source: String,
    pub mode: ReconcileMode,
}

/// Body of `PUT .../source`: the collection link to remember (provider + source URL/id).
#[derive(Debug, Deserialize)]
pub struct SaveSourceRequest {
    pub provider: String,
    pub source: String,
}

/// A saved external collection link for a game.
#[derive(Debug, Serialize)]
pub struct CollectionSourceResponse {
    pub provider: &'static str,
    pub external_id: String,
    /// A canonical, user-facing URL for the collection on the provider.
    pub url: String,
    /// RFC3339 timestamp of the last successful sync, or null if never synced.
    pub last_synced_at: Option<String>,
}

/// The status of a background import/sync job — returned when one is enqueued and each
/// time the client polls. Imports run asynchronously (throttled by the provider rate
/// limit), so the client kicks one off and polls this until `complete`/`error`.
#[derive(Debug, Serialize)]
pub struct ImportJobResponse {
    pub job_id: u64,
    /// `queued` | `running` | `complete` | `error`.
    pub status: &'static str,
    /// The import summary, present only when `status == "complete"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ImportSummary>,
    /// A user-facing message, present only when `status == "error"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ImportJobResponse {
    fn from_status(job_id: u64, status: JobStatus) -> Self {
        let (status, summary, error) = match status {
            JobStatus::Queued => ("queued", None, None),
            JobStatus::Running => ("running", None, None),
            JobStatus::Complete(summary) => ("complete", Some(summary), None),
            JobStatus::Failed(message) => ("error", None, Some(message)),
        };
        Self {
            job_id,
            status,
            summary,
            error,
        }
    }
}

/// `POST /api/collection/{game}/import` -> enqueue a one-off import from a collection
/// provider using the chosen reconcile mode (does not save the link). Validates the
/// request synchronously, then returns `202` with a job id to poll; the fetch +
/// reconcile run in the background, throttled by the provider rate limit.
pub async fn import_collection(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<ImportRequest>,
) -> Result<(StatusCode, Json<ImportJobResponse>), AppError> {
    require_game(&game)?;
    let provider = parse_provider(&payload.provider)?;
    if !provider.supports_game(&game) {
        return Err(AppError::Validation(format!(
            "{} import is not available for '{}'",
            provider.label(),
            game
        )));
    }
    // Resolve the source id up front so a bad URL/id is an immediate 422, not a job that
    // fails later.
    let collection_id = collection_import::parse_source(provider, &payload.source)?;

    let job_id = jobs::spawn_import_job(
        state.db.clone(),
        state.http.clone(),
        state.imports.clone(),
        jobs::ImportRequest {
            user_id: user.id,
            game,
            provider,
            collection_id,
            mode: payload.mode,
            stamp_source_synced: false,
        },
    )?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ImportJobResponse::from_status(job_id, JobStatus::Queued)),
    ))
}

/// `GET /api/collection/{game}/import/jobs/{job_id}` -> the status of a background
/// import/sync job (queued / running / complete / error). `404` for an unknown job or
/// one that isn't the caller's.
pub async fn get_import_job(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, job_id)): Path<(String, u64)>,
) -> Result<Json<ImportJobResponse>, AppError> {
    require_game(&game)?;
    let status = state
        .imports
        .status(job_id, user.id, &game)
        .ok_or_else(|| AppError::NotFound("import job not found".to_string()))?;
    Ok(Json(ImportJobResponse::from_status(job_id, status)))
}

/// `GET /api/collection/{game}/source` -> the saved collection link for this game, or
/// `null` if none is saved.
pub async fn get_collection_source(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<Option<CollectionSourceResponse>>, AppError> {
    require_game(&game)?;
    let row = find_source(&state, user.id, &game).await?;
    Ok(Json(row.map(source_response)))
}

/// `PUT /api/collection/{game}/source` -> save (upsert) the collection link for this
/// game. Validates that the source resolves to a provider collection id; does not sync.
pub async fn save_collection_source(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<SaveSourceRequest>,
) -> Result<Json<CollectionSourceResponse>, AppError> {
    require_game(&game)?;
    let provider = parse_provider(&payload.provider)?;
    if !provider.supports_game(&game) {
        return Err(AppError::Validation(format!(
            "{} import is not available for '{}'",
            provider.label(),
            game
        )));
    }
    // Validate + normalise the source to a bare provider collection id.
    let external_id = collection_import::parse_source(provider, &payload.source)?;
    let now = Utc::now();

    // Pointing the link at a different collection resets the sync marker; re-saving the
    // same link preserves it. (Read the current link to decide, then upsert atomically.)
    let existing = find_source(&state, user.id, &game).await?;
    let changed = existing
        .as_ref()
        .is_none_or(|e| e.provider != provider.as_str() || e.external_id != external_id);
    let last_synced_at = if changed {
        None
    } else {
        existing.as_ref().and_then(|e| e.last_synced_at)
    };

    // Upsert on the unique (user, game) index so two concurrent first-time saves can't
    // 500 on a unique violation (matches the collection-item upsert's race-safety).
    let active = collection_source::ActiveModel {
        user_id: Set(user.id),
        game: Set(game.clone()),
        provider: Set(provider.as_str().to_string()),
        external_id: Set(external_id),
        last_synced_at: Set(last_synced_at),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    CollectionSource::insert(active)
        .on_conflict(
            OnConflict::columns([
                collection_source::Column::UserId,
                collection_source::Column::Game,
            ])
            .update_columns([
                collection_source::Column::Provider,
                collection_source::Column::ExternalId,
                collection_source::Column::LastSyncedAt,
                collection_source::Column::UpdatedAt,
            ])
            .to_owned(),
        )
        .exec(&state.db)
        .await?;

    // Read back the canonical row for the response.
    let saved = find_source(&state, user.id, &game)
        .await?
        .ok_or_else(|| AppError::Internal("saved collection source vanished".to_string()))?;
    Ok(Json(source_response(saved)))
}

/// `DELETE /api/collection/{game}/source` -> forget the saved collection link.
/// Idempotent: deleting when nothing is saved still returns `204`.
pub async fn delete_collection_source(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<StatusCode, AppError> {
    require_game(&game)?;
    if let Some(existing) = find_source(&state, user.id, &game).await? {
        CollectionSource::delete_by_id(existing.id)
            .exec(&state.db)
            .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/collection/{game}/sync` -> enqueue a re-sync from the saved collection
/// link using mirror/replace semantics; the worker stamps `last_synced_at` on success.
/// Returns `202` with a job id to poll. `404` when no link is saved.
pub async fn sync_collection_source(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<(StatusCode, Json<ImportJobResponse>), AppError> {
    require_game(&game)?;
    let source = find_source(&state, user.id, &game)
        .await?
        .ok_or_else(|| AppError::NotFound("no saved collection link to sync".to_string()))?;
    let provider = Provider::from_id(&source.provider).ok_or_else(|| {
        AppError::Internal(format!(
            "stored collection provider '{}' is unknown",
            source.provider
        ))
    })?;

    let job_id = jobs::spawn_import_job(
        state.db.clone(),
        state.http.clone(),
        state.imports.clone(),
        jobs::ImportRequest {
            user_id: user.id,
            game,
            provider,
            collection_id: source.external_id,
            // A saved re-sync always mirrors the source (the user opted into this when
            // saving the link; the UI warns that it replaces the collection).
            mode: ReconcileMode::Replace,
            // Stamp `last_synced_at` on success (this is a re-sync of the saved link).
            stamp_source_synced: true,
        },
    )?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ImportJobResponse::from_status(job_id, JobStatus::Queued)),
    ))
}

// ---------- Helpers ----------

fn require_game(game: &str) -> Result<&'static Game, AppError> {
    catalog::find(game).ok_or_else(|| AppError::NotFound(format!("unknown game '{game}'")))
}

/// Parse a collection-provider id from a request, 422 on an unknown provider.
fn parse_provider(s: &str) -> Result<Provider, AppError> {
    Provider::from_id(s)
        .ok_or_else(|| AppError::Validation(format!("unknown collection provider '{s}'")))
}

/// The user's saved collection link for a game, if any.
async fn find_source(
    state: &AppState,
    user_id: i32,
    game: &str,
) -> Result<Option<collection_source::Model>, AppError> {
    Ok(CollectionSource::find()
        .filter(collection_source::Column::UserId.eq(user_id))
        .filter(collection_source::Column::Game.eq(game))
        .one(&state.db)
        .await?)
}

/// Shape a stored source row for the API, resolving its provider to a canonical URL.
fn source_response(row: collection_source::Model) -> CollectionSourceResponse {
    // A stored provider id should always resolve; degrade to a URL-less response rather
    // than failing the read if a future/renamed provider id lingers in an old row.
    let (provider, url) = match Provider::from_id(&row.provider) {
        Some(p) => (p.as_str(), p.collection_url(&row.external_id)),
        None => ("unknown", String::new()),
    };
    CollectionSourceResponse {
        provider,
        external_id: row.external_id,
        url,
        last_synced_at: row.last_synced_at.map(|t| t.to_rfc3339()),
    }
}

/// Resolve a card by its external (provider) id within a game, 404 if unknown.
async fn load_card(state: &AppState, game: &str, external_id: &str) -> Result<card::Model, AppError> {
    Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::ExternalId.eq(external_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("card '{external_id}' not found")))
}

/// The user's collection row for a card, if any.
async fn find_row(
    state: &AppState,
    user_id: i32,
    game: &str,
    card_id: i32,
) -> Result<Option<collection_item::Model>, AppError> {
    Ok(CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game))
        .filter(collection_item::Column::CardId.eq(card_id))
        .one(&state.db)
        .await?)
}

/// Trim, drop blanks, and de-duplicate a batch of requested external card ids,
/// preserving first-seen order (so the `IN (...)` bind list has no repeats and a
/// sloppy client list is tolerated).
fn dedupe_ids(ids: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s.clone()))
        .collect()
}

fn build_page<T>(data: Vec<T>, page: u64, page_size: u64, total: u64) -> Page<T> {
    Page {
        data,
        page,
        page_size,
        total,
        has_more: page.saturating_mul(page_size) < total,
    }
}

fn validate_quantity(value: i32, field: &str) -> Result<i32, AppError> {
    if value < 0 {
        return Err(AppError::Validation(format!(
            "{field} must not be negative"
        )));
    }
    if value > MAX_QUANTITY {
        return Err(AppError::Validation(format!(
            "{field} must be at most {MAX_QUANTITY}"
        )));
    }
    Ok(value)
}

/// Parse a stored decimal price string (e.g. `"12.34"`) to integer USD cents,
/// rounding to the nearest cent. `None`/empty/unparseable yields `None` so an
/// unpriced card simply doesn't contribute to a valuation.
fn price_cents(price: Option<&str>) -> Option<i128> {
    let value: f64 = price?.trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some((value * 100.0).round() as i128)
}

/// Format integer USD cents as a 2-dp decimal string (e.g. `1234` -> `"12.34"`).
fn format_cents(cents: i128) -> String {
    let dollars = cents / 100;
    let rem = (cents % 100).abs();
    format!("{dollars}.{rem:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_and_size_defaults_and_clamps() {
        let p = ListParams {
            page: None,
            page_size: None,
        };
        assert_eq!(p.page_and_size(), (1, DEFAULT_PAGE_SIZE));

        let p = ListParams {
            page: Some(0),
            page_size: Some(9999),
        };
        assert_eq!(p.page_and_size(), (1, MAX_PAGE_SIZE));

        let p = ListParams {
            page: Some(3),
            page_size: Some(20),
        };
        assert_eq!(p.page_and_size(), (3, 20));
    }

    #[test]
    fn build_page_derives_has_more() {
        let page = build_page(vec![1, 2, 3], 1, 3, 10);
        assert!(page.has_more, "more rows remain after page 1");
        let page = build_page(vec![1], 4, 3, 10);
        assert!(!page.has_more, "page 4 of 10 rows is the last");
        let page = build_page(Vec::<i32>::new(), 1, 60, 0);
        assert!(!page.has_more);
    }

    #[test]
    fn dedupe_ids_trims_dedupes_and_drops_blanks() {
        let out = dedupe_ids(vec![
            "  a ".into(),
            "b".into(),
            "a".into(),
            "".into(),
            "   ".into(),
            "b".into(),
            "c".into(),
        ]);
        // First-seen order preserved, blanks gone, no repeats.
        assert_eq!(out, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn validate_quantity_bounds() {
        assert_eq!(validate_quantity(0, "quantity").unwrap(), 0);
        assert_eq!(validate_quantity(5, "quantity").unwrap(), 5);
        assert!(matches!(
            validate_quantity(-1, "quantity"),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            validate_quantity(MAX_QUANTITY + 1, "foil_quantity"),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn price_cents_parses_and_rounds() {
        assert_eq!(price_cents(Some("12.34")), Some(1234));
        assert_eq!(price_cents(Some("0.5")), Some(50));
        assert_eq!(price_cents(Some("  1  ")), Some(100));
        assert_eq!(price_cents(Some("0.005")), Some(1)); // rounds to nearest cent
        assert_eq!(price_cents(Some("")), None);
        assert_eq!(price_cents(Some("n/a")), None);
        assert_eq!(price_cents(None), None);
    }

    #[test]
    fn format_cents_renders_two_decimals() {
        assert_eq!(format_cents(1234), "12.34");
        assert_eq!(format_cents(5), "0.05");
        assert_eq!(format_cents(100), "1.00");
        assert_eq!(format_cents(0), "0.00");
    }
}

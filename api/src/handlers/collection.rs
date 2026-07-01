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
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    SelectTwo, Set, SqlErr,
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
use crate::handlers::catalog::{CardResponse, SortDir, SortField, apply_card_sort, search_condition};
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
/// Hard ceiling on an uploaded collection CSV, enforced as a route body limit (see the
/// router). Sized generously above any real collection *when exported with only the three
/// columns we ask for* (Scryfall ID, Finish, Quantity ≈ 60 bytes/row, so ~16 MB spans far
/// more than [`collection_import`]'s row cap) while bounding the memory a single upload
/// can force us to buffer + parse. A larger, all-columns export can exceed this — the UI
/// tells the user to export only the three needed columns.
pub const MAX_CSV_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

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
    /// Optional search query — the same Scryfall-style syntax the public catalog
    /// card lists accept (parsed by [`crate::scryfall::search`]); a malformed query
    /// is a 422. Absent/blank means no filter.
    #[serde(default)]
    pub q: Option<String>,
    /// Sort key. `updated` (the default) orders by most-recently-changed; every other
    /// key (`name`/`rarity`/`released`/`cmc`/`price`) reuses the catalog card sorts.
    /// An unknown value is a 422.
    #[serde(default)]
    pub sort: Option<String>,
    /// Sort direction (`asc`/`desc`); absent = the sort key's natural direction. An
    /// unknown value is a 422.
    #[serde(default)]
    pub dir: Option<String>,
}

/// How the collection list is ordered: either the collection-specific recency order
/// (the default) or one of the shared catalog card sorts, reused verbatim so the
/// collection grid can sort identically to the browse grids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CollectionSort {
    /// Most-recently added/updated first (by `collection_items.updated_at`).
    Recent,
    /// A card-column sort shared with the catalog card lists.
    Card(SortField),
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

    /// The trimmed search query, or `None` when it's absent or blank.
    fn search(&self) -> Option<&str> {
        self.q.as_deref().map(str::trim).filter(|q| !q.is_empty())
    }

    /// Resolve the `sort`/`dir` params into a validated `(sort, direction)`,
    /// defaulting to most-recently-updated. An unrecognised key/direction is a 422 —
    /// consistent with a malformed `q` — rather than being silently ignored.
    fn sort_spec(&self) -> Result<(CollectionSort, SortDir), AppError> {
        let (sort, default_dir) = match self.sort.as_deref().map(str::trim).filter(|s| !s.is_empty())
        {
            // The collection's natural default (and its explicit key) is recency.
            None | Some("updated" | "recent") => (CollectionSort::Recent, SortDir::Desc),
            Some(value) => {
                let field = SortField::parse(value)?;
                (CollectionSort::Card(field), field.default_dir())
            }
        };
        let dir = match self.dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => default_dir,
            Some(value) => SortDir::parse(value)?,
        };
        Ok((sort, dir))
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
    let game_meta = require_game(&game)?;
    let (page, page_size) = params.page_and_size();
    let (sort, dir) = params.sort_spec()?;
    // Parse the optional Scryfall-syntax query up front so a malformed one 422s
    // before we touch the DB (mirrors the catalog card lists).
    let search = params
        .search()
        .map(|s| search_condition(game_meta, s))
        .transpose()?;

    let paginator =
        collection_query(user.id, &game, search, sort, dir).paginate(&state.db, page_size);
    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;

    // `find_also_related` is a LEFT join, so a holding whose card row is gone (e.g.
    // removed by a catalog re-import) comes back with `None` — skip it, exactly as
    // the summary/valuation reads do.
    let data: Vec<CollectionEntry> = rows
        .into_iter()
        .filter_map(|(item, card)| {
            card.map(|c| CollectionEntry {
                card: CardResponse::from(c),
                quantity: item.quantity,
                foil_quantity: item.foil_quantity,
            })
        })
        .collect();

    Ok(Json(build_page(data, page, page_size, total)))
}

/// Build the collection-list query for a user + game: join `cards` (so it can be
/// searched and sorted on card columns), apply the optional already-parsed search
/// condition, and order by the chosen sort. Kept separate from the handler so the
/// join/filter/sort can be unit-tested against a seeded DB without an `AppState`.
///
/// The search condition and the card sort touch only `cards` columns; the `user_id`
/// and `game` filters and the recency sort stay entity-qualified to
/// `collection_items`, so nothing is ambiguous across the join (both tables carry a
/// `game` column).
fn collection_query(
    user_id: i32,
    game: &str,
    search: Option<Condition>,
    sort: CollectionSort,
    dir: SortDir,
) -> SelectTwo<collection_item::Entity, card::Entity> {
    let mut query = CollectionItem::find()
        .find_also_related(Card)
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game));
    if let Some(condition) = search {
        query = query.filter(condition);
    }
    match sort {
        // Newest change first (or oldest, if reversed), with a stable id tiebreaker
        // for deterministic paging.
        CollectionSort::Recent => query
            .order_by(collection_item::Column::UpdatedAt, dir.order())
            .order_by(collection_item::Column::Id, dir.order()),
        CollectionSort::Card(field) => apply_card_sort(query, field, dir, false),
    }
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

/// Query params for a CSV upload. The reconcile `mode` rides in the query string so the
/// request body can be the raw CSV file. Parsed as an optional string (not a typed
/// `ReconcileMode`) so a missing/invalid mode is our JSON `422`, not axum's default
/// text/plain query-rejection.
#[derive(Debug, Deserialize)]
pub struct CsvImportParams {
    pub mode: Option<String>,
}

/// `POST /api/collection/{game}/import/csv?mode=...` -> import a collection from an
/// uploaded Archidekt CSV export. The request body is the raw CSV file (bounded by the
/// route's body limit, [`MAX_CSV_UPLOAD_BYTES`]); the reconcile mode is a query param.
///
/// Unlike the URL import this needs no upstream fetch, so it reconciles **synchronously**
/// and returns the [`ImportSummary`] directly (no rate limiter, no background job): a CSV
/// has no location to re-sync from, so it's inherently one-off. `404` for an unknown game,
/// `422` for a bad mode / unreadable CSV / one missing a required column / an empty upload.
pub async fn import_collection_csv(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<CsvImportParams>,
    body: Bytes,
) -> Result<Json<ImportSummary>, AppError> {
    require_game(&game)?;
    // The CSV shape is Archidekt's, and Archidekt is Magic-only (its card ids are Scryfall
    // ids); gate on the same provider/game support as the URL import.
    if !Provider::Archidekt.supports_game(&game) {
        return Err(AppError::Validation(format!(
            "CSV collection import is not available for '{game}'"
        )));
    }
    let mode = parse_reconcile_mode(params.mode.as_deref())?;
    if body.is_empty() {
        return Err(AppError::Validation("no CSV file was uploaded".to_string()));
    }

    let summary = collection_import::execute_csv_import(&state.db, user.id, &game, mode, &body)
        .await
        .map_err(AppError::from)?;
    Ok(Json(summary))
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

/// Parse a reconcile mode from a query param, 422 when absent or unrecognised. Used by
/// the CSV upload, where the mode is a query param rather than a typed JSON field (so a
/// bad value returns our JSON error, not axum's default query rejection).
fn parse_reconcile_mode(s: Option<&str>) -> Result<ReconcileMode, AppError> {
    match s.map(str::trim) {
        Some("overwrite") => Ok(ReconcileMode::Overwrite),
        Some("replace") => Ok(ReconcileMode::Replace),
        Some("merge") => Ok(ReconcileMode::Merge),
        _ => Err(AppError::Validation(
            "mode must be one of: overwrite, replace, merge".to_string(),
        )),
    }
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

    /// Build a `ListParams` with only paging set — the search/sort tests override
    /// `q`/`sort`/`dir` via struct-update on top of this.
    fn params(page: Option<u64>, page_size: Option<u64>) -> ListParams {
        ListParams {
            page,
            page_size,
            q: None,
            sort: None,
            dir: None,
        }
    }

    #[test]
    fn page_and_size_defaults_and_clamps() {
        assert_eq!(params(None, None).page_and_size(), (1, DEFAULT_PAGE_SIZE));
        assert_eq!(params(Some(0), Some(9999)).page_and_size(), (1, MAX_PAGE_SIZE));
        assert_eq!(params(Some(3), Some(20)).page_and_size(), (3, 20));
    }

    #[test]
    fn search_trims_and_blank_filters() {
        assert_eq!(
            ListParams {
                q: Some("  goblin ".into()),
                ..params(None, None)
            }
            .search(),
            Some("goblin")
        );
        assert_eq!(
            ListParams {
                q: Some("   ".into()),
                ..params(None, None)
            }
            .search(),
            None
        );
        assert_eq!(params(None, None).search(), None);
    }

    #[test]
    fn sort_spec_defaults_to_recent_and_reuses_card_sorts() {
        // Absent, and the explicit recency keys, all resolve to newest-first.
        for sort in [None, Some("updated"), Some("recent")] {
            assert_eq!(
                ListParams {
                    sort: sort.map(str::to_string),
                    ..params(None, None)
                }
                .sort_spec()
                .unwrap(),
                (CollectionSort::Recent, SortDir::Desc)
            );
        }
        // A reversed recency (oldest first).
        assert_eq!(
            ListParams {
                sort: Some("updated".into()),
                dir: Some("asc".into()),
                ..params(None, None)
            }
            .sort_spec()
            .unwrap(),
            (CollectionSort::Recent, SortDir::Asc)
        );
        // Card sorts borrow the catalog field + its natural direction.
        assert_eq!(
            ListParams {
                sort: Some("name".into()),
                ..params(None, None)
            }
            .sort_spec()
            .unwrap(),
            (CollectionSort::Card(SortField::Name), SortDir::Asc)
        );
        assert_eq!(
            ListParams {
                sort: Some("price".into()),
                ..params(None, None)
            }
            .sort_spec()
            .unwrap(),
            (CollectionSort::Card(SortField::Price), SortDir::Desc)
        );
    }

    #[test]
    fn sort_spec_rejects_unknown_values() {
        assert!(matches!(
            ListParams {
                sort: Some("color".into()),
                ..params(None, None)
            }
            .sort_spec(),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            ListParams {
                dir: Some("sideways".into()),
                ..params(None, None)
            }
            .sort_spec(),
            Err(AppError::Validation(_))
        ));
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
    fn parse_reconcile_mode_accepts_known_modes_and_rejects_others() {
        assert!(matches!(
            parse_reconcile_mode(Some("overwrite")),
            Ok(ReconcileMode::Overwrite)
        ));
        assert!(matches!(
            parse_reconcile_mode(Some(" replace ")),
            Ok(ReconcileMode::Replace)
        ));
        assert!(matches!(
            parse_reconcile_mode(Some("merge")),
            Ok(ReconcileMode::Merge)
        ));
        // Missing or unrecognised -> our JSON validation error (422), never a silent default.
        assert!(matches!(
            parse_reconcile_mode(None),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            parse_reconcile_mode(Some("wipe")),
            Err(AppError::Validation(_))
        ));
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

    /// A minimal `mtg` card row: only the fields the collection search/sort tests
    /// exercise (name, type line, USD price) are meaningful; the rest are defaulted.
    fn seed_card(id: i32, name: &str, type_line: &str, price_usd: Option<&str>) -> card::Model {
        let ts = "2024-01-01T00:00:00Z"
            .parse::<sea_orm::prelude::DateTimeUtc>()
            .unwrap();
        card::Model {
            id,
            game: "mtg".into(),
            external_id: format!("ext-{id}"),
            oracle_id: None,
            name: name.into(),
            set_code: "tst".into(),
            set_name: "TST".into(),
            collector_number: id.to_string(),
            collector_number_int: Some(id),
            rarity: None,
            lang: "en".into(),
            released_at: None,
            mana_cost: None,
            cmc: None,
            type_line: Some(type_line.into()),
            color_identity: None,
            colors: None,
            layout: None,
            oracle_text: None,
            power: None,
            toughness: None,
            loyalty: None,
            image_small: None,
            image_normal: None,
            image_large: None,
            image_art_crop: None,
            image_png: None,
            card_faces: None,
            price_usd: price_usd.map(str::to_string),
            price_usd_foil: None,
            price_eur: None,
            price_tix: None,
            digital: false,
            created_at: ts,
            updated_at: ts,
        }
    }

    /// The joined collection query scopes to the signed-in user, filters with the
    /// shared Scryfall search over card columns, and orders by the chosen sort —
    /// exercising the whole `collection_query` path against a real (in-memory) DB.
    #[tokio::test]
    async fn collection_query_scopes_by_user_and_applies_search_and_sort() {
        use sea_orm::{ActiveModelTrait, IntoActiveModel, prelude::DateTimeUtc};

        let db = crate::test_support::migrated_memory_db().await;
        let mtg = catalog::find("mtg").expect("mtg game");
        let at = |s: &str| s.parse::<DateTimeUtc>().unwrap();

        // Two users; user 2 exists only to prove their holdings never leak into
        // user 1's list (the FK on collection_items.user_id needs the rows present).
        for uid in [1, 2] {
            crate::entities::user::ActiveModel {
                id: Set(uid),
                email: Set(format!("u{uid}@example.test")),
                password_hash: Set("x".into()),
                display_name: Set(None),
                created_at: Set(at("2024-01-01T00:00:00Z")),
                updated_at: Set(at("2024-01-01T00:00:00Z")),
            }
            .insert(&db)
            .await
            .expect("insert user");
        }

        for c in [
            seed_card(1, "Goblin Guide", "Creature — Goblin", Some("5.00")),
            seed_card(2, "Forest", "Basic Land — Forest", Some("0.10")),
            seed_card(3, "Goblin King", "Creature — Goblin", Some("2.00")),
            seed_card(4, "Goblin Piker", "Creature — Goblin", Some("1.00")),
        ] {
            c.into_active_model().insert(&db).await.expect("insert card");
        }

        // User 1 owns cards 1..=3 (updated at increasing times so recency order is
        // 3, 2, 1); user 2 owns card 4.
        let hold = |id: i32, card_id: i32, user_id: i32, updated: &str| {
            collection_item::ActiveModel {
                id: Set(id),
                user_id: Set(user_id),
                game: Set("mtg".into()),
                card_id: Set(card_id),
                quantity: Set(1),
                foil_quantity: Set(0),
                created_at: Set(at("2024-01-01T00:00:00Z")),
                updated_at: Set(at(updated)),
            }
        };
        for h in [
            hold(1, 1, 1, "2024-01-01T00:00:00Z"),
            hold(2, 2, 1, "2024-02-01T00:00:00Z"),
            hold(3, 3, 1, "2024-03-01T00:00:00Z"),
            hold(4, 4, 2, "2024-04-01T00:00:00Z"),
        ] {
            h.insert(&db).await.expect("insert holding");
        }

        async fn names(
            db: &sea_orm::DatabaseConnection,
            search: Option<Condition>,
            sort: CollectionSort,
            dir: SortDir,
        ) -> Vec<String> {
            collection_query(1, "mtg", search, sort, dir)
                .all(db)
                .await
                .expect("run collection query")
                .into_iter()
                .filter_map(|(_, card)| card.map(|c| c.name))
                .collect()
        }

        // Default recency (updated desc): newest holding first, user 2's card absent.
        assert_eq!(
            names(&db, None, CollectionSort::Recent, SortDir::Desc).await,
            ["Goblin King", "Forest", "Goblin Guide"]
        );

        // The shared Scryfall grammar runs over the joined card columns: `t:goblin`
        // keeps only user 1's two Goblins (Forest dropped; user 2's Goblin out of scope).
        let goblins = search_condition(mtg, "t:goblin").unwrap();
        assert_eq!(
            names(&db, Some(goblins), CollectionSort::Recent, SortDir::Desc).await,
            ["Goblin King", "Goblin Guide"]
        );

        // Price sort borrows the catalog card sort verbatim: 5.00, 2.00, 0.10.
        assert_eq!(
            names(&db, None, CollectionSort::Card(SortField::Price), SortDir::Desc).await,
            ["Goblin Guide", "Goblin King", "Forest"]
        );
    }
}

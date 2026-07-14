//! Collection import/sync endpoints: a one-off import from an external provider (URL or
//! CSV upload), polling an import job, and the saved-collection-link CRUD + re-sync. The
//! provider fetch + reconcile live in [`crate::collection_import`]; these handlers
//! validate, enqueue, and shape the responses.

use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::StatusCode,
};
use chrono::Utc;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::auth::extractor::{AuthUser, WritableUser};
use crate::collection_import::jobs::{self, JobStatus};
use crate::collection_import::{self, ImportSummary, Provider, ReconcileMode};
use crate::entities::collection_source;
use crate::entities::prelude::CollectionSource;
use crate::error::AppError;
use crate::extract::{JsonBody, Path, Query};
use crate::handlers::shared::require_game;
use crate::state::AppState;

use super::{
    CollectionSourceResponse, CsvImportParams, ImportJobResponse, ImportRequest, SaveSourceRequest,
};

/// `POST /api/collection/{game}/import` -> enqueue a one-off import from a collection
/// provider using the chosen reconcile mode (does not save the link). Validates the
/// request synchronously, then returns `202` with a job id to poll; the fetch +
/// reconcile run in the background, throttled by the provider rate limit. On success the
/// worker stamps `last_synced_at` on a saved link that points at this same collection (if
/// any), so importing your saved collection updates "Last synced" just like a re-sync.
#[utoipa::path(
    post,
    path = "/api/collection/{game}/import",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    request_body = ImportRequest,
    responses(
        (status = 202, description = "The import was enqueued; poll the returned job id.", body = ImportJobResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Unknown provider, provider unavailable for the game, live import disabled, or an unparseable source URL/id."),
    ),
)]
pub async fn import_collection(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
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
    // Refuse a provider whose live network import is temporarily disabled (Moxfield today)
    // before doing anything else — the disable is unconditional, so a bad URL shouldn't be
    // reported as a source error when the provider is off entirely.
    ensure_network_import_enabled(provider)?;
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
        },
    )?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ImportJobResponse::from_status(job_id, JobStatus::Queued)),
    ))
}

/// `POST /api/collection/{game}/import/csv?mode=...` -> import a collection from an
/// uploaded CSV export (Archidekt or Moxfield — the shape is sniffed from the header
/// row). The request body is the raw CSV file (bounded by the route's body limit,
/// [`MAX_CSV_UPLOAD_BYTES`](super::MAX_CSV_UPLOAD_BYTES)); the reconcile mode is a
/// query param.
///
/// Unlike the URL import this needs no upstream fetch, so it reconciles **synchronously**
/// and returns the [`ImportSummary`] directly (no rate limiter, no background job): a CSV
/// has no location to re-sync from, so it's inherently one-off. `404` for an unknown game,
/// `422` for a bad mode / unreadable CSV / one missing a required column / an empty upload.
#[utoipa::path(
    post,
    path = "/api/collection/{game}/import/csv",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("mode" = Option<String>, Query, description = "Reconcile mode: `overwrite` / `replace` / `merge`"),
    ),
    request_body(content_type = "text/csv", description = "The raw CSV file (an Archidekt or Moxfield collection export)."),
    responses(
        (status = 200, description = "The import ran synchronously; the summary of what was matched and applied.", body = ImportSummary),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "CSV import unavailable for the game, a bad/missing mode, an unreadable CSV, a missing required column, or an empty upload."),
    ),
)]
pub async fn import_collection_csv(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path(game): Path<String>,
    Query(params): Query<CsvImportParams>,
    body: Bytes,
) -> Result<Json<ImportSummary>, AppError> {
    require_game(&game)?;
    // Both CSV shapes identify Magic printings (Scryfall ids / set + collector number),
    // so gate on the same provider/game support as the URL imports.
    if !Provider::Archidekt.supports_game(&game) && !Provider::Moxfield.supports_game(&game) {
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
#[utoipa::path(
    get,
    path = "/api/collection/{game}/import/jobs/{job_id}",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("job_id" = u64, Path, description = "Import job id returned when the job was enqueued"),
    ),
    responses(
        (status = 200, description = "The job's status (queued / running / complete / error).", body = ImportJobResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or no such job for the caller."),
    ),
)]
pub async fn get_import_job(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, job_id)): Path<(String, u64)>,
) -> Result<Json<ImportJobResponse>, AppError> {
    require_game(&game)?;
    let view = state
        .imports
        .view(job_id, user.id, &game)
        .ok_or_else(|| AppError::NotFound("import job not found".to_string()))?;
    Ok(Json(ImportJobResponse::from_view(job_id, view)))
}

/// `GET /api/collection/{game}/source` -> the saved collection link for this game, or
/// `null` if none is saved.
#[utoipa::path(
    get,
    path = "/api/collection/{game}/source",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 200, description = "The saved collection link, or `null` when no link is saved for the game.", body = CollectionSourceResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
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
#[utoipa::path(
    put,
    path = "/api/collection/{game}/source",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    request_body = SaveSourceRequest,
    responses(
        (status = 200, description = "The saved collection link.", body = CollectionSourceResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Unknown provider, provider unavailable for the game, live import disabled, or an unparseable source URL/id."),
    ),
)]
pub async fn save_collection_source(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
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
    // A saved link exists only to be re-synced, so don't let one be saved for a provider
    // whose live import is disabled (Moxfield today) — it could never sync.
    ensure_network_import_enabled(provider)?;
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
        smart: Set(payload.smart),
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
                collection_source::Column::Smart,
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
#[utoipa::path(
    delete,
    path = "/api/collection/{game}/source",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 204, description = "The saved link was forgotten (idempotent — 204 even if nothing was saved)."),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn delete_collection_source(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
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
/// link; the worker stamps `last_synced_at` on success. Uses smart (incremental) sync
/// when the saved link opted into it, otherwise a full mirror/replace. Returns `202`
/// with a job id to poll. `404` when no link is saved.
#[utoipa::path(
    post,
    path = "/api/collection/{game}/sync",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 202, description = "The re-sync was enqueued; poll the returned job id.", body = ImportJobResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or no saved collection link to sync."),
        (status = 422, description = "The saved provider's live import is temporarily disabled."),
    ),
)]
pub async fn sync_collection_source(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
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
    // A link saved before the provider's live import was disabled can still be on file
    // (saving is now blocked too, but old rows persist), so gate the re-sync as well.
    ensure_network_import_enabled(provider)?;

    // The saved link records how it re-syncs: smart (incremental, only recently-changed
    // cards) or a full mirror (removes cards no longer upstream). Both stamp the source
    // as synced; the UI tailors its confirmation to which one runs.
    let mode = if source.smart {
        ReconcileMode::Smart
    } else {
        ReconcileMode::Replace
    };

    let job_id = jobs::spawn_import_job(
        state.db.clone(),
        state.http.clone(),
        state.imports.clone(),
        jobs::ImportRequest {
            user_id: user.id,
            game,
            provider,
            collection_id: source.external_id,
            mode,
        },
    )?;

    Ok((
        StatusCode::ACCEPTED,
        Json(ImportJobResponse::from_status(job_id, JobStatus::Queued)),
    ))
}

// ---------- Helpers ----------

/// Parse a collection-provider id from a request, 422 on an unknown provider.
fn parse_provider(s: &str) -> Result<Provider, AppError> {
    Provider::from_id(s)
        .ok_or_else(|| AppError::Validation(format!("unknown collection provider '{s}'")))
}

/// Reject a provider whose **live network** import (URL/link import + saved-link re-sync)
/// is temporarily disabled — Moxfield today, pending an approved `User-Agent` (see
/// [`Provider::network_import_enabled`]). Returns `422` with an actionable message; the
/// CSV upload path never calls this, so a disabled provider's collection can still be
/// imported by uploading its CSV export.
fn ensure_network_import_enabled(provider: Provider) -> Result<(), AppError> {
    if provider.network_import_enabled() {
        return Ok(());
    }
    Err(AppError::Validation(format!(
        "{label} link import and re-sync are temporarily unavailable. You can still import \
         a {label} collection by uploading a CSV export instead.",
        label = provider.label()
    )))
}

/// Parse a reconcile mode from a query param, 422 when absent or unrecognised. Used by
/// the CSV upload, where the mode is a query param rather than a typed JSON field (so a
/// bad value returns our JSON error, not axum's default query rejection).
pub(super) fn parse_reconcile_mode(s: Option<&str>) -> Result<ReconcileMode, AppError> {
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
        smart: row.smart,
    }
}

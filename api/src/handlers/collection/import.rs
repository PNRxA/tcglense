//! Collection import endpoints: a one-off import from an external provider (URL, uploaded
//! file, or pasted text) and polling an import job. The provider fetch + reconcile live in
//! [`crate::collection_import`]; these handlers validate, enqueue, and shape the responses.

use axum::{Json, body::Bytes, extract::State, http::StatusCode};

use crate::auth::extractor::{AuthUser, WritableUser};
use crate::collection_import::jobs::{self, JobStatus};
use crate::collection_import::{self, ImportSummary, Provider, ReconcileMode};
use crate::error::AppError;
use crate::extract::{JsonBody, Path, Query};
use crate::handlers::shared::require_game;
use crate::state::AppState;

use super::{CsvImportParams, ImportJobResponse, ImportRequest};

/// Import collection
///
/// `POST /api/collection/{game}/import` -> enqueue a one-off import from a collection
/// provider using the chosen reconcile mode. Validates the request synchronously, then
/// returns `202` with a job id to poll; the fetch + reconcile run in the background,
/// throttled by the provider rate limit.
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
        state.analytics_cache.clone(),
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

/// Import collection from CSV
///
/// `POST /api/collection/{game}/import/csv?mode=...` -> import a collection from an
/// uploaded export file. The shape is sniffed from the content: an Archidekt, Moxfield or
/// Mythic Tools CSV, or — when it matches no CSV header we know — a plain-text card list
/// (`1 Sol Ring (C21) 263 *F*`), so a `.txt` export imports here too. The request body is
/// the raw file (bounded by the route's body limit,
/// [`MAX_CSV_UPLOAD_BYTES`](super::MAX_CSV_UPLOAD_BYTES)); the reconcile mode is a
/// query param.
///
/// Unlike the URL import this needs no upstream fetch, so it reconciles **synchronously**
/// and returns the [`ImportSummary`] directly (no rate limiter, no background job).
/// `404` for an unknown game,
/// `422` for a bad mode / unreadable file / one missing a required column / an empty upload.
#[utoipa::path(
    post,
    path = "/api/collection/{game}/import/csv",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("mode" = Option<String>, Query, description = "Reconcile mode: `overwrite` / `replace` / `merge`"),
    ),
    request_body(content_type = "text/csv", description = "The raw export file: an Archidekt, Moxfield or Mythic Tools collection CSV, or a plain-text card list."),
    responses(
        (status = 200, description = "The import ran synchronously; the summary of what was matched and applied.", body = ImportSummary),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "File import unavailable for the game, a bad/missing mode, an unreadable file, a missing required column, or an empty upload."),
    ),
)]
pub async fn import_collection_csv(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path(game): Path<String>,
    Query(params): Query<CsvImportParams>,
    body: Bytes,
) -> Result<Json<ImportSummary>, AppError> {
    run_file_import(state, user.id, game, params, body, "no file was uploaded").await
}

/// Import collection from pasted text
///
/// `POST /api/collection/{game}/import/text?mode=...` -> import a collection from text the
/// user pasted, for apps that can't hand a browser a file. Mythic Tools (issue #572) is the
/// motivating case: it's a phone app, so copying its export out is far easier than saving
/// and uploading it.
///
/// The body is the pasted text, and the format is sniffed exactly as for an uploaded file —
/// a pasted CSV (Mythic Tools, Archidekt, Moxfield) and a pasted card list
/// (`1 Sol Ring (C21) 263 *F*`, one per line) both work, so the user never has to tell us
/// which they have. Runs synchronously and returns the [`ImportSummary`], like the upload.
#[utoipa::path(
    post,
    path = "/api/collection/{game}/import/text",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("mode" = Option<String>, Query, description = "Reconcile mode: `overwrite` / `replace` / `merge`"),
    ),
    request_body(content_type = "text/plain", description = "The pasted collection: a card list (one card per line) or the contents of a collection CSV export."),
    responses(
        (status = 200, description = "The import ran synchronously; the summary of what was matched and applied.", body = ImportSummary),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Text import unavailable for the game, a bad/missing mode, unreadable text, or nothing pasted."),
    ),
)]
pub async fn import_collection_text(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path(game): Path<String>,
    Query(params): Query<CsvImportParams>,
    body: Bytes,
) -> Result<Json<ImportSummary>, AppError> {
    run_file_import(
        state,
        user.id,
        game,
        params,
        body,
        "paste your collection first",
    )
    .await
}

/// The shared body of the upload and paste imports: validate, run the sniffing importer,
/// and bump the analytics cache. Only the "nothing was sent" message differs, so the two
/// routes stay one behaviour with two entry points rather than two implementations.
async fn run_file_import(
    state: AppState,
    user_id: i32,
    game: String,
    params: CsvImportParams,
    body: Bytes,
    empty_message: &str,
) -> Result<Json<ImportSummary>, AppError> {
    require_game(&game)?;
    // Every supported shape identifies Magic printings (Scryfall ids / set + collector
    // number / card names), so gate on the same provider/game support as the URL imports.
    if !Provider::Archidekt.supports_game(&game)
        && !Provider::Moxfield.supports_game(&game)
        && !Provider::MythicTools.supports_game(&game)
    {
        return Err(AppError::Validation(format!(
            "collection import is not available for '{game}'"
        )));
    }
    let mode = parse_reconcile_mode(params.mode.as_deref())?;
    if body.is_empty() {
        return Err(AppError::Validation(empty_message.to_string()));
    }

    let result =
        collection_import::execute_file_import(&state.db, user_id, &game, mode, &body).await;
    // Orphan the user's cached analytics bodies (#413) on success AND failure: the
    // reconcile commits mutations before its outcome is known (the star-holding
    // fold runs in its own transaction ahead of the plan apply), so a failed
    // import may still have changed the holdings. A spurious bump costs one cache
    // miss; a missed one serves stale analytics for the body TTL.
    state.analytics_cache.bump_holdings(user_id, &game).await;
    Ok(Json(result.map_err(AppError::from)?))
}

/// Get import job
///
/// `GET /api/collection/{game}/import/jobs/{job_id}` -> the status of a background
/// import job (queued / running / complete / error). `404` for an unknown job or one
/// that isn't the caller's.
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

// ---------- Helpers ----------

/// Parse a collection-provider id from a request, 422 on an unknown provider.
fn parse_provider(s: &str) -> Result<Provider, AppError> {
    Provider::from_id(s)
        .ok_or_else(|| AppError::Validation(format!("unknown collection provider '{s}'")))
}

/// Reject a provider whose **live network** (URL/link) import is temporarily disabled —
/// Moxfield today, pending an approved `User-Agent` (see
/// [`Provider::network_import_enabled`]). Returns `422` with an actionable message; the
/// file/paste import path never calls this, so a disabled provider's collection can still
/// be imported by uploading or pasting its CSV export.
fn ensure_network_import_enabled(provider: Provider) -> Result<(), AppError> {
    if provider.network_import_enabled() {
        return Ok(());
    }
    Err(AppError::Validation(format!(
        "{label} link import is temporarily unavailable. You can still import a {label} \
         collection by uploading a CSV export instead.",
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

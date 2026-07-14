//! Authenticated per-(user, game) public-visibility toggle plus the owner's collection-
//! landing display preferences (issue #381). Lives in the router's `private` group
//! (no-store, per-user rate limited).

use axum::{Json, extract::State};

use crate::auth::extractor::{AuthUser, WritableUser};
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::require_game;
use crate::state::AppState;

use super::{CollectionVisibility, SetVisibilityRequest, apply_visibility_patch, visibility_state};

/// `GET /api/collection/{game}/visibility` -> whether this game's collection is public, the
/// owner's landing display prefs, and the caller's public handle (null until they set a
/// username). Defaults (private, both sections shown) when no row exists yet.
#[utoipa::path(
    get,
    path = "/api/collection/{game}/visibility",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 200, description = "The (user, game) sharing + display state and the caller's handle.", body = CollectionVisibility),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn get_collection_visibility(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<CollectionVisibility>, AppError> {
    require_game(&game)?;
    let s = visibility_state(&state.db, user.id, &game).await?;
    Ok(Json(CollectionVisibility {
        public: s.is_public,
        show_value_chart: s.show_value_chart,
        show_movers: s.show_movers,
        handle: crate::auth::username::handle_of(&user),
    }))
}

/// `PUT /api/collection/{game}/visibility` -> partial patch of sharing + display prefs.
/// `WritableUser`, so a read-only API key is 403. Enabling public **requires a username
/// first** (a public collection is addressed by handle) — a 409 the SPA branches on to
/// prompt the username step. Only the fields present in the body are written, so the
/// display toggles never disturb the sharing flag and vice versa.
#[utoipa::path(
    put,
    path = "/api/collection/{game}/visibility",
    tag = "Collection",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    request_body = SetVisibilityRequest,
    responses(
        (status = 200, description = "The resulting sharing + display state after the patch.", body = CollectionVisibility),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "A read-scoped API key cannot write."),
        (status = 404, description = "Unknown game."),
        (status = 409, description = "Enabling public sharing without a username set first."),
    ),
)]
pub async fn set_collection_visibility(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<SetVisibilityRequest>,
) -> Result<Json<CollectionVisibility>, AppError> {
    require_game(&game)?;

    if payload.public == Some(true) && user.username.is_none() {
        return Err(AppError::Conflict(
            "set a username before making a collection public".to_string(),
        ));
    }

    let s = apply_visibility_patch(
        &state.db,
        user.id,
        &game,
        payload.public,
        payload.show_value_chart,
        payload.show_movers,
    )
    .await?;

    Ok(Json(CollectionVisibility {
        public: s.is_public,
        show_value_chart: s.show_value_chart,
        show_movers: s.show_movers,
        handle: crate::auth::username::handle_of(&user),
    }))
}

//! Authenticated per-(user, game) public-visibility toggle. Lives in the router's
//! `private` group (no-store, per-user rate limited).

use axum::{Json, extract::State};

use crate::auth::extractor::{AuthUser, WritableUser};
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::require_game;
use crate::state::AppState;

use super::{CollectionVisibility, SetVisibilityRequest, is_public, set_visibility};

/// `GET /api/collection/{game}/visibility` -> whether this game's collection is public,
/// plus the caller's public handle (null until they set a username).
pub async fn get_collection_visibility(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<CollectionVisibility>, AppError> {
    require_game(&game)?;
    Ok(Json(CollectionVisibility {
        public: is_public(&state.db, user.id, &game).await?,
        handle: crate::auth::username::handle_of(&user),
    }))
}

/// `PUT /api/collection/{game}/visibility` -> enable/disable public sharing for this
/// game. `WritableUser`, so a read-only API key is 403. Enabling **requires a username
/// first** (a public collection is addressed by handle) — a 409 the SPA branches on to
/// prompt the username step.
pub async fn set_collection_visibility(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<SetVisibilityRequest>,
) -> Result<Json<CollectionVisibility>, AppError> {
    require_game(&game)?;

    if payload.public && user.username.is_none() {
        return Err(AppError::Conflict(
            "set a username before making a collection public".to_string(),
        ));
    }

    set_visibility(&state.db, user.id, &game, payload.public).await?;

    Ok(Json(CollectionVisibility {
        public: payload.public,
        handle: crate::auth::username::handle_of(&user),
    }))
}

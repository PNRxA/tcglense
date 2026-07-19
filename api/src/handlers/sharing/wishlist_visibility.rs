//! Authenticated per-(user, game) wish-list public-visibility toggle (issue #493). The
//! wish-list twin of [`super::visibility`], reading/writing the independent
//! `wishlist_is_public` flag on the same `collection_visibility` row. Lives in the router's
//! `private` group (no-store, per-user rate limited). A wish list has no landing display
//! prefs, so this surface is just the public/private switch.

use axum::{Json, extract::State};

use crate::auth::extractor::{AuthUser, WritableUser};
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::require_game;
use crate::state::AppState;

use super::{
    SetWishlistVisibilityRequest, WishlistVisibility, apply_visibility_patch, visibility_state,
};

/// Get wish list visibility
///
/// `GET /api/wishlist/{game}/visibility` -> whether this game's wish list is public, plus the
/// caller's public handle (null until they set a username). Defaults (private) when no row
/// exists yet.
#[utoipa::path(
    get,
    path = "/api/wishlist/{game}/visibility",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    responses(
        (status = 200, description = "The (user, game) wish-list sharing state and the caller's handle.", body = WishlistVisibility),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
    ),
)]
pub async fn get_wishlist_visibility(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<WishlistVisibility>, AppError> {
    require_game(&game)?;
    let s = visibility_state(&state.db, user.id, &game).await?;
    Ok(Json(WishlistVisibility {
        public: s.wishlist_is_public,
        handle: crate::auth::username::handle_of(&user),
    }))
}

/// Set wish list visibility
///
/// `PUT /api/wishlist/{game}/visibility` -> flip the wish-list sharing flag. `WritableUser`,
/// so a read-only API key is 403. Enabling public **requires a username first** (a public
/// wish list is addressed by handle) — a 409 the SPA branches on to prompt the username step.
/// Only the wish-list flag is touched, so the collection's sharing state and the landing
/// display prefs on the same row are left untouched.
#[utoipa::path(
    put,
    path = "/api/wishlist/{game}/visibility",
    tag = "Wish list",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    request_body = SetWishlistVisibilityRequest,
    responses(
        (status = 200, description = "The resulting wish-list sharing state after the patch.", body = WishlistVisibility),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "A read-scoped API key cannot write."),
        (status = 404, description = "Unknown game."),
        (status = 409, description = "Enabling public sharing without a username set first."),
    ),
)]
pub async fn set_wishlist_visibility(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<SetWishlistVisibilityRequest>,
) -> Result<Json<WishlistVisibility>, AppError> {
    require_game(&game)?;

    if payload.public == Some(true) && user.username.is_none() {
        return Err(AppError::Conflict(
            "set a username before making a wish list public".to_string(),
        ));
    }

    let s = apply_visibility_patch(
        &state.db,
        user.id,
        &game,
        // The wish-list toggle never touches the collection flag or the display prefs.
        None,
        None,
        None,
        payload.public,
    )
    .await?;

    Ok(Json(WishlistVisibility {
        public: s.wishlist_is_public,
        handle: crate::auth::username::handle_of(&user),
    }))
}

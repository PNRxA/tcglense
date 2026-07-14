//! Deck write endpoints: create / update-metadata / delete a deck, move it between
//! folders, and toggle its public-sharing flag. All take [`WritableUser`] (a read-only
//! API key is `403`).

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};

use crate::auth::extractor::WritableUser;
use crate::entities::prelude::{Deck, DeckSection};
use crate::entities::{deck, deck_section};
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::require_game;
use crate::state::AppState;

use super::{
    CreateDeckRequest, DEFAULT_SECTIONS, DeckResponse, DeckVisibility, MAX_DECK_DESCRIPTION,
    MAX_DECK_NAME, MAX_DECKS_PER_GAME, MAX_FORMAT, MoveDeckFolderRequest, SetDeckVisibilityRequest,
    UpdateDeckRequest, card_counts_by_deck, load_deck, resolve_folder_ref, validate_name,
    validate_optional,
};
use crate::handlers::decks::deck_detail;

/// Create deck
///
/// `POST /api/decks/{game}` -> create a deck (seeded with the default sections) and return
/// its full detail. `422` for a blank/oversized name or over the per-game deck cap; `404`
/// if `folder_id` isn't one of the caller's folders.
#[utoipa::path(
    post,
    path = "/api/decks/{game}",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
    ),
    request_body = CreateDeckRequest,
    responses(
        (status = 200, description = "The newly created deck's full detail (seeded with default sections).", body = super::DeckDetail),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or `folder_id` is not one of the caller's folders."),
        (status = 422, description = "Blank/oversized name, or over the per-game deck cap."),
    ),
)]
pub async fn create_deck(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<CreateDeckRequest>,
) -> Result<Json<super::DeckDetail>, AppError> {
    require_game(&game)?;
    let name = validate_name(&payload.name, "name", MAX_DECK_NAME)?;
    let description = validate_optional(payload.description, "description", MAX_DECK_DESCRIPTION)?;
    let format = validate_optional(payload.format, "format", MAX_FORMAT)?;
    let folder_id = resolve_folder_ref(&state, user.id, &game, payload.folder_id).await?;

    let count = Deck::find()
        .filter(deck::Column::UserId.eq(user.id))
        .filter(deck::Column::Game.eq(&game))
        .count(&state.db)
        .await?;
    if count >= MAX_DECKS_PER_GAME {
        return Err(AppError::Validation(format!(
            "you can have at most {MAX_DECKS_PER_GAME} decks per game"
        )));
    }

    let now = Utc::now();
    let deck = deck::ActiveModel {
        user_id: Set(user.id),
        game: Set(game.clone()),
        folder_id: Set(folder_id),
        name: Set(name),
        description: Set(description),
        format: Set(format),
        is_public: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;

    // Seed the default sections so the new deck has a ready structure to sort into.
    let sections: Vec<deck_section::ActiveModel> = DEFAULT_SECTIONS
        .iter()
        .enumerate()
        .map(|(i, name)| deck_section::ActiveModel {
            deck_id: Set(deck.id),
            name: Set((*name).to_string()),
            position: Set(i as i32),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        })
        .collect();
    DeckSection::insert_many(sections).exec(&state.db).await?;

    let handle = crate::auth::username::handle_of(&user);
    Ok(Json(deck_detail(&state, &deck, handle).await?))
}

/// Update deck
///
/// `PUT /api/decks/{game}/{deck_id}` -> replace the deck's editable metadata
/// (name/description/format). Folder + sharing are separate endpoints.
#[utoipa::path(
    put,
    path = "/api/decks/{game}/{deck_id}",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
    ),
    request_body = UpdateDeckRequest,
    responses(
        (status = 200, description = "The updated deck header.", body = DeckResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
        (status = 422, description = "Blank/oversized name."),
    ),
)]
pub async fn update_deck(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, deck_id)): Path<(String, i32)>,
    JsonBody(payload): JsonBody<UpdateDeckRequest>,
) -> Result<Json<DeckResponse>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let name = validate_name(&payload.name, "name", MAX_DECK_NAME)?;
    let description = validate_optional(payload.description, "description", MAX_DECK_DESCRIPTION)?;
    let format = validate_optional(payload.format, "format", MAX_FORMAT)?;

    let mut active: deck::ActiveModel = deck.into();
    active.name = Set(name);
    active.description = Set(description);
    active.format = Set(format);
    active.updated_at = Set(Utc::now());
    let updated = active.update(&state.db).await?;

    let count = card_counts_by_deck(&state.db, &[updated.id])
        .await?
        .get(&updated.id)
        .copied()
        .unwrap_or(0);
    Ok(Json(DeckResponse::from_model(&updated, count)))
}

/// Delete deck
///
/// `DELETE /api/decks/{game}/{deck_id}` -> delete the deck (its sections + cards cascade
/// away). Idempotent-ish: a deck that isn't the caller's is a `404`.
#[utoipa::path(
    delete,
    path = "/api/decks/{game}/{deck_id}",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
    ),
    responses(
        (status = 204, description = "The deck (and its sections + cards) was deleted."),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
    ),
)]
pub async fn delete_deck(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, deck_id)): Path<(String, i32)>,
) -> Result<StatusCode, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    Deck::delete_by_id(deck.id).exec(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Move deck to folder
///
/// `PUT /api/decks/{game}/{deck_id}/folder` -> move the deck into a folder, or `null` to
/// loosen it. A non-null `folder_id` must be one of the caller's folders (`404` otherwise).
#[utoipa::path(
    put,
    path = "/api/decks/{game}/{deck_id}/folder",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
    ),
    request_body = MoveDeckFolderRequest,
    responses(
        (status = 200, description = "The deck header, with its new (or cleared) folder.", body = DeckResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, the deck is not the caller's, or `folder_id` is not one of the caller's folders."),
    ),
)]
pub async fn move_deck_to_folder(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, deck_id)): Path<(String, i32)>,
    JsonBody(payload): JsonBody<MoveDeckFolderRequest>,
) -> Result<Json<DeckResponse>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;
    let folder_id = resolve_folder_ref(&state, user.id, &game, payload.folder_id).await?;

    let mut active: deck::ActiveModel = deck.into();
    active.folder_id = Set(folder_id);
    active.updated_at = Set(Utc::now());
    let updated = active.update(&state.db).await?;

    let count = card_counts_by_deck(&state.db, &[updated.id])
        .await?
        .get(&updated.id)
        .copied()
        .unwrap_or(0);
    Ok(Json(DeckResponse::from_model(&updated, count)))
}

/// Set deck visibility
///
/// `PUT /api/decks/{game}/{deck_id}/visibility` -> enable/disable public sharing. Enabling
/// requires a username first (a public deck is addressed by handle) — else `409`, which the
/// SPA branches on to prompt the username step. Mirrors the collection visibility toggle.
#[utoipa::path(
    put,
    path = "/api/decks/{game}/{deck_id}/visibility",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("deck_id" = i32, Path, description = "Deck id"),
    ),
    request_body = SetDeckVisibilityRequest,
    responses(
        (status = 200, description = "The deck's new sharing state (public flag + owner handle).", body = DeckVisibility),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or the deck is not the caller's."),
        (status = 409, description = "Enabling public sharing without a username set."),
    ),
)]
pub async fn set_deck_visibility(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, deck_id)): Path<(String, i32)>,
    JsonBody(payload): JsonBody<SetDeckVisibilityRequest>,
) -> Result<Json<DeckVisibility>, AppError> {
    require_game(&game)?;
    let deck = load_deck(&state, user.id, &game, deck_id).await?;

    if payload.public && user.username.is_none() {
        return Err(AppError::Conflict(
            "set a username before making a deck public".to_string(),
        ));
    }

    let mut active: deck::ActiveModel = deck.into();
    active.is_public = Set(payload.public);
    active.updated_at = Set(Utc::now());
    active.update(&state.db).await?;

    Ok(Json(DeckVisibility {
        public: payload.public,
        handle: crate::auth::username::handle_of(&user),
    }))
}

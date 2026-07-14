//! Deck-folder endpoints: list / create / rename / delete the folders that organise a
//! game's decks. Reads take [`AuthUser`], writes take [`WritableUser`].

use std::collections::HashMap;

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};

use crate::auth::extractor::{AuthUser, WritableUser};
use crate::entities::prelude::{Deck, DeckFolder};
use crate::entities::{deck, deck_folder};
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::{DataBody, require_game};
use crate::state::AppState;

use super::{
    DeckFolderResponse, FolderNameRequest, MAX_FOLDER_NAME, MAX_FOLDERS_PER_GAME, validate_name,
};

/// Load a folder by id, proving it belongs to `user_id` for `game`. `404` otherwise.
async fn load_folder(
    state: &AppState,
    user_id: i32,
    game: &str,
    folder_id: i32,
) -> Result<deck_folder::Model, AppError> {
    DeckFolder::find_by_id(folder_id)
        .filter(deck_folder::Column::UserId.eq(user_id))
        .filter(deck_folder::Column::Game.eq(game))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("folder not found".to_string()))
}

/// How many decks are filed under each folder for a user + game (one grouped aggregate).
async fn deck_counts_by_folder(
    state: &AppState,
    user_id: i32,
    game: &str,
) -> Result<HashMap<i32, i64>, AppError> {
    let rows: Vec<(Option<i32>, i64)> = Deck::find()
        .select_only()
        .column(deck::Column::FolderId)
        .column_as(Expr::cust("COUNT(*)"), "deck_count")
        .filter(deck::Column::UserId.eq(user_id))
        .filter(deck::Column::Game.eq(game))
        .filter(deck::Column::FolderId.is_not_null())
        .group_by(deck::Column::FolderId)
        .into_tuple()
        .all(&state.db)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, count)| id.map(|id| (id, count)))
        .collect())
}

/// `GET /api/decks/{game}/folders` -> the caller's folders for a game (alphabetical), each
/// with how many decks are filed under it.
pub async fn list_folders(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<DataBody<Vec<DeckFolderResponse>>>, AppError> {
    require_game(&game)?;

    let folders = DeckFolder::find()
        .filter(deck_folder::Column::UserId.eq(user.id))
        .filter(deck_folder::Column::Game.eq(&game))
        .order_by_asc(deck_folder::Column::Name)
        .all(&state.db)
        .await?;
    let counts = deck_counts_by_folder(&state, user.id, &game).await?;

    let data = folders
        .into_iter()
        .map(|f| DeckFolderResponse {
            deck_count: counts.get(&f.id).copied().unwrap_or(0),
            id: f.id,
            name: f.name,
        })
        .collect();
    Ok(Json(DataBody { data }))
}

/// `POST /api/decks/{game}/folders` -> create a folder. `422` for a blank/oversized name
/// or over the per-game cap; `409` if a folder with that name already exists.
pub async fn create_folder(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<FolderNameRequest>,
) -> Result<Json<DeckFolderResponse>, AppError> {
    require_game(&game)?;
    let name = validate_name(&payload.name, "name", MAX_FOLDER_NAME)?;

    let count = DeckFolder::find()
        .filter(deck_folder::Column::UserId.eq(user.id))
        .filter(deck_folder::Column::Game.eq(&game))
        .count(&state.db)
        .await?;
    if count >= MAX_FOLDERS_PER_GAME {
        return Err(AppError::Validation(format!(
            "you can have at most {MAX_FOLDERS_PER_GAME} folders per game"
        )));
    }
    ensure_unique_name(&state, user.id, &game, &name, None).await?;

    let now = Utc::now();
    let folder = deck_folder::ActiveModel {
        user_id: Set(user.id),
        game: Set(game.clone()),
        name: Set(name),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;

    Ok(Json(DeckFolderResponse {
        id: folder.id,
        name: folder.name,
        deck_count: 0,
    }))
}

/// `PUT /api/decks/{game}/folders/{folder_id}` -> rename a folder.
pub async fn update_folder(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, folder_id)): Path<(String, i32)>,
    JsonBody(payload): JsonBody<FolderNameRequest>,
) -> Result<Json<DeckFolderResponse>, AppError> {
    require_game(&game)?;
    let folder = load_folder(&state, user.id, &game, folder_id).await?;
    let name = validate_name(&payload.name, "name", MAX_FOLDER_NAME)?;
    ensure_unique_name(&state, user.id, &game, &name, Some(folder.id)).await?;

    let mut active: deck_folder::ActiveModel = folder.into();
    active.name = Set(name);
    active.updated_at = Set(Utc::now());
    let updated = active.update(&state.db).await?;

    let counts = deck_counts_by_folder(&state, user.id, &game).await?;
    Ok(Json(DeckFolderResponse {
        deck_count: counts.get(&updated.id).copied().unwrap_or(0),
        id: updated.id,
        name: updated.name,
    }))
}

/// `DELETE /api/decks/{game}/folders/{folder_id}` -> delete a folder. Its decks are
/// **ungrouped** (their `folder_id` -> NULL via the FK), not deleted.
pub async fn delete_folder(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, folder_id)): Path<(String, i32)>,
) -> Result<StatusCode, AppError> {
    require_game(&game)?;
    let folder = load_folder(&state, user.id, &game, folder_id).await?;
    DeckFolder::delete_by_id(folder.id).exec(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 409 if another folder for this `(user, game)` already has `name` (excluding `exclude_id`
/// on a rename). A belt in front of the unique index so an ordinary duplicate is a clean
/// 409; a rare concurrent double-submit that races past this check still surfaces the
/// index violation as a 500 (self-inflicted, no data corruption — the index holds).
async fn ensure_unique_name(
    state: &AppState,
    user_id: i32,
    game: &str,
    name: &str,
    exclude_id: Option<i32>,
) -> Result<(), AppError> {
    let existing = DeckFolder::find()
        .filter(deck_folder::Column::UserId.eq(user_id))
        .filter(deck_folder::Column::Game.eq(game))
        .filter(deck_folder::Column::Name.eq(name))
        .one(&state.db)
        .await?;
    if let Some(f) = existing {
        if Some(f.id) != exclude_id {
            return Err(AppError::Conflict(format!(
                "a folder named \"{name}\" already exists"
            )));
        }
    }
    Ok(())
}

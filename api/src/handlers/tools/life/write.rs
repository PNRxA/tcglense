//! Session lifecycle: start a game (from scratch or as a rematch), edit its label/layout,
//! record its result, and delete it. All take [`WritableUser`] (a read-only API key is `403`).

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set, TransactionTrait,
};

use crate::auth::extractor::WritableUser;
use crate::entities::prelude::{LifeSession, LifeSessionPlayer};
use crate::entities::{life_session, life_session_player};
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::{require_game, validate_name, validate_optional};
use crate::state::AppState;

use super::{
    CreateLifeSessionRequest, FinishLifeSessionRequest, LifeSeatInput, LifeSessionDetail,
    LifeSessionResponse, MAX_FORMAT, MAX_PLAYER_NAME, MAX_PLAYERS, MAX_SESSION_NAME,
    MAX_SESSIONS_PER_GAME, RESULT_DRAW, RESULT_LOSS, RESULT_NONE, RESULT_WIN, STATUS_ACTIVE,
    STATUS_FINISHED, UpdateLifeSessionRequest, deck_names_for, default_layout_for,
    default_rotation_for, load_session, require_active, resolve_deck_ref, seats_of, session_detail,
    session_response, validate_layout, validate_rotation, validate_starting_life,
};

/// The life total a game starts on when neither the request nor a copied session says.
const DEFAULT_STARTING_LIFE: i32 = 20;

/// A seat resolved from a request row (or copied from an earlier session), ready to insert.
pub(super) struct ResolvedSeat {
    pub name: String,
    pub deck_id: Option<i32>,
    pub starting_life: i32,
    pub rotation: i32,
}

/// What a seat falls back to when the request leaves a field out: the arrangement it's being
/// seated into, how big the table is (which is what decides a `facing` seat's rotation), and
/// the session's own starting life.
pub(super) struct SeatDefaults<'a> {
    pub layout: &'a str,
    pub player_count: usize,
    pub starting_life: i32,
}

/// Start a tracked game
///
/// `POST /api/tools/{game}/life/sessions` -> open a new game and return it in full.
///
/// Describe the table with `players` (each seat optionally named, given a starting life, a
/// rotation, and one of your decks), or pass `from_session_id` to **rematch**: the seats,
/// decks, rotations, starting life, layout and format of that earlier game are copied and a
/// fresh game begins on full life — which is what makes a per-deck record accumulate over an
/// evening without re-entering the pod each time. An explicit field always overrides the
/// copied one.
///
/// With no `layout`, the arrangement each player count is normally played in is chosen (two
/// players face each other, four sit around the table), and each seat is rotated to match.
#[utoipa::path(
    post,
    path = "/api/tools/{game}/life/sessions",
    tag = "Tools",
    security(("api_key" = [])),
    params(("game" = String, Path, description = "Game id slug, e.g. `mtg`")),
    request_body = CreateLifeSessionRequest,
    responses(
        (status = 200, description = "The newly started game, its seats and its (empty) history.", body = LifeSessionDetail),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, `from_session_id` is not the caller's, or a seat's `deck_id` is not one of their decks."),
        (status = 422, description = "No seats, too many seats, a bad layout/rotation/starting life, or over the per-game session cap."),
    ),
)]
pub async fn create_session(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<CreateLifeSessionRequest>,
) -> Result<Json<LifeSessionDetail>, AppError> {
    require_game(&game)?;

    // A rematch copies the table it names; anything the request states explicitly still wins.
    let source = match payload.from_session_id {
        Some(id) => Some(load_session(&state, user.id, &game, id).await?),
        None => None,
    };

    let name = validate_optional(payload.name, "name", MAX_SESSION_NAME)?;
    let format = validate_optional(payload.format, "format", MAX_FORMAT)?
        .or_else(|| source.as_ref().and_then(|s| s.format.clone()));
    let starting_life = match payload.starting_life {
        Some(life) => validate_starting_life(life)?,
        None => source
            .as_ref()
            .map(|s| s.starting_life)
            .unwrap_or(DEFAULT_STARTING_LIFE),
    };

    // Seats: the request's if it sent any, else the copied session's.
    let inputs: Vec<LifeSeatInput> = if payload.players.is_empty() {
        match &source {
            Some(source) => seats_of(&state.db, source.id)
                .await?
                .into_iter()
                .map(|seat| LifeSeatInput {
                    name: Some(seat.name),
                    deck_id: seat.deck_id,
                    starting_life: Some(seat.starting_life),
                    rotation: Some(seat.rotation),
                })
                .collect(),
            None => Vec::new(),
        }
    } else {
        payload.players
    };
    if inputs.is_empty() {
        return Err(AppError::Validation(
            "a game needs at least one player".to_string(),
        ));
    }
    if inputs.len() > MAX_PLAYERS {
        return Err(AppError::Validation(format!(
            "a game can have at most {MAX_PLAYERS} players"
        )));
    }

    let layout = match payload.layout {
        Some(layout) => validate_layout(&layout)?,
        None => source
            .as_ref()
            .map(|s| s.layout.clone())
            .unwrap_or_else(|| default_layout_for(inputs.len()).to_string()),
    };

    let count = LifeSession::find()
        .filter(life_session::Column::UserId.eq(user.id))
        .filter(life_session::Column::Game.eq(&game))
        .count(&state.db)
        .await?;
    if count >= MAX_SESSIONS_PER_GAME {
        return Err(AppError::Validation(format!(
            "you can track at most {MAX_SESSIONS_PER_GAME} games per game"
        )));
    }

    let defaults = SeatDefaults {
        layout: &layout,
        player_count: inputs.len(),
        starting_life,
    };
    let mut seats = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.into_iter().enumerate() {
        seats.push(resolve_seat(&state, user.id, &game, input, index, &defaults).await?);
    }

    // The session and its seats are one unit — a game with no seats would be unusable and
    // would have to be cleaned up by hand, so they commit or fail together.
    let now = Utc::now();
    let txn = state.db.begin().await?;
    let session = life_session::ActiveModel {
        user_id: Set(user.id),
        game: Set(game.clone()),
        name: Set(name),
        format: Set(format),
        starting_life: Set(starting_life),
        layout: Set(layout),
        status: Set(STATUS_ACTIVE.to_string()),
        started_at: Set(now),
        finished_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;

    let rows: Vec<life_session_player::ActiveModel> = seats
        .into_iter()
        .enumerate()
        .map(|(index, seat)| life_session_player::ActiveModel {
            session_id: Set(session.id),
            position: Set(index as i32),
            name: Set(seat.name),
            deck_id: Set(seat.deck_id),
            starting_life: Set(seat.starting_life),
            // Everyone starts on their own full total; the history begins empty.
            life: Set(seat.starting_life),
            rotation: Set(seat.rotation),
            result: Set(RESULT_NONE.to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        })
        .collect();
    LifeSessionPlayer::insert_many(rows).exec(&txn).await?;
    txn.commit().await?;

    Ok(Json(session_detail(&state, user.id, &session).await?))
}

/// Validate one requested seat, defaulting its name, starting life and rotation.
pub(super) async fn resolve_seat(
    state: &AppState,
    user_id: i32,
    game: &str,
    input: LifeSeatInput,
    position: usize,
    defaults: &SeatDefaults<'_>,
) -> Result<ResolvedSeat, AppError> {
    // An unnamed seat is "Player 3", not a blank tile — the counter has to read from across a
    // table, and a name is the only thing distinguishing two identical totals.
    let name = match input.name {
        Some(name) if !name.trim().is_empty() => {
            validate_name(&name, "player name", MAX_PLAYER_NAME)?
        }
        _ => format!("Player {}", position + 1),
    };
    let starting_life = match input.starting_life {
        Some(life) => validate_starting_life(life)?,
        None => defaults.starting_life,
    };
    let rotation = match input.rotation {
        Some(rotation) => validate_rotation(rotation)?,
        // Without an explicit rotation, seat the player the way the layout seats them.
        None => default_rotation_for(defaults.layout, position, defaults.player_count),
    };
    Ok(ResolvedSeat {
        name,
        deck_id: resolve_deck_ref(state, user_id, game, input.deck_id).await?,
        starting_life,
        rotation,
    })
}

/// Edit a tracked game
///
/// `PUT /api/tools/{game}/life/sessions/{session_id}` -> change the game's label, format or
/// seat layout. Each field is optional (absent = unchanged); a blank `name`/`format` clears it.
/// Works on a finished game too — relabelling history is not rewriting it.
#[utoipa::path(
    put,
    path = "/api/tools/{game}/life/sessions/{session_id}",
    tag = "Tools",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("session_id" = i32, Path, description = "Tracked-game id"),
    ),
    request_body = UpdateLifeSessionRequest,
    responses(
        (status = 200, description = "The updated game header and seats.", body = LifeSessionResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or the session is not the caller's."),
        (status = 422, description = "Oversized name/format, or an unknown layout."),
    ),
)]
pub async fn update_session(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, session_id)): Path<(String, i32)>,
    JsonBody(payload): JsonBody<UpdateLifeSessionRequest>,
) -> Result<Json<LifeSessionResponse>, AppError> {
    require_game(&game)?;
    let session = load_session(&state, user.id, &game, session_id).await?;

    let mut active: life_session::ActiveModel = session.into();
    if let Some(name) = payload.name {
        active.name = Set(validate_optional(Some(name), "name", MAX_SESSION_NAME)?);
    }
    if let Some(format) = payload.format {
        active.format = Set(validate_optional(Some(format), "format", MAX_FORMAT)?);
    }
    if let Some(layout) = payload.layout {
        active.layout = Set(validate_layout(&layout)?);
    }
    active.updated_at = Set(Utc::now());
    let session = active.update(&state.db).await?;

    let seats = seats_of(&state.db, session.id).await?;
    let deck_names = deck_names_for(&state.db, user.id, &game, &seats).await?;
    Ok(Json(session_response(&session, seats, &deck_names)))
}

/// Record the result
///
/// `POST /api/tools/{game}/life/sessions/{session_id}/finish` -> close the game out.
/// `winner_player_id` names the seat that won (every other seat is a loss); `null` records a
/// draw for the whole table. The session becomes `finished`, is stamped with `finished_at`,
/// and stops accepting edits — and from here on it counts towards the per-deck record.
///
/// Already finished is a `409`: a result that has been counted must be deleted rather than
/// quietly overwritten.
#[utoipa::path(
    post,
    path = "/api/tools/{game}/life/sessions/{session_id}/finish",
    tag = "Tools",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("session_id" = i32, Path, description = "Tracked-game id"),
    ),
    request_body = FinishLifeSessionRequest,
    responses(
        (status = 200, description = "The finished game, with a result on every seat.", body = LifeSessionDetail),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, the session is not the caller's, or the winner is not one of its seats."),
        (status = 409, description = "The game is already finished."),
    ),
)]
pub async fn finish_session(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, session_id)): Path<(String, i32)>,
    JsonBody(payload): JsonBody<FinishLifeSessionRequest>,
) -> Result<Json<LifeSessionDetail>, AppError> {
    require_game(&game)?;
    let session = load_session(&state, user.id, &game, session_id).await?;
    require_active(&session)?;

    let seats = seats_of(&state.db, session.id).await?;
    if let Some(winner) = payload.winner_player_id
        && !seats.iter().any(|seat| seat.id == winner)
    {
        return Err(AppError::NotFound("player not found".to_string()));
    }

    let now = Utc::now();
    let txn = state.db.begin().await?;
    for seat in seats {
        let result = match payload.winner_player_id {
            Some(winner) if seat.id == winner => RESULT_WIN,
            Some(_) => RESULT_LOSS,
            None => RESULT_DRAW,
        };
        let mut active: life_session_player::ActiveModel = seat.into();
        active.result = Set(result.to_string());
        active.updated_at = Set(now);
        active.update(&txn).await?;
    }
    let mut active: life_session::ActiveModel = session.into();
    active.status = Set(STATUS_FINISHED.to_string());
    active.finished_at = Set(Some(now));
    active.updated_at = Set(now);
    let session = active.update(&txn).await?;
    txn.commit().await?;

    Ok(Json(session_detail(&state, user.id, &session).await?))
}

/// Delete a tracked game
///
/// `DELETE /api/tools/{game}/life/sessions/{session_id}` -> remove the game; its seats and
/// its whole life history cascade away with it, and a finished game's contribution to the
/// per-deck record disappears with it.
#[utoipa::path(
    delete,
    path = "/api/tools/{game}/life/sessions/{session_id}",
    tag = "Tools",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("session_id" = i32, Path, description = "Tracked-game id"),
    ),
    responses(
        (status = 204, description = "Deleted."),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or the session is not the caller's."),
    ),
)]
pub async fn delete_session(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, session_id)): Path<(String, i32)>,
) -> Result<StatusCode, AppError> {
    require_game(&game)?;
    let session = load_session(&state, user.id, &game, session_id).await?;
    LifeSession::delete_by_id(session.id)
        .exec(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

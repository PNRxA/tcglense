//! Life-counter reads: the caller's tracked games, and one game in full.
//!
//! Both take [`AuthUser`] — a read-only API key can look at your play history, it just can't
//! move a life total.

use std::collections::HashMap;

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::auth::extractor::AuthUser;
use crate::entities::prelude::{LifeSession, LifeSessionPlayer};
use crate::entities::{life_session, life_session_player};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::{DataBody, require_game};
use crate::state::AppState;

use super::{
    DEFAULT_SESSION_LIMIT, LifeSessionDetail, LifeSessionResponse, ListSessionsParams,
    MAX_SESSION_LIMIT, STATUS_ACTIVE, STATUS_FINISHED, SeatRefs, load_session, session_detail,
    session_response,
};

/// List tracked games
///
/// `GET /api/tools/{game}/life/sessions` -> the caller's tracked games for the game,
/// most-recently-started first, each with its seats (so a list row can name who played what)
/// but without the life history — that's on the detail read.
///
/// `?status=active` narrows to games still in progress, which is how the tool's landing finds
/// a game to resume; `?status=finished` gives the play log. `?limit` is clamped to
/// `1..=200` (default 50). An unknown `status` value is a `422`.
#[utoipa::path(
    get,
    path = "/api/tools/{game}/life/sessions",
    tag = "Tools",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("status" = Option<String>, Query, description = "Narrow to `active` or `finished` games."),
        ("limit" = Option<u64>, Query, description = "How many to return, 1..=200 (default 50)."),
    ),
    responses(
        (status = 200, description = "The caller's tracked games, newest first.", body = DataBody<Vec<LifeSessionResponse>>),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "Unknown `status` value."),
    ),
)]
pub async fn list_sessions(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ListSessionsParams>,
) -> Result<Json<DataBody<Vec<LifeSessionResponse>>>, AppError> {
    require_game(&game)?;
    let limit = params
        .limit
        .unwrap_or(DEFAULT_SESSION_LIMIT)
        .clamp(1, MAX_SESSION_LIMIT);

    let mut query = LifeSession::find()
        .filter(life_session::Column::UserId.eq(user.id))
        .filter(life_session::Column::Game.eq(&game));
    if let Some(status) = params.status.as_deref() {
        if status != STATUS_ACTIVE && status != STATUS_FINISHED {
            return Err(AppError::Validation(format!(
                "status must be {STATUS_ACTIVE} or {STATUS_FINISHED}"
            )));
        }
        query = query.filter(life_session::Column::Status.eq(status));
    }
    let sessions = query
        .order_by_desc(life_session::Column::StartedAt)
        .order_by_desc(life_session::Column::Id)
        .limit(limit)
        .all(&state.db)
        .await?;

    if sessions.is_empty() {
        return Ok(Json(DataBody { data: Vec::new() }));
    }

    // One seat query for the whole page, then one deck-name query for every seat on it — so
    // the list costs three queries regardless of how many games it returns.
    let session_ids: Vec<i32> = sessions.iter().map(|s| s.id).collect();
    let seats = LifeSessionPlayer::find()
        .filter(life_session_player::Column::SessionId.is_in(session_ids))
        .order_by_asc(life_session_player::Column::Position)
        .order_by_asc(life_session_player::Column::Id)
        .all(&state.db)
        .await?;
    let refs = SeatRefs::resolve(&state.db, user.id, &game, &seats).await?;

    let mut by_session: HashMap<i32, Vec<life_session_player::Model>> = HashMap::new();
    for seat in seats {
        by_session.entry(seat.session_id).or_default().push(seat);
    }

    let data = sessions
        .iter()
        .map(|session| {
            let seats = by_session.remove(&session.id).unwrap_or_default();
            session_response(session, seats, &refs)
        })
        .collect();
    Ok(Json(DataBody { data }))
}

/// Get a tracked game
///
/// `GET /api/tools/{game}/life/sessions/{session_id}` -> one tracked game in full: its
/// header, every seat, and every recorded life change in the order it happened. A session
/// that isn't the caller's is a `404`, never a `403`.
#[utoipa::path(
    get,
    path = "/api/tools/{game}/life/sessions/{session_id}",
    tag = "Tools",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("session_id" = i32, Path, description = "Tracked-game id"),
    ),
    responses(
        (status = 200, description = "The tracked game, its seats and its full life history.", body = LifeSessionDetail),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or the session is not the caller's."),
    ),
)]
pub async fn get_session(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, session_id)): Path<(String, i32)>,
) -> Result<Json<LifeSessionDetail>, AppError> {
    require_game(&game)?;
    let session = load_session(&state, user.id, &game, session_id).await?;
    Ok(Json(session_detail(&state, user.id, &session).await?))
}

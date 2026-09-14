//! Room lifecycle: open a table, list the ones you're at, read one by its invite code, and
//! close it.
//!
//! Three of the four take [`SessionUser`] — a live table is an interactive SPA feature, so an
//! API key never opens or closes one (the same call `/api/alerts` makes). The fourth, the
//! read by code, is **public**: the join page has to show who is already sitting down before
//! the visitor has any credential at all.
//!
//! The delete is the one place the code's shareability bites. A non-host `DELETE` answers
//! **404**, not 403, so someone holding a code they can't act on learns nothing about whether
//! it names a real room — the same no-existence-oracle rule `load_deck` and `load_session`
//! follow for ids.

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, TransactionTrait,
};
use serde::Serialize;

use crate::auth::extractor::SessionUser;
use crate::entities::prelude::{PlayRoom, PlaySeat};
use crate::entities::{play_room, play_seat};
use crate::error::AppError;
use crate::extract::{JsonBody, Path, Query};
use crate::handlers::shared::{require_game, validate_optional};
use crate::play::types::{
    FORMATS, JoinResponse, MAX_PLAYERS, MIN_PLAYERS, RoomStatus, RoomSummary, default_starting_life,
};
use crate::state::AppState;

use super::tokens::{CODE_ATTEMPTS, generate_code, generate_seat_token};
use super::{
    CreateRoomRequest, DEFAULT_ROOM_LIMIT, ListRoomsParams, MAX_LABEL, MAX_OPEN_ROOMS_PER_HOST,
    MAX_ROOM_LIMIT, MAX_STARTING_LIFE, MIN_STARTING_LIFE, load_room, seat_view, summary_for,
    summary_from, validate_display_name,
};

/// The list wrapper, matching every other `{ data: [...] }` read in the app.
#[derive(Debug, Serialize)]
pub struct RoomListResponse {
    pub data: Vec<RoomSummary>,
}

/// The default seat count when the host doesn't say — a four-player Commander pod, which is
/// the table this tool exists for.
const DEFAULT_MAX_PLAYERS: i32 = 4;

/// Open a play table.
///
/// `POST /api/tools/{game}/play/rooms` -> `201` with the room, the host's seat (index 0) and
/// the seat token the socket needs. The host is seated immediately: a room with nobody in it
/// is not a thing you can share a link to.
pub async fn create_room(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Path(game): Path<String>,
    JsonBody(payload): JsonBody<CreateRoomRequest>,
) -> Result<Response, AppError> {
    require_game(&game)?;

    let label = validate_optional(payload.label, "label", MAX_LABEL)?.unwrap_or_default();
    let format = payload.format.trim().to_ascii_lowercase();
    let Some(default_life) = default_starting_life(&format) else {
        return Err(AppError::Validation(format!(
            "format must be one of {}",
            FORMATS.join(", ")
        )));
    };
    let starting_life = payload.starting_life.unwrap_or(default_life);
    if !(MIN_STARTING_LIFE..=MAX_STARTING_LIFE).contains(&starting_life) {
        return Err(AppError::Validation(format!(
            "starting life must be between {MIN_STARTING_LIFE} and {MAX_STARTING_LIFE}"
        )));
    }
    let max_players = payload.max_players.unwrap_or(DEFAULT_MAX_PLAYERS);
    if !(MIN_PLAYERS..=MAX_PLAYERS).contains(&max_players) {
        return Err(AppError::Validation(format!(
            "max_players must be between {MIN_PLAYERS} and {MAX_PLAYERS}"
        )));
    }

    // Each open room is an in-memory table and a set of live sockets, not just a row, so the
    // cap counts what is *open* rather than what was ever created — finishing or deleting a
    // game frees a slot.
    let open = PlayRoom::find()
        .filter(play_room::Column::HostUserId.eq(user.id))
        .filter(play_room::Column::Game.eq(game.as_str()))
        .filter(play_room::Column::Status.ne(RoomStatus::Finished.as_str()))
        .count(&state.db)
        .await?;
    if open >= MAX_OPEN_ROOMS_PER_HOST {
        return Err(AppError::Conflict(format!(
            "you already have {MAX_OPEN_ROOMS_PER_HOST} open tables; close one first"
        )));
    }

    // The host's seat is named after their handle; an account without one yet falls back to
    // a neutral label rather than refusing to open a table.
    let host_name = user
        .username
        .as_deref()
        .and_then(|name| validate_display_name(name).ok())
        .unwrap_or_else(|| "Host".to_string());

    let (token, token_hash) = generate_seat_token();
    let now = Utc::now();

    // The code is minted optimistically and the unique index is the arbiter: retrying on a
    // violation is cheaper (and race-free) compared with a read-then-insert that two hosts
    // can pass simultaneously.
    let mut last_err = None;
    for _ in 0..CODE_ATTEMPTS {
        let txn = state.db.begin().await?;
        let room = play_room::ActiveModel {
            code: Set(generate_code()),
            game: Set(game.clone()),
            host_user_id: Set(user.id),
            label: Set(label.clone()),
            format: Set(format.clone()),
            starting_life: Set(starting_life),
            max_players: Set(max_players),
            status: Set(RoomStatus::Lobby.as_str().to_string()),
            state: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await;
        let room = match room {
            Ok(room) => room,
            Err(err) => {
                let _ = txn.rollback().await;
                last_err = Some(err);
                continue;
            }
        };
        let seat = play_seat::ActiveModel {
            room_id: Set(room.id),
            seat_index: Set(0),
            user_id: Set(Some(user.id)),
            display_name: Set(host_name.clone()),
            token_hash: Set(token_hash.clone()),
            ready: Set(false),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        txn.commit().await?;

        let seats = vec![seat.clone()];
        let body = JoinResponse {
            room: summary_from(&state, &room, &seats),
            seat: seat_view(&room, &seat, false),
            seat_token: token,
        };
        return Ok((StatusCode::CREATED, Json(body)).into_response());
    }

    Err(AppError::Internal(format!(
        "could not mint a unique room code after {CODE_ATTEMPTS} attempts: {last_err:?}"
    )))
}

/// The tables you're at.
///
/// `GET /api/tools/{game}/play/rooms` -> rooms the caller hosts **or** holds a seat in, most
/// recently active first. The two halves are one query so a guest-turned-account and a host
/// see the same list.
pub async fn list_rooms(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Path(game): Path<String>,
    Query(params): Query<ListRoomsParams>,
) -> Result<Json<RoomListResponse>, AppError> {
    require_game(&game)?;
    let limit = params
        .limit
        .unwrap_or(DEFAULT_ROOM_LIMIT)
        .clamp(1, MAX_ROOM_LIMIT);

    let seated: Vec<i32> = PlaySeat::find()
        .select_only()
        .column(play_seat::Column::RoomId)
        .filter(play_seat::Column::UserId.eq(user.id))
        .into_tuple()
        .all(&state.db)
        .await?;

    let mut query = PlayRoom::find()
        .filter(play_room::Column::Game.eq(game.as_str()))
        .filter(
            Condition::any()
                .add(play_room::Column::HostUserId.eq(user.id))
                .add(play_room::Column::Id.is_in(seated)),
        );
    if let Some(status) = params.status.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let status = RoomStatus::parse(status)
            .ok_or_else(|| AppError::Validation("unknown room status".to_string()))?;
        query = query.filter(play_room::Column::Status.eq(status.as_str()));
    }

    let rooms = query
        .order_by_desc(play_room::Column::UpdatedAt)
        .order_by_desc(play_room::Column::Id)
        .limit(limit)
        .all(&state.db)
        .await?;

    let mut data = Vec::with_capacity(rooms.len());
    for room in &rooms {
        data.push(summary_for(&state, room).await?);
    }
    Ok(Json(RoomListResponse { data }))
}

/// One table by its invite code.
///
/// `GET /api/tools/{game}/play/rooms/{code}` -> the room and its seats. **Public**: the join
/// page has to render the table before the visitor has any credential. Nothing secret rides
/// the summary — a seat's token hash, its decklist and its hand are all elsewhere.
pub async fn get_room(
    State(state): State<AppState>,
    Path((game, code)): Path<(String, String)>,
) -> Result<Json<RoomSummary>, AppError> {
    require_game(&game)?;
    let room = load_room(&state, &game, &code).await?;
    Ok(Json(summary_for(&state, &room).await?))
}

/// Close a table.
///
/// `DELETE /api/tools/{game}/play/rooms/{code}` -> `204`. Host session only; **anyone else
/// (including a seated player) gets a 404**, so a shared code never confirms a room's
/// existence to someone who can't act on it. Every open socket is told why and closed.
pub async fn delete_room(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Path((game, code)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    require_game(&game)?;
    let room = load_room(&state, &game, &code).await?;
    if room.host_user_id != user.id {
        return Err(AppError::NotFound("room not found".to_string()));
    }

    PlayRoom::delete_by_id(room.id).exec(&state.db).await?;
    // Order matters: the row is gone before the sockets are told, so a client that races a
    // reconnect finds nothing rather than a half-deleted table.
    state.play.close_room(room.id, "the host closed this table");
    Ok(StatusCode::NO_CONTENT)
}

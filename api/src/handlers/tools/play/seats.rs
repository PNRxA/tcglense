//! Seats: taking one, loading a deck into it, saying you're ready, and leaving.
//!
//! These are the **guest-facing** half of the tool, and they are the reason `MaybeUser`
//! exists. A seat may be backed by an account or by nothing at all, so authorization here is
//! the per-seat token in `X-Play-Seat` ([`super::tokens`]) rather than a session — and the
//! one thing a session *does* buy you is continuity: a signed-in joiner is recognised by
//! `user_id`, so re-joining from another device re-takes the same chair (with a fresh token)
//! instead of eating a second one.
//!
//! Every write here ends with a [`push_lobby`](crate::handlers::tools::play::registry::PlayRegistry::push_lobby),
//! so the lobby page never polls: whoever is watching the room socket sees the seat appear,
//! the deck land or the chair empty as it happens.
//!
//! [`MaybeUser`]: crate::auth::extractor::MaybeUser

use axum::{Json, extract::State, http::HeaderMap, http::StatusCode};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait};

use crate::auth::extractor::MaybeUser;
use crate::entities::prelude::PlaySeat;
use crate::entities::{play_room, play_seat};
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::require_game;
use crate::play::types::{JoinResponse, SeatView};
use crate::state::AppState;

use super::registry::touch_room;
use super::tokens::{authorize_seat, generate_seat_token, seat_by_token, seat_token_from};
use super::{
    JoinRequest, LoadDeckRequest, SetReadyRequest, load_room, load_room_on, require_lobby,
    seat_view, seats_of, summary_from, validate_display_name,
};

/// Take (or re-take) a seat at a table.
///
/// `POST /api/tools/{game}/play/rooms/{code}/join`. Public with optional auth — a guest sends
/// a `name`, a signed-in caller may send nothing at all.
///
/// Resolution is in a fixed order, and the order is the point:
/// 1. a `seat_token` that matches a seat of this room re-takes **that** seat, token unchanged
///    (a reload must not cost you your chair);
/// 2. otherwise, a signed-in caller who already holds a seat re-takes it with a **rotated**
///    token (a new device, having lost the old token — rotating means the old device's token
///    stops working, which is what makes "kick my other tab" possible);
/// 3. otherwise a new seat, which needs the room to still be a lobby, a free chair, and a name.
pub async fn join_room(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    Path((game, code)): Path<(String, String)>,
    JsonBody(payload): JsonBody<JoinRequest>,
) -> Result<Json<JoinResponse>, AppError> {
    require_game(&game)?;
    let room = load_room(&state, &game, &code).await?;

    // (1) A token we issued: hand the same seat back untouched.
    if let Some(token) = payload.seat_token.as_deref().map(str::trim).filter(|t| !t.is_empty())
        && let Some(seat) = seat_by_token(&state.db, room.id, token).await?
    {
        return Ok(Json(join_response(&state, &room, seat, token.to_string()).await?));
    }

    // (2) A signed-in caller who already sits here: same chair, fresh token.
    if let Some(user) = user.as_ref() {
        let existing = PlaySeat::find()
            .filter(play_seat::Column::RoomId.eq(room.id))
            .filter(play_seat::Column::UserId.eq(user.id))
            .one(&state.db)
            .await?;
        if let Some(seat) = existing {
            let (token, token_hash) = generate_seat_token();
            let seat = play_seat::ActiveModel {
                id: Set(seat.id),
                token_hash: Set(token_hash),
                ..Default::default()
            }
            .update(&state.db)
            .await?;
            return Ok(Json(join_response(&state, &room, seat, token).await?));
        }
    }

    // (3) A new seat. The name rules differ by caller: a guest must give one, a signed-in
    // caller falls back to their username — but an explicit name always wins, so two accounts
    // at the same table can still be told apart by table name rather than handle.
    let name = match payload.name.as_deref() {
        Some(name) => validate_display_name(name)?,
        None => user
            .as_ref()
            .and_then(|u| u.username.as_deref())
            .map(validate_display_name)
            .transpose()?
            .ok_or_else(|| AppError::Validation("name is required".to_string()))?,
    };

    let txn = state.db.begin().await?;
    // Re-read inside the transaction: the lobby gate and the seat count must be evaluated
    // against the state this insert lands in, not the one the read above saw.
    let room = load_room_on(&txn, &game, &code).await?;
    require_lobby(&room)?;
    let seats = seats_of(&txn, room.id).await?;
    if i32::try_from(seats.len()).unwrap_or(i32::MAX) >= room.max_players {
        let _ = txn.rollback().await;
        return Err(AppError::Conflict("this table is full".to_string()));
    }
    // The lowest free chair, so a table that lost its middle seat fills the gap rather than
    // growing past `max_players` on indices alone.
    let taken: Vec<i32> = seats.iter().map(|seat| seat.seat_index).collect();
    let seat_index = (0..room.max_players)
        .find(|index| !taken.contains(index))
        .ok_or_else(|| AppError::Conflict("this table is full".to_string()))?;

    let (token, token_hash) = generate_seat_token();
    let seat = play_seat::ActiveModel {
        room_id: Set(room.id),
        seat_index: Set(seat_index),
        user_id: Set(user.as_ref().map(|u| u.id)),
        display_name: Set(name),
        token_hash: Set(token_hash),
        ready: Set(false),
        created_at: Set(Utc::now()),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    touch_room(&txn, room.id).await?;
    txn.commit().await?;

    Ok(Json(join_response(&state, &room, seat, token).await?))
}

/// Load a deck into a seat.
///
/// `POST .../seats/{seat_id}/deck` (seat token). Lobby only — a decklist is resolved into
/// card definitions once, here, so that starting the game needs no catalog query.
/// `source: "deck"` additionally needs the caller's session, because "one of my decks" is
/// meaningless for a guest seat. Loading always clears `ready`: you readied for the old list.
pub async fn load_seat_deck(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    headers: HeaderMap,
    Path((game, code, seat_id)): Path<(String, String, i32)>,
    JsonBody(payload): JsonBody<LoadDeckRequest>,
) -> Result<Json<SeatView>, AppError> {
    require_game(&game)?;
    let room = load_room(&state, &game, &code).await?;
    require_lobby(&room)?;
    let seat = authorize_seat(
        &state.db,
        room.id,
        seat_id,
        seat_token_from(&headers).as_deref(),
    )
    .await?;

    let resolved = super::decks::resolve_deck(&state, &game, &room, &seat, user.as_ref(), payload)
        .await?;
    let deck_json = serde_json::to_string(&resolved.cards)
        .map_err(|err| AppError::Internal(format!("failed to encode a play decklist: {err}")))?;

    let seat = play_seat::ActiveModel {
        id: Set(seat.id),
        deck_source: Set(Some(resolved.source.to_string())),
        deck_ref: Set(resolved.deck_ref),
        deck_name: Set(Some(resolved.name)),
        deck_json: Set(Some(deck_json)),
        // A new list means the old "ready" said nothing about this one.
        ready: Set(false),
        ..Default::default()
    }
    .update(&state.db)
    .await?;

    touch_room(&state.db, room.id).await?;
    let view = seat_view(&room, &seat, state.play.connected_seats(room.id).contains(&seat.id));
    push_lobby(&state, &room).await?;
    Ok(Json(view))
}

/// `POST .../seats/{seat_id}/ready` (seat token) -> the seat.
///
/// Readying without a deck is a `422`: "ready" is a promise about a list, so there has to be
/// one. Un-readying is always allowed.
pub async fn set_seat_ready(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((game, code, seat_id)): Path<(String, String, i32)>,
    JsonBody(payload): JsonBody<SetReadyRequest>,
) -> Result<Json<SeatView>, AppError> {
    require_game(&game)?;
    let room = load_room(&state, &game, &code).await?;
    require_lobby(&room)?;
    let seat = authorize_seat(
        &state.db,
        room.id,
        seat_id,
        seat_token_from(&headers).as_deref(),
    )
    .await?;

    if payload.ready && seat.deck_json.is_none() {
        return Err(AppError::Validation(
            "load a deck before readying up".to_string(),
        ));
    }

    let seat = play_seat::ActiveModel {
        id: Set(seat.id),
        ready: Set(payload.ready),
        ..Default::default()
    }
    .update(&state.db)
    .await?;

    touch_room(&state.db, room.id).await?;
    let view = seat_view(&room, &seat, state.play.connected_seats(room.id).contains(&seat.id));
    push_lobby(&state, &room).await?;
    Ok(Json(view))
}

/// Leave a seat, or (as the host) remove someone else's.
///
/// `DELETE .../seats/{seat_id}` -> `204`. Authorized by **either** that seat's token or the
/// host's session, which is the shape a real table has: you can get up, and the person whose
/// table it is can ask you to. Lobby only, and the host may not remove *their own* seat —
/// a table with no host is nobody's, so the honest action is deleting the room.
pub async fn leave_seat(
    State(state): State<AppState>,
    MaybeUser(user): MaybeUser,
    headers: HeaderMap,
    Path((game, code, seat_id)): Path<(String, String, i32)>,
) -> Result<StatusCode, AppError> {
    require_game(&game)?;
    let room = load_room(&state, &game, &code).await?;
    require_lobby(&room)?;

    let is_host = user.as_ref().is_some_and(|u| u.id == room.host_user_id);
    let seat = if is_host {
        PlaySeat::find_by_id(seat_id)
            .filter(play_seat::Column::RoomId.eq(room.id))
            .one(&state.db)
            .await?
            .ok_or_else(|| AppError::NotFound("seat not found".to_string()))?
    } else {
        authorize_seat(
            &state.db,
            room.id,
            seat_id,
            seat_token_from(&headers).as_deref(),
        )
        .await?
    };

    if seat.user_id == Some(room.host_user_id) {
        return Err(AppError::Conflict(
            "the host can't leave their own table; close it instead".to_string(),
        ));
    }

    PlaySeat::delete_by_id(seat.id).exec(&state.db).await?;
    touch_room(&state.db, room.id).await?;
    state
        .play
        .close_seat(room.id, seat.id, "you were removed from this table");
    push_lobby(&state, &room).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Helpers ----------

/// Re-read the room's seats and push the fresh lobby state at everyone watching.
async fn push_lobby(state: &AppState, room: &play_room::Model) -> Result<(), AppError> {
    let seats = seats_of(&state.db, room.id).await?;
    state
        .play
        .push_lobby(room.id, summary_from(state, room, &seats));
    Ok(())
}

/// The `JoinResponse` every join path answers with: the room as everyone sees it, the
/// caller's seat, and the token that seat is held by.
async fn join_response(
    state: &AppState,
    room: &play_room::Model,
    seat: play_seat::Model,
    token: String,
) -> Result<JoinResponse, AppError> {
    let seats = seats_of(&state.db, room.id).await?;
    let summary = summary_from(state, room, &seats);
    let connected = state.play.connected_seats(room.id);
    let view = seat_view(room, &seat, connected.contains(&seat.id));
    state.play.push_lobby(room.id, summary.clone());
    Ok(JoinResponse {
        room: summary,
        seat: view,
        seat_token: token,
    })
}

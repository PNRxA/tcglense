//! The life history: record a change, and undo one.
//!
//! [`adjust_life`] is the hot path — every tap on the counter ends up here — so it does the
//! least work of anything in this module and answers with just the seat and the event it
//! created, not the whole game. The client accumulates a run of taps and commits **one**
//! delta, which is why the history reads as "lost 5" rather than five "lost 1" rows.
//!
//! [`undo_life_event`] is the cold path and does the careful thing instead: it removes the row
//! and re-folds the seat's remaining chain through [`replay`](super::replay::replay), so
//! undoing a change from the middle of a game leaves every later total correct rather than
//! only fixing the number at the end.

use axum::{Json, extract::State};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
    TransactionTrait,
};

use crate::auth::extractor::WritableUser;
use crate::entities::prelude::LifeEvent;
use crate::entities::{life_event, life_session_player};
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::require_game;
use crate::state::AppState;

use super::replay::{ReplayEvent, clamp_life, replay};
use super::{
    AdjustLifeRequest, KIND_ADJUST, KIND_SET, LifeChange, LifeEventResponse, LifeSessionDetail,
    MAX_EVENTS_PER_SESSION, load_seat, load_seat_on, load_session, load_session_on, require_active,
    seat_response, session_detail, touch_session, validate_delta, validate_life,
};

/// Which of the two forms the request asked for, once validated — resolved before the write
/// transaction opens, applied against the seat total read inside it.
enum Change {
    /// A relative change: a run of taps committed as one delta.
    Adjust(i32),
    /// An absolute correction, which pins the total whatever it was before.
    Set(i32),
}

/// Change a life total
///
/// `POST /api/tools/{game}/life/sessions/{session_id}/players/{player_id}/life` -> move a
/// seat's life and record it in the history.
///
/// Send **exactly one** of:
/// - `delta` — a relative change (`-3`). This is what a run of taps commits, so one history row
///   describes the whole hit rather than three rows describing one point each.
/// - `life` — an absolute correction (`31`), for when the total on screen is simply wrong. The
///   history still records how far that moved the seat, so the chart stays continuous.
///
/// Totals clamp to the storable range, and the recorded `delta` is the movement that actually
/// happened — a tap at the floor records `0`, not a phantom loss. A finished game is a `409`.
#[utoipa::path(
    post,
    path = "/api/tools/{game}/life/sessions/{session_id}/players/{player_id}/life",
    tag = "Tools",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("session_id" = i32, Path, description = "Tracked-game id"),
        ("player_id" = i32, Path, description = "Seat id"),
    ),
    request_body = AdjustLifeRequest,
    responses(
        (status = 200, description = "The seat as it now stands, and the event that moved it.", body = LifeChange),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or the session/seat is not the caller's."),
        (status = 409, description = "The game is finished."),
        (status = 422, description = "Neither or both of `delta`/`life`, an out-of-range value, or the session's history cap is full."),
    ),
)]
pub async fn adjust_life(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, session_id, player_id)): Path<(String, i32, i32)>,
    JsonBody(payload): JsonBody<AdjustLifeRequest>,
) -> Result<Json<LifeChange>, AppError> {
    require_game(&game)?;
    let session = load_session(&state, user.id, &game, session_id).await?;
    require_active(&session)?;
    load_seat(&state, session.id, player_id).await?;

    // Exactly one of the two forms, validated before any lock is taken. Accepting both would
    // leave the server guessing which the user meant, and accepting neither would append an
    // empty history row.
    let change = match (payload.delta, payload.life) {
        (Some(delta), None) => Change::Adjust(validate_delta(delta)?),
        (None, Some(life)) => Change::Set(validate_life(life)?),
        _ => {
            return Err(AppError::Validation(
                "send exactly one of delta or life".to_string(),
            ));
        }
    };

    let now = Utc::now();
    let txn = state.db.begin().await?;
    // Serialize every write against this game through the parent session row, the way
    // `decks::cards` serializes card writes through the deck row: it takes SQLite's single-writer
    // lock (and Postgres' row lock) before the reads below, so a delta can't be computed against
    // a total another request is in the middle of moving, and the finished-game gate can't be
    // passed by a request that a concurrent `finish` is about to invalidate.
    touch_session(&txn, session.id, now).await?;
    let session = load_session_on(&txn, user.id, &game, session_id).await?;
    require_active(&session)?;
    let seat = load_seat_on(&txn, session.id, player_id).await?;

    let (life_after, kind) = match change {
        Change::Adjust(delta) => (
            clamp_life(i64::from(seat.life) + i64::from(delta)),
            KIND_ADJUST,
        ),
        Change::Set(life) => (life, KIND_SET),
    };
    // The delta stored is the movement that actually happened after clamping, so the history
    // never claims a change the total didn't make.
    let delta = life_after - seat.life;

    let events = LifeEvent::find()
        .filter(life_event::Column::SessionId.eq(session.id))
        .count(&txn)
        .await?;
    if events >= MAX_EVENTS_PER_SESSION {
        return Err(AppError::Validation(format!(
            "this game has reached its {MAX_EVENTS_PER_SESSION}-change history limit"
        )));
    }

    let event = life_event::ActiveModel {
        session_id: Set(session.id),
        player_id: Set(seat.id),
        delta: Set(delta),
        life_after: Set(life_after),
        kind: Set(kind.to_string()),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    let mut active: life_session_player::ActiveModel = seat.into();
    active.life = Set(life_after);
    active.updated_at = Set(now);
    let seat = active.update(&txn).await?;
    txn.commit().await?;

    Ok(Json(LifeChange {
        player: seat_response(&state, user.id, &game, seat).await?,
        event: LifeEventResponse::from(event),
    }))
}

/// Undo a life change
///
/// `DELETE /api/tools/{game}/life/sessions/{session_id}/events/{event_id}` -> remove one
/// recorded change, from anywhere in the game's history, and re-derive the affected seat.
///
/// Any event may be removed, not just the newest: the seat's remaining chain is re-folded from
/// its starting life, so a mis-tap discovered three turns later is undone correctly rather than
/// leaving every total after it off by the same amount. Relative changes shift; an absolute
/// correction still pins its own total, and only its own reported delta moves. Returns the
/// whole game, since the re-fold can change many rows.
#[utoipa::path(
    delete,
    path = "/api/tools/{game}/life/sessions/{session_id}/events/{event_id}",
    tag = "Tools",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("session_id" = i32, Path, description = "Tracked-game id"),
        ("event_id" = i32, Path, description = "Life-change id"),
    ),
    responses(
        (status = 200, description = "The game with the change removed and the seat re-derived.", body = LifeSessionDetail),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or the session/event is not the caller's."),
        (status = 409, description = "The game is finished."),
    ),
)]
pub async fn undo_life_event(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, session_id, event_id)): Path<(String, i32, i32)>,
) -> Result<Json<LifeSessionDetail>, AppError> {
    require_game(&game)?;
    let session = load_session(&state, user.id, &game, session_id).await?;
    require_active(&session)?;

    let now = Utc::now();
    let txn = state.db.begin().await?;
    // Under the session lock, as in `adjust_life` — and here it is load-bearing rather than
    // merely tidy: the fold below rebuilds the seat's total from the events it can see, so a tap
    // that committed between an unlocked read and this write would be absent from `remaining`
    // and silently reverted by the re-fold.
    touch_session(&txn, session.id, now).await?;
    let session = load_session_on(&txn, user.id, &game, session_id).await?;
    require_active(&session)?;

    // Scoped to the session whose ownership was just proved, so an event id from another
    // user's game is a 404 like everything else here.
    let event = LifeEvent::find_by_id(event_id)
        .filter(life_event::Column::SessionId.eq(session.id))
        .one(&txn)
        .await?
        .ok_or_else(|| AppError::NotFound("life change not found".to_string()))?;
    let seat = load_seat_on(&txn, session.id, event.player_id).await?;

    let remaining = LifeEvent::find()
        .filter(life_event::Column::PlayerId.eq(seat.id))
        .filter(life_event::Column::Id.ne(event.id))
        .order_by_asc(life_event::Column::Id)
        .all(&txn)
        .await?;
    let folded = replay(
        seat.starting_life,
        &remaining
            .iter()
            .map(|e| ReplayEvent::from_row(&e.kind, e.delta, e.life_after))
            .collect::<Vec<_>>(),
    );

    LifeEvent::delete_by_id(event.id).exec(&txn).await?;
    for (row, (delta, life_after)) in remaining.iter().zip(folded.events.iter()) {
        // Only the rows the fold actually moved are written.
        if row.delta == *delta && row.life_after == *life_after {
            continue;
        }
        life_event::ActiveModel {
            id: Set(row.id),
            delta: Set(*delta),
            life_after: Set(*life_after),
            ..Default::default()
        }
        .update(&txn)
        .await?;
    }
    let mut active: life_session_player::ActiveModel = seat.into();
    active.life = Set(folded.life);
    active.updated_at = Set(now);
    active.update(&txn).await?;
    txn.commit().await?;

    Ok(Json(session_detail(&state, user.id, &session).await?))
}

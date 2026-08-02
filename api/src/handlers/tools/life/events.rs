//! The history: record a change, and undo one.
//!
//! [`adjust_life`] is the hot path — every tap on the counter ends up here — so it does the
//! least work of anything in this module and answers with just the seat and the event it
//! created, not the whole game. The client accumulates a run of taps and commits **one**
//! delta, which is why the history reads as "lost 5" rather than five "lost 1" rows.
//!
//! [`undo_life_event`] is the cold path and does the careful thing instead: it removes the row
//! and re-folds the seat's remaining chains through
//! [`replay_seat`](super::replay::replay_seat), so undoing a change from the middle of a game
//! leaves every later total correct rather than only fixing the number at the end.
//!
//! Both routes serve **every** counter, not just life (issue #595). The counter axis rides the
//! request rather than getting endpoints of its own, because a poison tap and a life tap are
//! the same operation on the same history — and because that is what keeps the
//! finished-game gate, the per-session change cap, the session lock and the undo contract
//! written once. The one asymmetry is the seat's denormalised `life` column: **only** a `life`
//! change writes it, so a commander-damage tap never moves a life total as a side effect. That
//! is deliberate — at a real table the player reconciles the two, and doing it for them would
//! double-count every hit that was already tapped in.

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

use super::counters::{
    COUNTER_LIFE, has_source, parse_counters, require_enabled, start_value, validate_counter,
    validate_value,
};
use super::replay::{clamp_value, replay_event, replay_seat};
use super::{
    AdjustLifeRequest, KIND_ADJUST, KIND_SET, LifeChange, LifeCounterResponse, LifeEventResponse,
    LifeSessionDetail, MAX_EVENTS_PER_SESSION, load_seat, load_seat_on, load_session,
    load_session_on, require_active, seat_response, session_detail, touch_session, validate_delta,
};

/// Which of the two forms the request asked for, once validated — resolved before the write
/// transaction opens, applied against the counter's value read inside it.
enum Change {
    /// A relative change: a run of taps committed as one delta.
    Adjust(i32),
    /// An absolute correction, which pins the value whatever it was before.
    Set(i32),
}

/// Resolve and validate the request's counter axis: which counter it moves, and (for commander
/// damage) which seat's commander dealt it.
///
/// The source is **required** for commander damage and **refused** for everything else, both as
/// a `422`: 21 damage is per commander, so a sourceless damage row would be a number that can't
/// decide anything, and a source on a poison row would be state nothing reads. A source that
/// isn't a seat of this game is a `404`, like every other id here; a seat sourcing damage to
/// *itself* is a `422` — a player's own commander doesn't deal them commander damage, and
/// accepting it would put a phantom lethal source on the mat.
async fn resolve_counter(
    state: &AppState,
    session: &crate::entities::life_session::Model,
    player_id: i32,
    payload: &AdjustLifeRequest,
) -> Result<(&'static str, Option<i32>), AppError> {
    let counter = validate_counter(payload.counter.as_deref())?;
    require_enabled(&parse_counters(&session.counters), counter)?;

    if !has_source(counter) {
        if payload.source_player_id.is_some() {
            return Err(AppError::Validation(format!(
                "source_player_id is only meaningful for commander_damage, not {counter}"
            )));
        }
        return Ok((counter, None));
    }
    let Some(source) = payload.source_player_id else {
        return Err(AppError::Validation(
            "commander_damage needs a source_player_id — 21 damage is counted per commander"
                .to_string(),
        ));
    };
    if source == player_id {
        return Err(AppError::Validation(
            "a seat's own commander doesn't deal it commander damage".to_string(),
        ));
    }
    load_seat(state, session.id, source).await?;
    Ok((counter, Some(source)))
}

/// The value `counter` currently stands at for this seat.
///
/// Life is read off the seat row — it is the one counter that is denormalised, and reading it
/// anywhere else would make this a second derivation of a number that already has an owner.
/// Every other counter is the `life_after` of its chain's newest event, which the undo's re-fold
/// keeps correct, so the chain never has to be replayed to append to it.
async fn current_value<C: sea_orm::ConnectionTrait>(
    db: &C,
    seat: &life_session_player::Model,
    counter: &str,
    source_player_id: Option<i32>,
) -> Result<i32, AppError> {
    if counter == COUNTER_LIFE {
        return Ok(seat.life);
    }
    let mut query = LifeEvent::find()
        .filter(life_event::Column::PlayerId.eq(seat.id))
        .filter(life_event::Column::Counter.eq(counter));
    query = match source_player_id {
        Some(source) => query.filter(life_event::Column::SourcePlayerId.eq(source)),
        None => query.filter(life_event::Column::SourcePlayerId.is_null()),
    };
    let latest = query
        .order_by_desc(life_event::Column::Id)
        .one(db)
        .await?
        .map(|row| row.life_after);
    Ok(latest.unwrap_or_else(|| start_value(counter, seat.starting_life)))
}

/// Change a life total or counter
///
/// `POST /api/tools/{game}/life/sessions/{session_id}/players/{player_id}/life` -> move one of a
/// seat's numbers and record it in the history.
///
/// Send **exactly one** of:
/// - `delta` — a relative change (`-3`). This is what a run of taps commits, so one history row
///   describes the whole hit rather than three rows describing one point each.
/// - `life` — an absolute correction (`31`), for when the value on screen is simply wrong. The
///   history still records how far that moved the seat, so the chart stays continuous.
///
/// `counter` picks which number moves — absent means `life`, so a client that predates counters
/// is unaffected. `commander_damage` additionally needs `source_player_id` (the seat whose
/// commander dealt it); every other counter refuses one. A counter the game isn't tracking is a
/// `422` — turn it on for the session first.
///
/// Values clamp to the counter's own range (life may go negative, nothing else can), and the
/// recorded `delta` is the movement that actually happened — a tap at the floor records `0`, not
/// a phantom loss. A commander-damage tap deliberately does **not** move the target's life: it
/// is a separate counter that the player reconciles against life, exactly as at a real table.
/// A finished game is a `409`.
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
        (status = 200, description = "The seat as it now stands, the event that moved it, and (for a non-life counter) where that counter now stands.", body = LifeChange),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or the session/seat/`source_player_id` is not the caller's."),
        (status = 409, description = "The game is finished."),
        (status = 422, description = "Neither or both of `delta`/`life`, an unknown or untracked `counter`, a missing/forbidden/self `source_player_id`, an out-of-range value, or the session's history cap is full."),
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
    let (counter, source_player_id) =
        resolve_counter(&state, &session, player_id, &payload).await?;

    // Exactly one of the two forms, validated before any lock is taken. Accepting both would
    // leave the server guessing which the user meant, and accepting neither would append an
    // empty history row.
    let change = match (payload.delta, payload.life) {
        (Some(delta), None) => Change::Adjust(validate_delta(delta)?),
        (None, Some(life)) => Change::Set(validate_value(counter, life)?),
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
    let before = current_value(&txn, &seat, counter, source_player_id).await?;

    let (life_after, kind) = match change {
        Change::Adjust(delta) => (
            clamp_value(counter, i64::from(before) + i64::from(delta)),
            KIND_ADJUST,
        ),
        Change::Set(life) => (life, KIND_SET),
    };
    // The delta stored is the movement that actually happened after clamping, so the history
    // never claims a change the value didn't make.
    let delta = life_after - before;

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
        counter: Set(counter.to_string()),
        source_player_id: Set(source_player_id),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    let mut active: life_session_player::ActiveModel = seat.into();
    // The seat's `life` column is written here and in the undo's re-fold, and nowhere else — so
    // a counter change touches only the seat's timestamp. A commander-damage tap that also moved
    // life would double-count every hit the player had already tapped in.
    if counter == COUNTER_LIFE {
        active.life = Set(life_after);
    }
    active.updated_at = Set(now);
    let seat = active.update(&txn).await?;
    txn.commit().await?;

    Ok(Json(LifeChange {
        player: seat_response(&state, user.id, &game, seat).await?,
        // Non-life counters live nowhere but the history, so the hot-path response carries the
        // new value: a client can patch its state from this alone, the way it already swaps in
        // the seat, instead of re-reading the game after every tap.
        counter: (counter != COUNTER_LIFE).then(|| LifeCounterResponse {
            player_id: event.player_id,
            counter: counter.to_string(),
            source_player_id,
            value: life_after,
        }),
        event: LifeEventResponse::from(event),
    }))
}

/// Undo a change
///
/// `DELETE /api/tools/{game}/life/sessions/{session_id}/events/{event_id}` -> remove one
/// recorded change, from anywhere in the game's history, and re-derive the affected seat.
///
/// Any event may be removed, not just the newest: the seat's remaining chains are re-folded from
/// their starting values, so a mis-tap discovered three turns later is undone correctly rather
/// than leaving every value after it off by the same amount. Relative changes shift; an absolute
/// correction still pins its own value, and only its own reported delta moves. Chains are
/// independent, so undoing a poison tap leaves life — and damage from every *other* commander —
/// exactly where they were. Returns the whole game, since the re-fold can change many rows.
///
/// Allowed for any counter the history holds, including one the game has since stopped
/// tracking: hiding a counter must not strand its rows.
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
    let folded = replay_seat(
        seat.starting_life,
        &remaining.iter().map(replay_event).collect::<Vec<_>>(),
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
    // The second of the two places `life` is ever written. The fold returns the seat's life
    // chain specifically, so undoing a poison or commander-damage row writes back the same
    // number it already held rather than a value derived from the wrong chain.
    active.life = Set(folded.life);
    active.updated_at = Set(now);
    active.update(&txn).await?;
    txn.commit().await?;

    Ok(Json(session_detail(&state, user.id, &session).await?))
}

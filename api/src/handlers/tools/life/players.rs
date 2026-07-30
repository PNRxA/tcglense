//! Seat management: add a player mid-game, rename/relink/rotate one, remove one, and reorder
//! the table. All take [`WritableUser`], and all require the session to still be `active` — a
//! finished game's seats are what the per-deck record was computed from.
//!
//! A seat has no `user_id`, so every handler here loads the parent session first
//! ([`load_session`]) and only then the seat within it ([`load_seat`]) — a seat id from
//! someone else's game is a `404`.

use axum::{Json, extract::State};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, EntityTrait, Set, TransactionTrait};

use crate::auth::extractor::WritableUser;
use crate::entities::life_session_player;
use crate::entities::prelude::LifeSessionPlayer;
use crate::error::AppError;
use crate::extract::{JsonBody, Path};
use crate::handlers::shared::{require_game, validate_name};
use crate::state::AppState;

use super::write::{SeatDefaults, resolve_seat};
use super::{
    LifeSeatInput, LifeSeatResponse, LifeSessionDetail, MAX_PLAYER_NAME, MAX_PLAYERS, RESULT_NONE,
    ReorderLifeSeatsRequest, UpdateLifeSeatRequest, load_seat, load_session, require_active,
    require_single_link, resolve_commander_ref, resolve_deck_ref, seat_response, seats_of,
    session_detail, touch_session, validate_rotation,
};

/// Add a player
///
/// `POST /api/tools/{game}/life/sessions/{session_id}/players` -> seat another player at the
/// table (someone joined the pod). They start on their own full life, appended after the last
/// seat, and the game is returned in full since its shape changed.
#[utoipa::path(
    post,
    path = "/api/tools/{game}/life/sessions/{session_id}/players",
    tag = "Tools",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("session_id" = i32, Path, description = "Tracked-game id"),
    ),
    request_body = LifeSeatInput,
    responses(
        (status = 200, description = "The game with the new seat added.", body = LifeSessionDetail),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, the session is not the caller's, `deck_id` is not one of their decks, or `commander_card_id` is not a card in the catalog."),
        (status = 409, description = "The game is finished."),
        (status = 422, description = "Too many players, both a deck and a commander, or a bad rotation/starting life."),
    ),
)]
pub async fn add_player(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, session_id)): Path<(String, i32)>,
    JsonBody(payload): JsonBody<LifeSeatInput>,
) -> Result<Json<LifeSessionDetail>, AppError> {
    require_game(&game)?;
    let session = load_session(&state, user.id, &game, session_id).await?;
    require_active(&session)?;

    let seats = seats_of(&state.db, session.id).await?;
    if seats.len() >= MAX_PLAYERS {
        return Err(AppError::Validation(format!(
            "a game can have at most {MAX_PLAYERS} players"
        )));
    }
    let position = seats.len();
    let defaults = SeatDefaults {
        layout: &session.layout,
        player_count: position + 1,
        starting_life: session.starting_life,
    };
    let seat = resolve_seat(&state, user.id, &game, payload, position, &defaults).await?;

    let now = Utc::now();
    let txn = state.db.begin().await?;
    life_session_player::ActiveModel {
        session_id: Set(session.id),
        position: Set(position as i32),
        name: Set(seat.name),
        deck_id: Set(seat.deck_id),
        commander_card_id: Set(seat.commander_card_id),
        starting_life: Set(seat.starting_life),
        life: Set(seat.starting_life),
        rotation: Set(seat.rotation),
        result: Set(RESULT_NONE.to_string()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    touch_session(&txn, session.id, now).await?;
    txn.commit().await?;

    Ok(Json(session_detail(&state, user.id, &session).await?))
}

/// Edit a player
///
/// `PUT /api/tools/{game}/life/sessions/{session_id}/players/{player_id}` -> replace the
/// seat's name, its deck-or-commander link, and its rotation.
///
/// This is a **full replace**, not a patch: an absent or null `deck_id`/`commander_card_id`
/// unlinks what was there and an absent `rotation` seats the player upright, so a client changing one field sends the others
/// as they stand. The seat's life and starting life are deliberately not editable here — a
/// mis-set total is corrected through the life endpoint, which records it in the history
/// instead of silently moving the number.
#[utoipa::path(
    put,
    path = "/api/tools/{game}/life/sessions/{session_id}/players/{player_id}",
    tag = "Tools",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("session_id" = i32, Path, description = "Tracked-game id"),
        ("player_id" = i32, Path, description = "Seat id"),
    ),
    request_body = UpdateLifeSeatRequest,
    responses(
        (status = 200, description = "The updated seat.", body = LifeSeatResponse),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, the session/seat is not the caller's, `deck_id` is not one of their decks, or `commander_card_id` is not a card in the catalog."),
        (status = 409, description = "The game is finished."),
        (status = 422, description = "A blank/oversized name, both a deck and a commander, or a bad rotation."),
    ),
)]
pub async fn update_player(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, session_id, player_id)): Path<(String, i32, i32)>,
    JsonBody(payload): JsonBody<UpdateLifeSeatRequest>,
) -> Result<Json<LifeSeatResponse>, AppError> {
    require_game(&game)?;
    let session = load_session(&state, user.id, &game, session_id).await?;
    require_active(&session)?;
    let seat = load_seat(&state, session.id, player_id).await?;

    let name = validate_name(&payload.name, "player name", MAX_PLAYER_NAME)?;
    let rotation = validate_rotation(payload.rotation)?;
    let deck_id = resolve_deck_ref(&state, user.id, &game, payload.deck_id).await?;
    let commander_card_id = resolve_commander_ref(&state, &game, payload.commander_card_id).await?;
    require_single_link(deck_id, commander_card_id)?;

    let now = Utc::now();
    let mut active: life_session_player::ActiveModel = seat.into();
    active.name = Set(name);
    active.deck_id = Set(deck_id);
    active.commander_card_id = Set(commander_card_id);
    active.rotation = Set(rotation);
    active.updated_at = Set(now);
    let seat = active.update(&state.db).await?;
    touch_session(&state.db, session.id, now).await?;

    Ok(Json(seat_response(&state, user.id, &game, seat).await?))
}

/// Remove a player
///
/// `DELETE /api/tools/{game}/life/sessions/{session_id}/players/{player_id}` -> take a seat
/// off the table (they scooped, or were added by mistake). Their life history goes with them,
/// and the remaining seats are renumbered so positions stay 0-based and gap-free.
///
/// Removing the last seat is a `422` — a game with no players is not a game, and the session
/// should be deleted instead.
#[utoipa::path(
    delete,
    path = "/api/tools/{game}/life/sessions/{session_id}/players/{player_id}",
    tag = "Tools",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("session_id" = i32, Path, description = "Tracked-game id"),
        ("player_id" = i32, Path, description = "Seat id"),
    ),
    responses(
        (status = 200, description = "The game with the seat removed and the rest renumbered.", body = LifeSessionDetail),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or the session/seat is not the caller's."),
        (status = 409, description = "The game is finished."),
        (status = 422, description = "This is the last seat — delete the game instead."),
    ),
)]
pub async fn remove_player(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, session_id, player_id)): Path<(String, i32, i32)>,
) -> Result<Json<LifeSessionDetail>, AppError> {
    require_game(&game)?;
    let session = load_session(&state, user.id, &game, session_id).await?;
    require_active(&session)?;
    let seat = load_seat(&state, session.id, player_id).await?;

    let seats = seats_of(&state.db, session.id).await?;
    if seats.len() <= 1 {
        return Err(AppError::Validation(
            "a game needs at least one player — delete the game instead".to_string(),
        ));
    }

    let now = Utc::now();
    let txn = state.db.begin().await?;
    LifeSessionPlayer::delete_by_id(seat.id).exec(&txn).await?;
    // Close the gap the removal left, so `position` stays the dense seat order the layout
    // math indexes into.
    renumber(&txn, seats.iter().filter(|s| s.id != seat.id), now).await?;
    touch_session(&txn, session.id, now).await?;
    txn.commit().await?;

    Ok(Json(session_detail(&state, user.id, &session).await?))
}

/// Reorder the table
///
/// `PUT /api/tools/{game}/life/sessions/{session_id}/players/reorder` -> set the seat order,
/// which is the other half of "where does everyone sit": the layout decides the shape, the
/// order decides who's in which spot of it. `player_ids` must be exactly the session's seats,
/// each once.
#[utoipa::path(
    put,
    path = "/api/tools/{game}/life/sessions/{session_id}/players/reorder",
    tag = "Tools",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("session_id" = i32, Path, description = "Tracked-game id"),
    ),
    request_body = ReorderLifeSeatsRequest,
    responses(
        (status = 200, description = "The game with its seats reordered.", body = LifeSessionDetail),
        (status = 401, description = "Missing or invalid API key."),
        (status = 403, description = "API key is read-only."),
        (status = 404, description = "Unknown game, or the session is not the caller's."),
        (status = 409, description = "The game is finished."),
        (status = 422, description = "`player_ids` is not exactly the session's seats."),
    ),
)]
pub async fn reorder_players(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    Path((game, session_id)): Path<(String, i32)>,
    JsonBody(payload): JsonBody<ReorderLifeSeatsRequest>,
) -> Result<Json<LifeSessionDetail>, AppError> {
    require_game(&game)?;
    let session = load_session(&state, user.id, &game, session_id).await?;
    require_active(&session)?;

    let seats = seats_of(&state.db, session.id).await?;
    // Insist on a total permutation rather than tolerating a partial list: a client that lost
    // a seat mid-drag would otherwise silently collapse the order it didn't send.
    //
    // The length check is load-bearing and comes first: the sorted comparison below can't see a
    // duplicate on its own (`[a, a, b]` compares equal to `[a, b]` once deduped), and a repeated
    // id would leave two seats writing the same position and a hole where one of them should
    // have been — which the layout maths indexes into, so a seat would render in the wrong cell.
    let mut requested = payload.player_ids.clone();
    requested.sort_unstable();
    requested.dedup();
    let mut existing: Vec<i32> = seats.iter().map(|s| s.id).collect();
    existing.sort_unstable();
    if payload.player_ids.len() != existing.len() || requested != existing {
        return Err(AppError::Validation(
            "player_ids must list exactly the game's players, each once".to_string(),
        ));
    }

    let now = Utc::now();
    let txn = state.db.begin().await?;
    for (position, id) in payload.player_ids.iter().enumerate() {
        life_session_player::ActiveModel {
            id: Set(*id),
            position: Set(position as i32),
            updated_at: Set(now),
            ..Default::default()
        }
        .update(&txn)
        .await?;
    }
    touch_session(&txn, session.id, now).await?;
    txn.commit().await?;

    Ok(Json(session_detail(&state, user.id, &session).await?))
}

/// Rewrite `position` over an already-ordered run of seats so it is 0-based and gap-free.
/// Only the rows whose position actually moves are written.
async fn renumber<'a, C, I>(
    db: &C,
    seats: I,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), AppError>
where
    C: sea_orm::ConnectionTrait,
    I: Iterator<Item = &'a life_session_player::Model>,
{
    for (position, seat) in seats.enumerate() {
        let position = position as i32;
        if seat.position == position {
            continue;
        }
        life_session_player::ActiveModel {
            id: Set(seat.id),
            position: Set(position),
            updated_at: Set(now),
            ..Default::default()
        }
        .update(db)
        .await?;
    }
    Ok(())
}

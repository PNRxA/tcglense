//! The **play table** — an online, manual game of Magic played with friends.
//!
//! `/api/tools/{game}/play/...`. A *room* ([`play_room`]) is one table: the host opens it,
//! shares the six-character invite code, friends take *seats* ([`play_seat`]) and load a
//! deck, and the host starts the game. From that moment the table lives on **one WebSocket
//! per tab** ([`ws`]) driven by the pure engine in [`crate::play`]; the REST routes here only
//! get you from "nobody has a seat" to "everybody has a deck".
//!
//! It is the second tool beside [`life`](super::life), and it is the first surface in the app
//! that is neither public-catalog nor per-user. Three things fall out of that, and they are
//! what the module layout is organised around:
//!
//! - **A seat, not an account, is the unit of authorization.** A guest joins with nothing but
//!   a display name, so the credential for everything a seat does is the per-seat token
//!   ([`tokens`]) in `X-Play-Seat`, stored only as a digest. Only the *host* routes (create,
//!   list, delete) take a session, and they take [`SessionUser`] — like `/api/alerts`, because
//!   a live table is an interactive feature an API key has no business driving.
//!   A non-host's `DELETE` is a **404**, not a 403: the code is shareable, so the surface must
//!   not confirm which codes exist to someone who can't act on them.
//! - **The table is in memory, not in the database.** Every applied action would otherwise be
//!   a write on the socket's hot path. [`registry`] holds the live [`RoomState`] per room,
//!   hydrating it from `play_rooms.state` on first touch and letting a background sweeper
//!   write dirty rooms back every couple of seconds. A restart resumes from the last sweep.
//! - **The lobby and the table are the same shape.** A lobby has no `RoomState` at all — it
//!   *is* the seat rows — so the socket sends [`RoomSummary`] (`lobby` frames) until the game
//!   starts and [`Snapshot`](crate::play::types::Snapshot)s afterwards. Both the REST reads
//!   and the pushed lobby frames go through one [`summary_for`], so a page that polled and a
//!   page that was pushed can never disagree.
//!
//! Deck loading ([`decks`]) resolves one of the caller's decks, any precon, or a pasted list
//! into `Vec<CardDef>` **once**, at load time, and stores it on the seat — so starting the
//! game is a pure in-memory build with no catalog query on the path.
//!
//! Every route here answers `Cache-Control: no-store`; none are in OpenAPI (they're a
//! session/SPA feature, and the socket isn't JSON at all) — see `openapi.rs`'s
//! `INTENTIONALLY_UNDOCUMENTED`.
//!
//! [`SessionUser`]: crate::auth::extractor::SessionUser
//! [`play_room`]: crate::entities::play_room
//! [`play_seat`]: crate::entities::play_seat
//! [`RoomState`]: crate::play::types::RoomState

use rustrict::CensorStr;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::Deserialize;

use crate::entities::prelude::{PlayRoom, PlaySeat};
use crate::entities::{play_room, play_seat};
use crate::error::AppError;
use crate::play::types::{CardDef, RoomStatus, RoomSummary, SeatView};
use crate::state::AppState;

pub(crate) mod decks;
pub(crate) mod registry;
pub(crate) mod rooms;
pub(crate) mod seats;
pub(crate) mod tokens;
pub(crate) mod ws;

pub use rooms::{create_room, delete_room, get_room, list_rooms};
pub use seats::{join_room, leave_seat, load_seat_deck, set_seat_ready};
pub use ws::room_socket;

// ---------- Bounds ----------

/// A display name is what everyone else at the table sees, so it gets the same treatment a
/// username does: trimmed, bounded, and run through the profanity blocklist.
pub(crate) const MAX_DISPLAY_NAME: usize = 32;
/// Table label ("Friday pod"), optional.
pub(crate) const MAX_LABEL: usize = 60;
/// Starting life is overridable but bounded — a table, not a spreadsheet.
pub(crate) const MIN_STARTING_LIFE: i32 = 1;
pub(crate) const MAX_STARTING_LIFE: i32 = 999;
/// How many rooms one host may have open (`lobby` or `playing`) at once. Rooms are cheap
/// rows but each one is an in-memory table and a set of live sockets, so the cap is what
/// stops one account from parking hundreds of them.
pub(crate) const MAX_OPEN_ROOMS_PER_HOST: u64 = 20;
/// Default / maximum rooms the list returns.
pub(crate) const DEFAULT_ROOM_LIMIT: u64 = 50;
pub(crate) const MAX_ROOM_LIMIT: u64 = 50;

// ---------- Request shapes ----------

/// `POST /api/tools/{game}/play/rooms`.
#[derive(Debug, Deserialize)]
pub(crate) struct CreateRoomRequest {
    #[serde(default)]
    pub label: Option<String>,
    pub format: String,
    #[serde(default)]
    pub starting_life: Option<i32>,
    #[serde(default)]
    pub max_players: Option<i32>,
}

/// `GET /api/tools/{game}/play/rooms`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ListRoomsParams {
    /// `lobby` / `playing` / `finished`; absent returns every status.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub limit: Option<u64>,
}

/// `POST /api/tools/{game}/play/rooms/{code}/join`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct JoinRequest {
    /// Required for a guest; a signed-in caller defaults to their username.
    #[serde(default)]
    pub name: Option<String>,
    /// A previously issued seat token — re-takes that seat instead of minting a new one.
    #[serde(default)]
    pub seat_token: Option<String>,
}

/// `POST .../seats/{seat_id}/deck` — an externally tagged union on `source`, matching
/// `LoadPlayDeckBody` in `web/src/lib/api/play.ts`.
#[derive(Debug, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub(crate) enum LoadDeckRequest {
    /// One of the caller's own decks (the seat must be a signed-in one).
    Deck { deck_id: i32 },
    /// Any published preconstructed deck, by slug.
    Precon { slug: String },
    /// A pasted decklist in the shared text grammar.
    Text { text: String },
}

/// `POST .../seats/{seat_id}/ready`.
#[derive(Debug, Deserialize)]
pub(crate) struct SetReadyRequest {
    pub ready: bool,
}

// ---------- Validation ----------

/// Trim + validate a seat's display name. Same rules as a username's final two checks
/// (length, then the `rustrict` blocklist) — reused rather than re-derived because "a name
/// other people are shown" is the same problem in both places.
pub(crate) fn validate_display_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    let len = name.chars().count();
    if len == 0 {
        return Err(AppError::Validation("name must not be empty".to_string()));
    }
    if len > MAX_DISPLAY_NAME {
        return Err(AppError::Validation(format!(
            "name must be at most {MAX_DISPLAY_NAME} characters"
        )));
    }
    if name.is_inappropriate() {
        return Err(AppError::Validation(
            "that name isn't allowed; please choose another".to_string(),
        ));
    }
    Ok(name.to_string())
}

/// The room's status as the engine's vocabulary, or a masked 500 for a column that somehow
/// holds something else (only reachable by writing the row by hand).
pub(crate) fn room_status(room: &play_room::Model) -> Result<RoomStatus, AppError> {
    RoomStatus::parse(&room.status)
        .ok_or_else(|| AppError::Internal(format!("unknown play room status '{}'", room.status)))
}

/// Refuse a lobby-only action once the game has started. A `409` (the state is wrong, not the
/// request) — the same call the life counter's finished-session gate makes.
pub(crate) fn require_lobby(room: &play_room::Model) -> Result<(), AppError> {
    if room_status(room)? == RoomStatus::Lobby {
        return Ok(());
    }
    Err(AppError::Conflict(
        "this game has already started".to_string(),
    ))
}

// ---------- Shared loads ----------

/// Load a room by its invite code within a game, or **404**. The code is normalised first, so
/// a link pasted in lower case resolves.
pub(crate) async fn load_room(
    state: &AppState,
    game: &str,
    code: &str,
) -> Result<play_room::Model, AppError> {
    load_room_on(&state.db, game, code).await
}

/// [`load_room`] against an arbitrary connection — a write re-reads the room *inside* its
/// transaction so the lobby gate is evaluated against the state it is about to write to.
pub(crate) async fn load_room_on<C: sea_orm::ConnectionTrait>(
    db: &C,
    game: &str,
    code: &str,
) -> Result<play_room::Model, AppError> {
    let code = tokens::normalize_code(code);
    PlayRoom::find()
        .filter(play_room::Column::Game.eq(game))
        .filter(play_room::Column::Code.eq(code))
        .one(db)
        .await?
        .ok_or_else(|| AppError::NotFound("room not found".to_string()))
}

/// A room's seats in seat order.
pub(crate) async fn seats_of<C: sea_orm::ConnectionTrait>(
    db: &C,
    room_id: i32,
) -> Result<Vec<play_seat::Model>, AppError> {
    Ok(PlaySeat::find()
        .filter(play_seat::Column::RoomId.eq(room_id))
        .order_by_asc(play_seat::Column::SeatIndex)
        .order_by_asc(play_seat::Column::Id)
        .all(db)
        .await?)
}

/// The decklist stored on a seat. A column that fails to parse reads as "no deck" rather than
/// failing the request — it is this server's own JSON, so a bad value means a schema change,
/// and the honest answer is that the seat needs to load its deck again.
pub(crate) fn seat_deck(seat: &play_seat::Model) -> Vec<CardDef> {
    seat.deck_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Vec<CardDef>>(json).ok())
        .unwrap_or_default()
}

// ---------- Views ----------

/// One seat as the lobby shows it. `connected` is the registry's live socket count for the
/// seat, which is why this takes it rather than reading it off the row — presence is in
/// memory, not in the database.
pub(crate) fn seat_view(
    room: &play_room::Model,
    seat: &play_seat::Model,
    connected: bool,
) -> SeatView {
    let deck = seat_deck(seat);
    SeatView {
        id: seat.id,
        seat_index: seat.seat_index,
        display_name: seat.display_name.clone(),
        is_host: seat.user_id == Some(room.host_user_id),
        is_user: seat.user_id.is_some(),
        ready: seat.ready,
        connected,
        deck_source: seat.deck_source.clone(),
        deck_name: seat.deck_name.clone(),
        deck_card_count: seat
            .deck_json
            .as_ref()
            .map(|_| u32::try_from(deck.len()).unwrap_or(u32::MAX)),
        commanders: deck
            .iter()
            .filter(|card| card.is_commander)
            .map(|card| card.name.clone())
            .collect(),
    }
}

/// The one place a [`RoomSummary`] is read for a REST answer — with `viewer_seat` filled in
/// for the account reading it (`None` for a stranger), which is what lets the hub's room list
/// tell the caller's own seat apart. The lobby frames the registry pushes go through
/// [`summary_from`] so a client that polled and one that was pushed see the same seats.
pub(crate) async fn summary_for_viewer(
    state: &AppState,
    room: &play_room::Model,
    user_id: Option<i32>,
) -> Result<RoomSummary, AppError> {
    let seats = seats_of(&state.db, room.id).await?;
    let mut summary = summary_from(state, room, &seats);
    summary.viewer_seat = viewer_seat_for(&seats, user_id);
    Ok(summary)
}

/// The seat an account holds among `seats`, if any.
pub(crate) fn viewer_seat_for(seats: &[play_seat::Model], user_id: Option<i32>) -> Option<i32> {
    let user_id = user_id?;
    seats
        .iter()
        .find(|seat| seat.user_id == Some(user_id))
        .map(|seat| seat.id)
}

/// Re-read a room's seats and push the fresh lobby state at everyone watching its socket.
///
/// The one builder of a pushed `lobby` frame: every REST write that changes who is at the
/// table ends here, and so does a socket connecting or disconnecting while the room is still a
/// lobby (presence there *is* the connection set, so the dots only move when this is sent).
/// It goes through [`summary_from`] like the REST reads, so a page that polled and a page that
/// was pushed can never disagree.
pub(crate) async fn push_lobby(state: &AppState, room: &play_room::Model) -> Result<(), AppError> {
    let seats = seats_of(&state.db, room.id).await?;
    state
        .play
        .push_lobby(room.id, summary_from(state, room, &seats));
    Ok(())
}

/// [`summary_for`] over seats the caller has already read (a write path that just wrote them
/// shouldn't pay for a second query).
pub(crate) fn summary_from(
    state: &AppState,
    room: &play_room::Model,
    seats: &[play_seat::Model],
) -> RoomSummary {
    let connected = state.play.connected_seats(room.id);
    RoomSummary {
        id: room.id,
        code: room.code.clone(),
        game: room.game.clone(),
        label: room.label.clone(),
        format: room.format.clone(),
        starting_life: room.starting_life,
        max_players: room.max_players,
        status: RoomStatus::parse(&room.status).unwrap_or(RoomStatus::Lobby),
        seats: seats
            .iter()
            .map(|seat| seat_view(room, seat, connected.contains(&seat.id)))
            .collect(),
        viewer_seat: None,
        created_at: room.created_at,
        updated_at: room.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_are_trimmed_bounded_and_filtered() {
        assert_eq!(validate_display_name("  Alice  ").unwrap(), "Alice");
        assert!(validate_display_name("   ").is_err());
        // The bound counts characters, not bytes.
        assert!(validate_display_name(&"a".repeat(MAX_DISPLAY_NAME)).is_ok());
        assert!(validate_display_name(&"a".repeat(MAX_DISPLAY_NAME + 1)).is_err());
    }
}

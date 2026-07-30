//! Authenticated, per-user **life counter** — a tracked game of MTG.
//!
//! `/api/tools/{game}/life/sessions/...`. A *session* is one game being played: it holds a
//! seat per player ([`life_session_player`]), each seat's current life total, and every life
//! change that got it there ([`life_event`]) — the gain/loss history the SPA charts and can
//! undo. Seats carry their own `rotation` and the session a `layout` slug, which together are
//! the "how many players and where do they sit" configuration.
//!
//! It is a **container** surface like [`decks`](crate::handlers::decks), not a
//! collection/wish-list twin: a user has many sessions per game, so it doesn't ride
//! `makeHoldingApi`'s shared holdings engine — there are no card holdings here at all. What
//! it *does* reuse is the container idiom itself: a seat has **no `user_id`** (it hangs off
//! `session_id`), so every seat- and event-scoped route calls [`load_session`] to prove
//! ownership first, and a session that isn't the caller's is a **404, not a 403** — no
//! existence oracle over session ids, exactly as `load_deck` does.
//!
//! The optional per-seat `deck_id` is what makes this more than a life counter: link a seat to
//! one of your decks, record who won, and [`deck_records`](stats::deck_records) turns finished
//! sessions into a win/loss record per deck. That link is deliberately **orphan-tolerant** —
//! deleting a deck you've played must not fail or delete history, so it carries no foreign
//! key and the stats read inner-joins `decks`, which simply stops counting a deck that's gone.
//!
//! Two invariants worth keeping:
//!
//! - **A finished session is immutable.** Life edits, seat edits, and undo all require
//!   `status == "active"` ([`require_active`]) and answer `409` otherwise, so a recorded
//!   result can't drift out from under the deck record it already contributed to.
//! - **Life totals are never derived twice.** A tap appends one event and moves the seat by
//!   its delta; an undo re-folds the seat's whole chain through the pure
//!   [`replay`](replay::replay) fold. Nothing else writes `life_session_players.life`.
//!
//! Every route is in the router's `private` group ([`AuthUser`] reads / [`WritableUser`]
//! writes, `Cache-Control: no-store`, per-user rate limited).
//!
//! [`AuthUser`]: crate::auth::extractor::AuthUser
//! [`WritableUser`]: crate::auth::extractor::WritableUser
//! [`life_session_player`]: crate::entities::life_session_player
//! [`life_event`]: crate::entities::life_event

use std::collections::HashMap;

use sea_orm::prelude::DateTimeUtc;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};

use crate::entities::prelude::{Deck, LifeEvent, LifeSession, LifeSessionPlayer};
use crate::entities::{deck, life_event, life_session, life_session_player};
use crate::error::AppError;
use crate::state::AppState;

mod events;
mod players;
mod read;
mod replay;
mod stats;
mod write;

pub use events::{adjust_life, undo_life_event};
pub use players::{add_player, remove_player, reorder_players, update_player};
pub use read::{get_session, list_sessions};
pub use stats::deck_records;
pub use write::{create_session, delete_session, finish_session, update_session};

// The `#[utoipa::path]`-generated route metadata structs, re-exported so
// `crate::openapi::ApiDoc` can name them at `crate::handlers::tools::life::__path_<fn>`.
pub use events::{__path_adjust_life, __path_undo_life_event};
pub use players::{
    __path_add_player, __path_remove_player, __path_reorder_players, __path_update_player,
};
pub use read::{__path_get_session, __path_list_sessions};
pub use stats::__path_deck_records;
pub use write::{
    __path_create_session, __path_delete_session, __path_finish_session, __path_update_session,
};

// ---------- Vocabularies ----------

/// A session being played. Only an active session accepts edits.
pub(crate) const STATUS_ACTIVE: &str = "active";
/// A session whose result has been recorded — immutable, and the only kind the per-deck
/// record counts.
pub(crate) const STATUS_FINISHED: &str = "finished";

/// A seat's outcome while the game is still being played.
pub(crate) const RESULT_NONE: &str = "none";
pub(crate) const RESULT_WIN: &str = "win";
pub(crate) const RESULT_LOSS: &str = "loss";
pub(crate) const RESULT_DRAW: &str = "draw";

/// A relative life change — the common case, one tap or a run of taps committed together.
pub(crate) const KIND_ADJUST: &str = "adjust";
/// An absolute correction: the total was typed in, not moved by.
pub(crate) const KIND_SET: &str = "set";

/// The seat-placement vocabulary. The SPA owns the pixels; the server owns the vocabulary so
/// a stored layout is always one the client can render (`web/src/lib/lifeLayout.ts` mirrors
/// this list, pinned by a unit test on each side).
///
/// Each slug is a physical arrangement, not a cosmetic one — which is why there are four and
/// not one with options:
///
/// - `rows` — one seat per full-width row, all upright. One person holding the device.
/// - `grid` — two columns, all upright. One person holding a *tablet* for a big pod.
/// - `facing` — two banks on opposite edges of the device, the far bank rotated 180°. The
///   device flat between two sides of a table; the common tabletop case at any count.
/// - `pinwheel` — one seat per edge, each a quarter turn from the last, for a 3- or
///   4-player pod sitting around a device in the middle.
pub(crate) const LAYOUTS: &[&str] = &["rows", "facing", "grid", "pinwheel"];

/// The rotations a seat may be stored at, in degrees clockwise, applied to the seat's tile
/// content. The convention (mirrored in `web/src/lib/lifeLayout.ts`): `0` reads upright to a
/// player at the near edge, `90` to one at the **left** edge, `180` at the far edge, `270` at
/// the right — because a clockwise quarter turn maps the text's "up" from the near edge to the
/// left one.
pub(crate) const ROTATIONS: &[i32] = &[0, 90, 180, 270];

/// The layout a new session gets when the client doesn't ask for one — the arrangement each
/// player count is normally played in: one player holds the device, two or three sit across a
/// table from each other, a pod of four sits around one, and a bigger pod gets the grid that
/// still fits on a held screen.
pub(crate) fn default_layout_for(player_count: usize) -> &'static str {
    match player_count {
        0..=1 => "rows",
        2..=3 => "facing",
        4 => "pinwheel",
        _ => "grid",
    }
}

/// The rotation `layout` seats `position` at, by seat index — so a server-created rematch
/// reproduces the arrangement the client would have drawn rather than flattening every seat
/// upright. Mirrored by `defaultRotationFor` in `web/src/lib/lifeLayout.ts`.
pub(crate) fn default_rotation_for(layout: &str, position: usize, player_count: usize) -> i32 {
    // A single seat is the whole screen and always reads upright, whatever the layout.
    if player_count <= 1 {
        return 0;
    }
    match layout {
        // The near bank fills up from seat 0; everything after it is the far side of the
        // table, which reads upside-down from where you're sitting.
        "facing" => {
            let near = player_count.div_ceil(2);
            if position >= near { 180 } else { 0 }
        }
        // One seat per edge, advancing a quarter turn: near, left, far, right. With three
        // seats the near player takes the whole bottom and the other two take the sides.
        "pinwheel" if player_count == 3 => match position {
            1 => 90,
            2 => 270,
            _ => 0,
        },
        "pinwheel" => match position {
            1 => 90,
            2 => 180,
            3 => 270,
            _ => 0,
        },
        // `rows` and `grid` are held by one person, so every seat reads upright.
        _ => 0,
    }
}

// ---------- Limits ----------

/// Generous per-`(user, game)` session cap: far above any real play history, but bounded so
/// the list stays cheap and one account can't create unbounded rows.
const MAX_SESSIONS_PER_GAME: u64 = 2_000;
/// Seats in one session. Six is a big pod; twelve leaves room for a Two-Headed Giant table or
/// a shared-life variant without becoming a spreadsheet.
const MAX_PLAYERS: usize = 12;
/// Life changes recorded in one session. A long game is a few hundred; the cap only exists so
/// a stuck client can't grow one session without bound (and so the replay fold stays cheap).
const MAX_EVENTS_PER_SESSION: u64 = 5_000;

/// The storable life range. Wide enough for the silliest gain loop, narrow enough that the
/// number still renders — and the fold clamps to it rather than overflowing.
pub(crate) const LIFE_MIN: i32 = -9_999;
pub(crate) const LIFE_MAX: i32 = 9_999;
/// The largest single change one commit may carry. A run of taps is committed as one delta, so
/// this bounds a stuck key, not a legitimate edit (an absolute `life` has no such cap — it's
/// bounded by [`LIFE_MIN`]/[`LIFE_MAX`] directly).
const MAX_DELTA: i32 = 1_000;
/// Starting-life bounds: 20 / 30 / 40 are the presets, and a custom value stays positive.
const MIN_STARTING_LIFE: i32 = 1;
const MAX_STARTING_LIFE: i32 = 9_999;

const MAX_SESSION_NAME: usize = 200;
const MAX_FORMAT: usize = 50;
const MAX_PLAYER_NAME: usize = 60;

// ---------- Response DTOs ----------

/// One seat in a tracked game: who's sitting there, what deck they brought, where they are on
/// screen, what they're on, and how the game ended for them.
#[derive(Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "LifeSeat"))]
pub struct LifeSeatResponse {
    pub id: i32,
    /// Seat order within the session, 0-based and gap-free.
    pub position: i32,
    pub name: String,
    /// The linked deck, or null. A deck the owner has since deleted reads as null.
    pub deck_id: Option<i32>,
    /// The linked deck's name, resolved for display so the client needs no second fetch.
    pub deck_name: Option<String>,
    pub starting_life: i32,
    pub life: i32,
    /// Screen rotation in degrees (`0`, `90`, `180`, `270`).
    pub rotation: i32,
    /// `none` while the game is active, then `win` / `loss` / `draw`.
    pub result: String,
}

impl LifeSeatResponse {
    fn from_model(seat: life_session_player::Model, deck_name: Option<String>) -> Self {
        Self {
            id: seat.id,
            position: seat.position,
            name: seat.name,
            // A seat pointing at a deck that no longer exists (or was never the caller's)
            // resolves to no name — so report the link as absent rather than dangling.
            deck_id: deck_name.is_some().then_some(seat.deck_id).flatten(),
            deck_name,
            starting_life: seat.starting_life,
            life: seat.life,
            rotation: seat.rotation,
            result: seat.result,
        }
    }
}

/// One recorded life change. `delta` is what the change was; `life_after` is what it left the
/// seat on, so a history row and a chart point need no client-side fold.
#[derive(Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "LifeEvent"))]
pub struct LifeEventResponse {
    pub id: i32,
    pub player_id: i32,
    pub delta: i32,
    pub life_after: i32,
    /// `adjust` (relative) or `set` (absolute correction).
    pub kind: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTimeUtc,
}

impl From<life_event::Model> for LifeEventResponse {
    fn from(e: life_event::Model) -> Self {
        Self {
            id: e.id,
            player_id: e.player_id,
            delta: e.delta,
            life_after: e.life_after,
            kind: e.kind,
            created_at: e.created_at,
        }
    }
}

/// A tracked game's header plus its seats — what the session list returns, and what every
/// write echoes back. The history is only on the detail read (it's the unbounded part).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "LifeSession"))]
pub struct LifeSessionResponse {
    pub id: i32,
    /// Game slug — carried so a client holding a bare session can build its links.
    pub game: String,
    pub name: Option<String>,
    pub format: Option<String>,
    /// The total a new seat in this session starts on.
    pub starting_life: i32,
    /// Seat-placement layout slug — one of `rows` / `facing` / `grid` / `pinwheel`.
    pub layout: String,
    /// `active` or `finished`. Only an active session accepts edits.
    pub status: String,
    /// Seats in `position` order.
    pub players: Vec<LifeSeatResponse>,
    #[schema(value_type = String, format = DateTime)]
    pub started_at: DateTimeUtc,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub finished_at: Option<DateTimeUtc>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTimeUtc,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: DateTimeUtc,
}

/// One tracked game in full: its header + seats, plus every recorded life change in the order
/// they happened. A session is bounded (at most [`MAX_EVENTS_PER_SESSION`] events), so this is
/// returned whole — the SPA groups `events` by `player_id` for the per-seat sparklines and
/// reads them in order for the shared timeline.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct LifeSessionDetail {
    pub session: LifeSessionResponse,
    pub events: Vec<LifeEventResponse>,
}

/// The result of one life change: the seat as it now stands and the event that moved it. Kept
/// small deliberately — this is the hot path (a tap commit), so it doesn't re-send the game.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct LifeChange {
    pub player: LifeSeatResponse,
    pub event: LifeEventResponse,
}

/// A deck's record across finished sessions: how it's actually performed. `games` counts only
/// finished sessions where the seat playing this deck carries a result, so an abandoned game
/// never dilutes a win rate.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct LifeDeckRecord {
    pub deck_id: i32,
    pub deck_name: String,
    pub games: i64,
    pub wins: i64,
    pub losses: i64,
    pub draws: i64,
    /// `wins / games` in `0.0..=1.0`, or null with no games — never a division by zero, and
    /// never a misleading `0%` for a deck that hasn't been played.
    pub win_rate: Option<f64>,
    /// When this deck was last played (the newest finished session's start), or null.
    #[schema(value_type = Option<String>, format = DateTime)]
    pub last_played_at: Option<DateTimeUtc>,
}

// ---------- Request DTOs ----------

/// One seat in a create/add request. Everything is optional: an unnamed seat is filled in as
/// `Player {n}`, and a seat with no `starting_life` inherits the session's.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct LifeSeatInput {
    #[serde(default)]
    pub name: Option<String>,
    /// One of the caller's decks for the game, or null. A deck that isn't theirs is a `404`.
    #[serde(default)]
    pub deck_id: Option<i32>,
    #[serde(default)]
    pub starting_life: Option<i32>,
    /// `0` / `90` / `180` / `270`. Absent takes the layout's default for the seat.
    #[serde(default)]
    pub rotation: Option<i32>,
}

/// Body of `POST /api/tools/{game}/life/sessions`.
///
/// Either describe the game (`players`, optionally with `starting_life` / `layout` / `format`)
/// or pass `from_session_id` to rematch: the seats, decks, rotations, starting life, layout and
/// format of an earlier session are copied and a fresh game begins on full life. An explicit
/// field always wins over the copied one, and a non-empty `players` replaces the copied seats
/// outright.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CreateLifeSessionRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub starting_life: Option<i32>,
    #[serde(default)]
    pub layout: Option<String>,
    /// The seats to open the game with. May be empty only alongside `from_session_id`.
    #[serde(default)]
    pub players: Vec<LifeSeatInput>,
    /// An earlier session of the caller's to copy the table from (a rematch).
    #[serde(default)]
    pub from_session_id: Option<i32>,
}

/// Body of `PUT /api/tools/{game}/life/sessions/{session_id}`: edit the game's label, format
/// or layout. Each field is optional — absent leaves it unchanged; an explicit blank string
/// clears `name`/`format`.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct UpdateLifeSessionRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub layout: Option<String>,
}

/// Body of `POST /api/tools/{game}/life/sessions/{session_id}/finish`: record the result.
/// `winner_player_id` names the seat that won; `null` records a draw for the whole table.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct FinishLifeSessionRequest {
    #[serde(default)]
    pub winner_player_id: Option<i32>,
}

/// Body of `PUT .../players/{player_id}`: replace the seat's editable state. This is a full
/// replace, not a patch — `deck_id` absent or null unlinks the deck, and `rotation` absent
/// resets the seat upright, so a client editing one field must send the others as they are.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct UpdateLifeSeatRequest {
    pub name: String,
    #[serde(default)]
    pub deck_id: Option<i32>,
    #[serde(default)]
    pub rotation: i32,
}

/// Body of `PUT .../players/reorder`: the seat ids in the desired order (must be exactly the
/// session's seats). Positions are rewritten 0-based and gap-free.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ReorderLifeSeatsRequest {
    pub player_ids: Vec<i32>,
}

/// Body of `POST .../players/{player_id}/life`: change a seat's total. Send exactly one of
/// `delta` (a relative change — what a run of taps commits) or `life` (an absolute
/// correction). Both, or neither, is a `422`.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct AdjustLifeRequest {
    #[serde(default)]
    pub delta: Option<i32>,
    #[serde(default)]
    pub life: Option<i32>,
}

/// Query for `GET /api/tools/{game}/life/sessions`: narrow to games still in progress or
/// already finished, and cap how many come back.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ListSessionsParams {
    /// `active` or `finished`; absent returns both.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub limit: Option<u64>,
}

/// Query for `GET /api/tools/{game}/life/decks`: narrow the record list to one deck (what the
/// deck page asks for, so it doesn't pull every deck's record to show one line).
#[derive(Debug, Default, Deserialize)]
pub(crate) struct DeckRecordParams {
    #[serde(default)]
    pub deck_id: Option<i32>,
}

/// Default / maximum number of sessions the list returns.
pub(crate) const DEFAULT_SESSION_LIMIT: u64 = 50;
pub(crate) const MAX_SESSION_LIMIT: u64 = 200;

// ---------- Shared helpers ----------

/// Load a session by id, proving it belongs to `user_id` for `game`. A session that doesn't
/// exist, belongs to another user, or is for another game is a **404** (never 403), so the
/// surface is not an existence oracle over session ids.
pub(crate) async fn load_session(
    state: &AppState,
    user_id: i32,
    game: &str,
    session_id: i32,
) -> Result<life_session::Model, AppError> {
    LifeSession::find_by_id(session_id)
        .filter(life_session::Column::UserId.eq(user_id))
        .filter(life_session::Column::Game.eq(game))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("session not found".to_string()))
}

/// Load a seat by id, proving it belongs to `session_id` (whose ownership the caller has
/// already proved). A seat in another session is a **404**.
pub(crate) async fn load_seat(
    state: &AppState,
    session_id: i32,
    player_id: i32,
) -> Result<life_session_player::Model, AppError> {
    LifeSessionPlayer::find_by_id(player_id)
        .filter(life_session_player::Column::SessionId.eq(session_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("player not found".to_string()))
}

/// Refuse an edit to a finished game. A recorded result already counts towards the per-deck
/// record, so letting life or seats move afterwards would silently rewrite history — hence a
/// `409`, which tells the client the state is wrong rather than the request.
pub(crate) fn require_active(session: &life_session::Model) -> Result<(), AppError> {
    if session.status == STATUS_ACTIVE {
        return Ok(());
    }
    Err(AppError::Conflict(
        "this game is finished; start a rematch to keep playing".to_string(),
    ))
}

/// A session's seats in display order.
pub(crate) async fn seats_of(
    db: &sea_orm::DatabaseConnection,
    session_id: i32,
) -> Result<Vec<life_session_player::Model>, AppError> {
    Ok(LifeSessionPlayer::find()
        .filter(life_session_player::Column::SessionId.eq(session_id))
        .order_by_asc(life_session_player::Column::Position)
        .order_by_asc(life_session_player::Column::Id)
        .all(db)
        .await?)
}

/// Resolve the names of the decks a set of seats link to, keyed by deck id.
///
/// Scoped to the caller's decks for the game, so a link to a deck that has since been deleted
/// (the column is FK-less and orphan-tolerant by design) simply resolves to nothing and the
/// seat reports no deck — never another user's deck name.
pub(crate) async fn deck_names_for(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    game: &str,
    seats: &[life_session_player::Model],
) -> Result<HashMap<i32, String>, AppError> {
    let ids: Vec<i32> = seats.iter().filter_map(|s| s.deck_id).collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(i32, String)> = Deck::find()
        .select_only()
        .column(deck::Column::Id)
        .column(deck::Column::Name)
        .filter(deck::Column::Id.is_in(ids))
        .filter(deck::Column::UserId.eq(user_id))
        .filter(deck::Column::Game.eq(game))
        .into_tuple()
        .all(db)
        .await?;
    Ok(rows.into_iter().collect())
}

/// Shape a session + its seats into the wire header, resolving each seat's deck name.
pub(crate) fn session_response(
    session: &life_session::Model,
    seats: Vec<life_session_player::Model>,
    deck_names: &HashMap<i32, String>,
) -> LifeSessionResponse {
    LifeSessionResponse {
        id: session.id,
        game: session.game.clone(),
        name: session.name.clone(),
        format: session.format.clone(),
        starting_life: session.starting_life,
        layout: session.layout.clone(),
        status: session.status.clone(),
        players: seats
            .into_iter()
            .map(|seat| {
                let deck_name = seat.deck_id.and_then(|id| deck_names.get(&id).cloned());
                LifeSeatResponse::from_model(seat, deck_name)
            })
            .collect(),
        started_at: session.started_at,
        finished_at: session.finished_at,
        created_at: session.created_at,
        updated_at: session.updated_at,
    }
}

/// The full detail read: header + seats + the whole history, in the order it happened.
/// Shared by the detail GET and by every write that changes the table's shape.
pub(crate) async fn session_detail(
    state: &AppState,
    user_id: i32,
    session: &life_session::Model,
) -> Result<LifeSessionDetail, AppError> {
    let seats = seats_of(&state.db, session.id).await?;
    let deck_names = deck_names_for(&state.db, user_id, &session.game, &seats).await?;
    let events = LifeEvent::find()
        .filter(life_event::Column::SessionId.eq(session.id))
        .order_by_asc(life_event::Column::Id)
        .all(&state.db)
        .await?;
    Ok(LifeSessionDetail {
        session: session_response(session, seats, &deck_names),
        events: events.into_iter().map(LifeEventResponse::from).collect(),
    })
}

/// Shape one seat for the wire, resolving its deck name.
pub(crate) async fn seat_response(
    state: &AppState,
    user_id: i32,
    game: &str,
    seat: life_session_player::Model,
) -> Result<LifeSeatResponse, AppError> {
    let names = deck_names_for(&state.db, user_id, game, std::slice::from_ref(&seat)).await?;
    let deck_name = seat.deck_id.and_then(|id| names.get(&id).cloned());
    Ok(LifeSeatResponse::from_model(seat, deck_name))
}

/// Resolve a seat's deck reference: `None` stays `None`; a `Some(id)` must be one of the
/// caller's decks for the game (else 404, matching `decks`' folder reference).
pub(crate) async fn resolve_deck_ref(
    state: &AppState,
    user_id: i32,
    game: &str,
    deck_id: Option<i32>,
) -> Result<Option<i32>, AppError> {
    let Some(id) = deck_id else {
        return Ok(None);
    };
    let exists = Deck::find_by_id(id)
        .filter(deck::Column::UserId.eq(user_id))
        .filter(deck::Column::Game.eq(game))
        .one(&state.db)
        .await?
        .is_some();
    if !exists {
        return Err(AppError::NotFound("deck not found".to_string()));
    }
    Ok(Some(id))
}

/// Validate a layout slug against [`LAYOUTS`].
pub(crate) fn validate_layout(layout: &str) -> Result<String, AppError> {
    let trimmed = layout.trim();
    if LAYOUTS.contains(&trimmed) {
        return Ok(trimmed.to_string());
    }
    Err(AppError::Validation(format!(
        "layout must be one of {}",
        LAYOUTS.join(", ")
    )))
}

/// Validate a seat rotation against [`ROTATIONS`].
pub(crate) fn validate_rotation(rotation: i32) -> Result<i32, AppError> {
    if ROTATIONS.contains(&rotation) {
        return Ok(rotation);
    }
    Err(AppError::Validation(
        "rotation must be 0, 90, 180 or 270".to_string(),
    ))
}

/// Validate a starting life total.
pub(crate) fn validate_starting_life(life: i32) -> Result<i32, AppError> {
    if (MIN_STARTING_LIFE..=MAX_STARTING_LIFE).contains(&life) {
        return Ok(life);
    }
    Err(AppError::Validation(format!(
        "starting_life must be between {MIN_STARTING_LIFE} and {MAX_STARTING_LIFE}"
    )))
}

/// Validate a relative life change.
pub(crate) fn validate_delta(delta: i32) -> Result<i32, AppError> {
    // A range check, not `delta.abs() <= MAX_DELTA`: `i32::MIN.abs()` overflows, which panics on
    // a request path in a debug build and wraps back to `i32::MIN` in a release one — where it
    // then compares as *within* the bound and slips past the very check it was meant to fail.
    if (-MAX_DELTA..=MAX_DELTA).contains(&delta) {
        return Ok(delta);
    }
    Err(AppError::Validation(format!(
        "delta must be between -{MAX_DELTA} and {MAX_DELTA}"
    )))
}

/// Validate an absolute life total.
pub(crate) fn validate_life(life: i32) -> Result<i32, AppError> {
    if (LIFE_MIN..=LIFE_MAX).contains(&life) {
        return Ok(life);
    }
    Err(AppError::Validation(format!(
        "life must be between {LIFE_MIN} and {LIFE_MAX}"
    )))
}

/// Bump a session's `updated_at` so an edit bubbles it up the recency-sorted list. One cheap
/// indexed UPDATE — the caller has already proved ownership via [`load_session`].
pub(crate) async fn touch_session<C: sea_orm::ConnectionTrait>(
    db: &C,
    session_id: i32,
    now: DateTimeUtc,
) -> Result<(), AppError> {
    use sea_orm::sea_query::Expr;
    LifeSession::update_many()
        .col_expr(life_session::Column::UpdatedAt, Expr::value(now))
        .filter(life_session::Column::Id.eq(session_id))
        .exec(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_matches_how_each_player_count_is_actually_played() {
        assert_eq!(default_layout_for(1), "rows");
        assert_eq!(default_layout_for(2), "facing");
        assert_eq!(default_layout_for(3), "facing");
        assert_eq!(default_layout_for(4), "pinwheel");
        assert_eq!(default_layout_for(6), "grid");
        // Every default must be a slug the client knows how to render.
        for count in 0..=MAX_PLAYERS {
            assert!(
                LAYOUTS.contains(&default_layout_for(count)),
                "count {count}"
            );
        }
    }

    #[test]
    fn facing_splits_the_table_into_a_near_and_a_far_bank() {
        let banks = |count: usize| -> Vec<i32> {
            (0..count)
                .map(|p| default_rotation_for("facing", p, count))
                .collect()
        };
        // Two across a table: the far seat is upside-down from the near one.
        assert_eq!(banks(2), vec![0, 180]);
        // An odd count puts the extra seat on the near side, since the near bank is the one
        // whose player is holding (or sitting behind) the device.
        assert_eq!(banks(3), vec![0, 0, 180]);
        assert_eq!(banks(4), vec![0, 0, 180, 180]);
        assert_eq!(banks(5), vec![0, 0, 0, 180, 180]);
    }

    #[test]
    fn pinwheel_advances_a_quarter_turn_per_edge() {
        let pinwheel = |count: usize| -> Vec<i32> {
            (0..count)
                .map(|p| default_rotation_for("pinwheel", p, count))
                .collect()
        };
        // Four seats, one per edge: near, left, far, right.
        assert_eq!(pinwheel(4), vec![0, 90, 180, 270]);
        // Three seats: the near player plus the two sides — nobody sits opposite.
        assert_eq!(pinwheel(3), vec![0, 90, 270]);
    }

    #[test]
    fn held_layouts_keep_every_seat_upright_and_rotations_stay_in_vocabulary() {
        assert_eq!(default_rotation_for("rows", 3, 5), 0);
        assert_eq!(default_rotation_for("grid", 2, 3), 0);
        // A lone seat is the whole screen, so no layout turns it away from its reader.
        for layout in LAYOUTS {
            assert_eq!(default_rotation_for(layout, 0, 1), 0, "{layout}");
        }
        for layout in LAYOUTS {
            for count in 1..=MAX_PLAYERS {
                for position in 0..count {
                    let r = default_rotation_for(layout, position, count);
                    assert!(ROTATIONS.contains(&r), "{layout}/{position}/{count} -> {r}");
                }
            }
        }
    }

    #[test]
    fn validators_reject_out_of_vocabulary_input() {
        assert!(validate_layout("pinwheel").is_ok());
        assert!(validate_layout(" rows ").is_ok(), "slugs are trimmed");
        assert!(validate_layout("spiral").is_err());
        assert!(validate_rotation(270).is_ok());
        assert!(validate_rotation(45).is_err());
        assert!(validate_starting_life(40).is_ok());
        assert!(validate_starting_life(0).is_err());
        assert!(validate_delta(-1_000).is_ok());
        assert!(validate_delta(1_001).is_err());
        // The extremes of the type, not just of the bound: a naive `abs()` check overflows on
        // `i32::MIN` — panicking in debug, and wrapping back inside the bound in release.
        assert!(validate_delta(i32::MIN).is_err());
        assert!(validate_delta(i32::MAX).is_err());
        assert!(validate_life(i32::MIN).is_err());
        assert!(validate_life(i32::MAX).is_err());
        assert!(validate_starting_life(i32::MIN).is_err());
        assert!(validate_life(LIFE_MIN).is_ok());
        assert!(validate_life(LIFE_MIN - 1).is_err());
    }
}

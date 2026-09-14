//! The reducer: `apply` one seat's [`Action`] to a [`RoomState`], answering with what
//! changed (so the socket layer can patch every viewer) and, for the two look-at-your-library
//! actions, a private [`Peek`] for the actor alone.
//!
//! Rules of the road (the engine enforces *ownership and shape*, never Magic's rules):
//!
//! - A seat acts only on cards it **controls**. Hand / library / graveyard / exile / command
//!   are always controlled by their owner; a battlefield card by its `controller`.
//! - A seat changes only its **own** life, counters and commander damage taken.
//! - `start`, `set_active`, `end_game` are host-only; `concede` with one seat left finishes
//!   the game with that seat as winner.
//! - Every successful call bumps `state.version` by exactly one and (except
//!   `set_position`) appends one log line, so the socket's patch stream is a total order.
//! - A token (`def.is_token`) leaving the battlefield is deleted (`Changes::removed`).
//! - Moving a card onto the battlefield untaps it, clears `attached_to` on it and on
//!   anything attached to it, and (unless `face_down` says otherwise) turns it face up on
//!   face 0; moving it off the battlefield does the same reset. Cards attached to a card
//!   that leaves the battlefield are detached (they stay where they are).
//! - Ordered zones: `placement` `Top` (default) inserts at index 0, `Bottom` pushes.
//!   `library[0]` / `graveyard[0]` are the tops. `hand` appends at the end.
//! - Bounds: `Draw.n`/`LookTop.n` ≤ `MAX_DRAW` (clamped to what is there), `Mulligan.hand_size`
//!   ≤ `MAX_HAND_SIZE`, `Roll.sides` in 2..=1000, `Chat.text` 1..=`MAX_CHAT` chars after
//!   trim, `Counter.name` 1..=`MAX_COUNTER_NAME`, `CreateToken.count` 1..=20, `x`/`y`
//!   clamped into 0.0..=1.0, `Life.delta` ±1000 with life clamped to -999..=9999.
//!
//! Pure: no I/O, no clocks (`now` is passed in), randomness through [`PlayRng`].

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};

use crate::play::rng::PlayRng;
use crate::play::types::{Action, CardDef, CardId, Peek, RoomState, RoomStatus, SeatId, SeatState};

/// What one applied action touched — the socket layer turns this into per-viewer patches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// Cards whose instance changed (moved, tapped, countered, …) or were created.
    pub cards: BTreeSet<CardId>,
    /// Cards that no longer exist (tokens that left the battlefield).
    pub removed: BTreeSet<CardId>,
    /// Seats whose snapshot changed (zones, life, counters, connection …).
    pub seats: BTreeSet<SeatId>,
    /// Index into `state.log` of the first entry this action appended (`state.log.len()`
    /// when it appended none). The log is capped, so this is computed *after* trimming.
    pub log_from: usize,
}

/// Why an action was refused. `code()` is the wire `error.code`; `Display` is the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionError {
    NotPlaying,
    NotLobby,
    HostOnly,
    NoSuchSeat,
    NoSuchCard,
    NotYourCard,
    WrongZone,
    SeatOut,
    /// `start` with a seat that has no deck (names the seat).
    NoDeck(String),
    TooFewSeats,
    Invalid(&'static str),
}

impl ActionError {
    pub fn code(&self) -> &'static str {
        match self {
            ActionError::NotPlaying => "not_playing",
            ActionError::NotLobby => "not_lobby",
            ActionError::HostOnly => "host_only",
            ActionError::NoSuchSeat => "no_such_seat",
            ActionError::NoSuchCard => "no_such_card",
            ActionError::NotYourCard => "not_your_card",
            ActionError::WrongZone => "wrong_zone",
            ActionError::SeatOut => "seat_out",
            ActionError::NoDeck(_) => "no_deck",
            ActionError::TooFewSeats => "too_few_seats",
            ActionError::Invalid(_) => "invalid",
        }
    }
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionError::NotPlaying => write!(f, "the game is not in progress"),
            ActionError::NotLobby => write!(f, "the game has already started"),
            ActionError::HostOnly => write!(f, "only the host can do that"),
            ActionError::NoSuchSeat => write!(f, "no such seat"),
            ActionError::NoSuchCard => write!(f, "no such card"),
            ActionError::NotYourCard => write!(f, "you don't control that card"),
            ActionError::WrongZone => write!(f, "that card isn't in a zone this applies to"),
            ActionError::SeatOut => write!(f, "you are out of the game"),
            ActionError::NoDeck(name) => write!(f, "{name} has no deck loaded"),
            ActionError::TooFewSeats => write!(f, "at least two seats are needed"),
            ActionError::Invalid(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for ActionError {}

/// What `apply` produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub changes: Changes,
    /// Only for `look_top` / `search_library`, and only for the actor.
    pub peek: Option<Peek>,
}

/// A fresh lobby-state room (version 0, `Lobby`, no cards, turn 0 with no active seat).
pub fn new_room_state(format: &str, starting_life: i32, seats: Vec<SeatState>) -> RoomState {
    let _ = (format, starting_life, seats);
    todo!("engine implementer")
}

/// A seat with its deck loaded, before the game starts (zones empty, life = starting life).
pub fn new_seat_state(
    id: SeatId,
    seat_index: i32,
    name: String,
    is_host: bool,
    deck: Vec<CardDef>,
    deck_name: Option<String>,
    starting_life: i32,
) -> SeatState {
    let _ = (
        id,
        seat_index,
        name,
        is_host,
        deck,
        deck_name,
        starting_life,
    );
    todo!("engine implementer")
}

/// Lobby → playing: every seat needs a non-empty `deck` (else `NoDeck(name)`), at least two
/// seats (`TooFewSeats`), status must be `Lobby` (`NotLobby`). Builds every card instance
/// (commanders → command zone, everything else → library), shuffles each library, draws 7
/// (or the whole library if smaller), picks a random starting seat, sets turn 1 / `Untap`,
/// logs "game started", and bumps the version. `SeatState::deck` is left in place (it is
/// the record of what was loaded).
pub fn start_game(
    state: &mut RoomState,
    rng: &mut PlayRng,
    now: DateTime<Utc>,
) -> Result<Changes, ActionError> {
    let _ = (state, rng, now);
    todo!("engine implementer")
}

/// Apply one action by `actor`. Refuses everything but `Chat` unless `status == Playing`
/// (`NotPlaying`); refuses everything but `Chat` from a seat that is `out` (`SeatOut`).
/// On `Ok` the version has been bumped and the log appended; on `Err` the state is
/// untouched.
pub fn apply(
    state: &mut RoomState,
    actor: SeatId,
    action: Action,
    rng: &mut PlayRng,
    now: DateTime<Utc>,
) -> Result<Outcome, ActionError> {
    let _ = (state, actor, action, rng, now);
    todo!("engine implementer")
}

/// A socket came or went for `seat`: adjusts `connections` (saturating at 0), logs a
/// system line only on the 0↔1 transitions, bumps the version. `Err(NoSuchSeat)` for an
/// unknown seat. Works in every status (the lobby shows presence too).
pub fn set_connected(
    state: &mut RoomState,
    seat: SeatId,
    connected: bool,
    now: DateTime<Utc>,
) -> Result<Changes, ActionError> {
    let _ = (state, seat, connected, now);
    todo!("engine implementer")
}

/// Whether the game accepts table actions.
pub fn is_playing(state: &RoomState) -> bool {
    state.status == RoomStatus::Playing
}

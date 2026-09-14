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
//! The log is third-person and leak-free: a card nobody was shown is "a card" (hand →
//! library, library → hand, anything going face down), a draw says how many rather than
//! what, and `look_top` / `search_library` say only that they happened — the cards
//! themselves ride the private [`Outcome::peek`].
//!
//! Pure: no I/O, no clocks (`now` is passed in), randomness through [`PlayRng`].
//!
//! The reducer is split by what an action touches: [`moves`] (zones, the library and the
//! two private peeks), [`cards`] (a permanent's tap / counters / faces / attachments /
//! control, and minting tokens), [`players`] (life, counters, the turn, dice, chat and the
//! two endings), with [`table`] holding the shared lookups and [`log`] the phrasing. This
//! file keeps the vocabulary ([`Changes`], [`ActionError`], [`Outcome`]), the lifecycle
//! (`new_room_state` / `new_seat_state` / `start_game` / `set_connected`) and the dispatch.

mod cards;
mod log;
mod moves;
mod players;
mod table;

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::play::engine::log::{push_log, settle_log};
use crate::play::engine::table::{seat_pos, turn_order};
use crate::play::rng::PlayRng;
use crate::play::types::{
    Action, CardDef, CardId, CardInstance, LogKind, MIN_PLAYERS, Peek, Phase, RoomState,
    RoomStatus, SeatId, SeatState, TurnState, Zone,
};

/// The opening hand every seat is dealt by [`start_game`].
pub const STARTING_HAND: u32 = 7;
/// How many copies one `create_token` may mint.
pub const MAX_TOKEN_COUNT: u32 = 20;
/// Bounds on a seat's life total and on one `life` step.
pub const MIN_LIFE: i32 = -999;
pub const MAX_LIFE: i32 = 9999;
pub const MAX_LIFE_DELTA: i32 = 1000;
/// Bounds on a `roll`.
pub const MIN_DIE_SIDES: u32 = 2;
pub const MAX_DIE_SIDES: u32 = 1000;

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
    RoomState {
        version: 0,
        status: RoomStatus::Lobby,
        format: format.to_string(),
        starting_life,
        seats,
        cards: BTreeMap::new(),
        turn: TurnState {
            number: 0,
            active_seat: None,
            phase: Phase::Untap,
        },
        log: Vec::new(),
        next_log_id: 1,
        winner: None,
    }
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
    SeatState {
        id,
        seat_index,
        name,
        is_host,
        deck,
        deck_name,
        life: starting_life,
        counters: BTreeMap::new(),
        commander_damage: BTreeMap::new(),
        out: false,
        connections: 0,
        library: Vec::new(),
        hand: Vec::new(),
        battlefield: Vec::new(),
        graveyard: Vec::new(),
        exile: Vec::new(),
        command: Vec::new(),
    }
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
    if state.status != RoomStatus::Lobby {
        return Err(ActionError::NotLobby);
    }
    if (state.seats.len() as i32) < MIN_PLAYERS {
        return Err(ActionError::TooFewSeats);
    }
    if let Some(seat) = state.seats.iter().find(|s| s.deck.is_empty()) {
        return Err(ActionError::NoDeck(seat.name.clone()));
    }

    let log_len = state.log.len();
    let mut changes = Changes::default();

    for index in 0..state.seats.len() {
        let seat_id = state.seats[index].id;
        let deck = state.seats[index].deck.clone();
        let mut library = Vec::with_capacity(deck.len());
        let mut command = Vec::new();
        for def in deck {
            let id = rng.card_id(&state.cards);
            let commander = def.is_commander;
            let instance = CardInstance {
                id,
                def,
                owner: seat_id,
                controller: seat_id,
                zone: if commander {
                    Zone::Command
                } else {
                    Zone::Library
                },
                tapped: false,
                face_down: false,
                face_index: 0,
                revealed: false,
                counters: BTreeMap::new(),
                x: 0.5,
                y: 0.5,
                attached_to: None,
                power_toughness: None,
            };
            state.cards.insert(id, instance);
            changes.cards.insert(id);
            if commander {
                command.push(id);
            } else {
                library.push(id);
            }
        }
        rng.shuffle(&mut library);

        let hand_size = (STARTING_HAND as usize).min(library.len());
        let hand: Vec<CardId> = library.drain(0..hand_size).collect();
        for id in &hand {
            if let Some(card) = state.cards.get_mut(id) {
                card.zone = Zone::Hand;
            }
        }

        let seat = &mut state.seats[index];
        seat.library = library;
        seat.hand = hand;
        seat.command = command;
        changes.seats.insert(seat_id);
    }

    let order = turn_order(state);
    let pick = rng.below(order.len() as u32) as usize;
    let first = state.seats[order[pick]].id;
    let first_name = state.seats[order[pick]].name.clone();

    state.status = RoomStatus::Playing;
    state.turn = TurnState {
        number: 1,
        active_seat: Some(first),
        phase: Phase::Untap,
    };
    state.winner = None;
    push_log(
        state,
        now,
        LogKind::System,
        None,
        format!("Game started — {first_name} goes first"),
    );
    state.version = state.version.wrapping_add(1);
    changes.log_from = settle_log(state, log_len);
    Ok(changes)
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
    let pos = seat_pos(state, actor).ok_or(ActionError::NoSuchSeat)?;
    if !matches!(action, Action::Chat { .. }) {
        if !is_playing(state) {
            return Err(ActionError::NotPlaying);
        }
        if state.seats[pos].out {
            return Err(ActionError::SeatOut);
        }
    }

    let log_len = state.log.len();
    let mut changes = Changes::default();
    let peek = dispatch(state, actor, action, rng, now, &mut changes)?;

    state.version = state.version.wrapping_add(1);
    changes.log_from = settle_log(state, log_len);
    Ok(Outcome { changes, peek })
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
    let pos = seat_pos(state, seat).ok_or(ActionError::NoSuchSeat)?;
    let log_len = state.log.len();
    let before = state.seats[pos].connections;
    let after = if connected {
        before.saturating_add(1)
    } else {
        before.saturating_sub(1)
    };
    state.seats[pos].connections = after;
    let name = state.seats[pos].name.clone();

    let mut changes = Changes::default();
    changes.seats.insert(seat);
    if before == 0 && after > 0 {
        push_log(
            state,
            now,
            LogKind::System,
            Some(seat),
            format!("{name} connected"),
        );
    } else if before > 0 && after == 0 {
        push_log(
            state,
            now,
            LogKind::System,
            Some(seat),
            format!("{name} disconnected"),
        );
    }
    state.version = state.version.wrapping_add(1);
    changes.log_from = settle_log(state, log_len);
    Ok(changes)
}

/// Whether the game accepts table actions.
pub fn is_playing(state: &RoomState) -> bool {
    state.status == RoomStatus::Playing
}

// ---------- The dispatch ----------

/// One action to the module that owns it. Every arm answers `Ok(None)` unless it is one of
/// the two library reads, whose private [`Peek`] rides back to the actor alone.
fn dispatch(
    state: &mut RoomState,
    actor: SeatId,
    action: Action,
    rng: &mut PlayRng,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<Option<Peek>, ActionError> {
    match action {
        Action::Draw { n } => moves::draw(state, actor, n, now, changes).map(|()| None),
        Action::Shuffle => moves::shuffle(state, actor, rng, now, changes).map(|()| None),
        Action::Mulligan { hand_size } => {
            moves::mulligan(state, actor, hand_size, rng, now, changes).map(|()| None)
        }
        Action::MoveCard {
            card,
            zone,
            placement,
            x,
            y,
            face_down,
        } => moves::move_card(
            state, actor, card, zone, placement, x, y, face_down, now, changes,
        )
        .map(|()| None),
        Action::MoveLibraryCard {
            card,
            zone,
            placement,
            x,
            y,
            face_down,
        } => moves::move_library_card(
            state, actor, card, zone, placement, x, y, face_down, now, changes,
        )
        .map(|()| None),
        Action::ReorderTop { cards } => {
            moves::reorder_top(state, actor, cards, now, changes).map(|()| None)
        }
        Action::LookTop { n } => moves::look_top(state, actor, n, now).map(Some),
        Action::SearchLibrary => moves::search_library(state, actor, now).map(Some),

        Action::SetPosition { card, x, y } => {
            cards::set_position(state, actor, card, x, y, changes).map(|()| None)
        }
        Action::Tap { card, tapped } => {
            cards::tap(state, actor, card, tapped, now, changes).map(|()| None)
        }
        Action::UntapAll => cards::untap_all(state, actor, now, changes).map(|()| None),
        Action::ToggleFace { card } => {
            cards::toggle_face(state, actor, card, now, changes).map(|()| None)
        }
        Action::SetFaceDown { card, face_down } => {
            cards::set_face_down(state, actor, card, face_down, now, changes).map(|()| None)
        }
        Action::Counter { card, name, delta } => {
            cards::counter(state, actor, card, name, delta, now, changes).map(|()| None)
        }
        Action::Reveal { card, revealed } => {
            cards::reveal(state, actor, card, revealed, now, changes).map(|()| None)
        }
        Action::Attach { card, to } => {
            cards::attach(state, actor, card, to, now, changes).map(|()| None)
        }
        Action::TakeControl { card, x, y } => {
            cards::take_control(state, actor, card, x, y, now, changes).map(|()| None)
        }
        Action::CreateToken {
            name,
            card_id,
            type_line,
            power_toughness,
            colors,
            x,
            y,
            count,
        } => cards::create_token(
            state,
            actor,
            name,
            card_id,
            type_line,
            power_toughness,
            colors,
            x,
            y,
            count,
            rng,
            now,
            changes,
        )
        .map(|()| None),
        Action::CloneCard { card } => {
            cards::clone_card(state, actor, card, rng, now, changes).map(|()| None)
        }

        Action::Life { delta } => players::life(state, actor, delta, now, changes).map(|()| None),
        Action::PlayerCounter { name, delta } => {
            players::player_counter(state, actor, name, delta, now, changes).map(|()| None)
        }
        Action::CommanderDamage { from_seat, delta } => {
            players::commander_damage(state, actor, from_seat, delta, now, changes).map(|()| None)
        }
        Action::PassTurn => players::pass_turn(state, actor, now).map(|()| None),
        Action::SetPhase { phase } => players::set_phase(state, actor, phase, now).map(|()| None),
        Action::SetActive { seat } => players::set_active(state, actor, seat, now).map(|()| None),
        Action::Roll { sides } => players::roll(state, actor, sides, rng, now).map(|()| None),
        Action::FlipCoin => players::flip_coin(state, actor, rng, now).map(|()| None),
        Action::Chat { text } => players::chat(state, actor, text, now).map(|()| None),
        Action::Concede => players::concede(state, actor, now, changes).map(|()| None),
        Action::EndGame { winner } => players::end_game(state, actor, winner, now).map(|()| None),
    }
}

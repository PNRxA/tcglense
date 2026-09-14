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

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::play::rng::PlayRng;
use crate::play::types::{
    Action, CardDef, CardFace, CardId, CardInstance, LogEntry, LogKind, MAX_CHAT, MAX_COUNTER_NAME,
    MAX_DRAW, MAX_HAND_SIZE, MAX_LOG, MIN_PLAYERS, PLAYER_COUNTERS, Peek, PeekKind, Phase,
    Placement, RoomState, RoomStatus, SeatId, SeatState, TurnState, Zone,
};
use crate::play::view::full_card_view;

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
/// A typed-in token's name / type line bound.
const MAX_TOKEN_TEXT: usize = 120;
/// How far each extra copy of a token is nudged so a stack of them is separable.
const TOKEN_SPREAD: f32 = 0.02;
/// The game slug an empty table falls back to when a token is made before any card exists.
const DEFAULT_GAME: &str = "mtg";

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
        if state.status != RoomStatus::Playing {
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

fn dispatch(
    state: &mut RoomState,
    actor: SeatId,
    action: Action,
    rng: &mut PlayRng,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<Option<Peek>, ActionError> {
    let who = seat_name(state, actor);
    match action {
        Action::Draw { n } => {
            if n == 0 || n > MAX_DRAW {
                return Err(ActionError::Invalid("draw between 1 and 30 cards"));
            }
            let pos = require_seat(state, actor)?;
            let take = (n as usize).min(state.seats[pos].library.len());
            let drawn: Vec<CardId> = state.seats[pos].library.drain(0..take).collect();
            for id in &drawn {
                if let Some(card) = state.cards.get_mut(id) {
                    card.zone = Zone::Hand;
                    card.revealed = false;
                }
                changes.cards.insert(*id);
            }
            state.seats[pos].hand.extend_from_slice(&drawn);
            changes.seats.insert(actor);
            let text = if take == 0 {
                format!("{who} tried to draw from an empty library")
            } else {
                format!("{who} drew {}", cards_phrase(take))
            };
            push_log(state, now, LogKind::Action, Some(actor), text);
            Ok(None)
        }

        Action::Shuffle => {
            let pos = require_seat(state, actor)?;
            let mut library = std::mem::take(&mut state.seats[pos].library);
            rng.shuffle(&mut library);
            state.seats[pos].library = library;
            changes.seats.insert(actor);
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} shuffled their library"),
            );
            Ok(None)
        }

        Action::Mulligan { hand_size } => {
            if hand_size > MAX_HAND_SIZE {
                return Err(ActionError::Invalid("that hand is too big"));
            }
            let pos = require_seat(state, actor)?;
            let old: Vec<CardId> = std::mem::take(&mut state.seats[pos].hand);
            for id in &old {
                if let Some(card) = state.cards.get_mut(id) {
                    card.zone = Zone::Library;
                    card.revealed = false;
                }
                changes.cards.insert(*id);
            }
            let mut library = std::mem::take(&mut state.seats[pos].library);
            library.extend_from_slice(&old);
            rng.shuffle(&mut library);
            let take = (hand_size as usize).min(library.len());
            let hand: Vec<CardId> = library.drain(0..take).collect();
            for id in &hand {
                if let Some(card) = state.cards.get_mut(id) {
                    card.zone = Zone::Hand;
                }
                changes.cards.insert(*id);
            }
            state.seats[pos].library = library;
            state.seats[pos].hand = hand;
            changes.seats.insert(actor);
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} mulliganed to {}", cards_phrase(take)),
            );
            Ok(None)
        }

        Action::MoveCard {
            card,
            zone,
            placement,
            x,
            y,
            face_down,
        } => {
            // A library card is not addressable here: the actor was never shown it, so it
            // reads as unknown (`move_library_card` is the peek-driven door).
            controlled(state, card, actor)?;
            move_card_core(
                state, actor, card, zone, placement, x, y, face_down, now, changes,
            );
            Ok(None)
        }

        Action::MoveLibraryCard {
            card,
            zone,
            placement,
            x,
            y,
            face_down,
        } => {
            let instance = state.cards.get(&card).ok_or(ActionError::NoSuchCard)?;
            if instance.zone != Zone::Library {
                return Err(ActionError::WrongZone);
            }
            if instance.owner != actor {
                return Err(ActionError::NotYourCard);
            }
            move_card_core(
                state, actor, card, zone, placement, x, y, face_down, now, changes,
            );
            Ok(None)
        }

        Action::ReorderTop { cards } => {
            if cards.is_empty() {
                return Err(ActionError::Invalid("no cards given"));
            }
            let pos = require_seat(state, actor)?;
            let unique: BTreeSet<CardId> = cards.iter().copied().collect();
            if unique.len() != cards.len() {
                return Err(ActionError::Invalid("those cards repeat"));
            }
            let library = &state.seats[pos].library;
            if cards.len() > library.len() {
                return Err(ActionError::Invalid(
                    "that is more cards than the library has",
                ));
            }
            let top: BTreeSet<CardId> = library[0..cards.len()].iter().copied().collect();
            if top != unique {
                return Err(ActionError::Invalid(
                    "those aren't the cards on top of the library",
                ));
            }
            let n = cards.len();
            state.seats[pos].library[0..n].copy_from_slice(&cards);
            changes.seats.insert(actor);
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!(
                    "{who} rearranged the top {} of their library",
                    cards_phrase(n)
                ),
            );
            Ok(None)
        }

        Action::SetPosition { card, x, y } => {
            let instance = controlled(state, card, actor)?;
            if instance.zone != Zone::Battlefield {
                return Err(ActionError::WrongZone);
            }
            if let Some(instance) = state.cards.get_mut(&card) {
                instance.x = clamp01(x);
                instance.y = clamp01(y);
            }
            changes.cards.insert(card);
            Ok(None)
        }

        Action::Tap { card, tapped } => {
            let instance = controlled(state, card, actor)?;
            if instance.zone != Zone::Battlefield {
                return Err(ActionError::WrongZone);
            }
            let label = battlefield_label(instance);
            if let Some(instance) = state.cards.get_mut(&card) {
                instance.tapped = tapped;
            }
            changes.cards.insert(card);
            let verb = if tapped { "tapped" } else { "untapped" };
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} {verb} {label}"),
            );
            Ok(None)
        }

        Action::UntapAll => {
            let ids: Vec<CardId> = state
                .cards
                .values()
                .filter(|c| c.zone == Zone::Battlefield && c.controller == actor && c.tapped)
                .map(|c| c.id)
                .collect();
            for id in ids {
                if let Some(card) = state.cards.get_mut(&id) {
                    card.tapped = false;
                }
                changes.cards.insert(id);
            }
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} untapped everything"),
            );
            Ok(None)
        }

        Action::ToggleFace { card } => {
            let instance = controlled(state, card, actor)?;
            let label = battlefield_label(instance);
            let mut flipped = false;
            if let Some(instance) = state.cards.get_mut(&card)
                && instance.def.faces.len() > 1
            {
                instance.face_index = u8::from(instance.face_index == 0);
                flipped = true;
            }
            changes.cards.insert(card);
            let text = if flipped {
                format!("{who} transformed {label}")
            } else {
                format!("{who} turned {label} over")
            };
            push_log(state, now, LogKind::Action, Some(actor), text);
            Ok(None)
        }

        Action::SetFaceDown { card, face_down } => {
            let instance = controlled(state, card, actor)?;
            if instance.zone != Zone::Battlefield {
                return Err(ActionError::WrongZone);
            }
            // Turning an already-hidden card face down again must not name it.
            let label = if face_down && instance.face_down {
                "a card".to_string()
            } else {
                instance.def.name.clone()
            };
            if let Some(instance) = state.cards.get_mut(&card) {
                instance.face_down = face_down;
                if face_down {
                    instance.face_index = 0;
                }
            }
            changes.cards.insert(card);
            let verb = if face_down { "face down" } else { "face up" };
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} turned {label} {verb}"),
            );
            Ok(None)
        }

        Action::Counter { card, name, delta } => {
            let trimmed = name.trim().to_string();
            if trimmed.is_empty() || trimmed.chars().count() > MAX_COUNTER_NAME {
                return Err(ActionError::Invalid("that counter name is out of bounds"));
            }
            if delta == 0 {
                return Err(ActionError::Invalid("a counter change can't be zero"));
            }
            let instance = controlled(state, card, actor)?;
            let label = battlefield_label(instance);
            let mut total = 0;
            if let Some(instance) = state.cards.get_mut(&card) {
                total = instance.counters.get(&trimmed).copied().unwrap_or(0) + delta;
                if total <= 0 {
                    instance.counters.remove(&trimmed);
                    total = 0;
                } else {
                    instance.counters.insert(trimmed.clone(), total);
                }
            }
            changes.cards.insert(card);
            let verb = if delta > 0 { "added" } else { "removed" };
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!(
                    "{who} {verb} {} {trimmed} counter{} on {label} ({total})",
                    delta.abs(),
                    if delta.abs() == 1 { "" } else { "s" }
                ),
            );
            Ok(None)
        }

        Action::Reveal { card, revealed } => {
            let instance = controlled(state, card, actor)?;
            if instance.zone != Zone::Hand {
                return Err(ActionError::WrongZone);
            }
            let name = instance.def.name.clone();
            if let Some(instance) = state.cards.get_mut(&card) {
                instance.revealed = revealed;
            }
            changes.cards.insert(card);
            changes.seats.insert(actor);
            let text = if revealed {
                format!("{who} revealed {name} from their hand")
            } else {
                format!("{who} stopped revealing a card")
            };
            push_log(state, now, LogKind::Action, Some(actor), text);
            Ok(None)
        }

        Action::Attach { card, to } => {
            let instance = controlled(state, card, actor)?;
            if instance.zone != Zone::Battlefield {
                return Err(ActionError::WrongZone);
            }
            let label = battlefield_label(instance);
            let target_label = match to {
                None => None,
                Some(target) => {
                    if target == card {
                        return Err(ActionError::Invalid("a card can't attach to itself"));
                    }
                    let other = state.cards.get(&target).ok_or(ActionError::NoSuchCard)?;
                    if other.zone != Zone::Battlefield {
                        return Err(ActionError::WrongZone);
                    }
                    if attachment_loops(state, target, card) {
                        return Err(ActionError::Invalid("that would attach in a loop"));
                    }
                    Some(battlefield_label(other))
                }
            };
            if let Some(instance) = state.cards.get_mut(&card) {
                instance.attached_to = to;
            }
            changes.cards.insert(card);
            let text = match target_label {
                Some(target) => format!("{who} attached {label} to {target}"),
                None => format!("{who} unattached {label}"),
            };
            push_log(state, now, LogKind::Action, Some(actor), text);
            Ok(None)
        }

        Action::TakeControl { card, x, y } => {
            let instance = state.cards.get(&card).ok_or(ActionError::NoSuchCard)?;
            if instance.zone != Zone::Battlefield {
                return Err(ActionError::WrongZone);
            }
            if instance.controller == actor {
                return Err(ActionError::Invalid("you already control that card"));
            }
            let from = instance.controller;
            let label = battlefield_label(instance);
            remove_from_zone(state, from, Zone::Battlefield, card);
            if let Some(instance) = state.cards.get_mut(&card) {
                instance.controller = actor;
                instance.x = clamp01(x);
                instance.y = clamp01(y);
            }
            if let Some(pos) = seat_pos(state, actor) {
                state.seats[pos].battlefield.push(card);
            }
            changes.cards.insert(card);
            changes.seats.insert(from);
            changes.seats.insert(actor);
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} took control of {label}"),
            );
            Ok(None)
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
        } => {
            let name = name.trim().to_string();
            if name.is_empty() || name.chars().count() > MAX_TOKEN_TEXT {
                return Err(ActionError::Invalid("that token name is out of bounds"));
            }
            if !(1..=MAX_TOKEN_COUNT).contains(&count) {
                return Err(ActionError::Invalid("make between 1 and 20 tokens"));
            }
            if let Some(line) = &type_line
                && line.chars().count() > MAX_TOKEN_TEXT
            {
                return Err(ActionError::Invalid("that type line is too long"));
            }
            let pos = require_seat(state, actor)?;
            let def = CardDef {
                card_id,
                game: table_game(state),
                name: name.clone(),
                faces: vec![CardFace {
                    name: name.clone(),
                    mana_cost: None,
                    type_line: type_line.clone(),
                    oracle_text: None,
                    power: None,
                    toughness: None,
                    loyalty: None,
                }],
                back_image: false,
                colors,
                cmc: None,
                is_commander: false,
                is_token: true,
            };
            for i in 0..count {
                let id = rng.card_id(&state.cards);
                let instance = CardInstance {
                    id,
                    def: def.clone(),
                    owner: actor,
                    controller: actor,
                    zone: Zone::Battlefield,
                    tapped: false,
                    face_down: false,
                    face_index: 0,
                    revealed: false,
                    counters: BTreeMap::new(),
                    x: clamp01(x + TOKEN_SPREAD * i as f32),
                    y: clamp01(y),
                    attached_to: None,
                    power_toughness: power_toughness.clone(),
                };
                state.cards.insert(id, instance);
                state.seats[pos].battlefield.push(id);
                changes.cards.insert(id);
            }
            changes.seats.insert(actor);
            let text = if count == 1 {
                format!("{who} created a {name} token")
            } else {
                format!("{who} created {count} {name} tokens")
            };
            push_log(state, now, LogKind::Action, Some(actor), text);
            Ok(None)
        }

        Action::CloneCard { card } => {
            let instance = controlled(state, card, actor)?;
            if instance.zone != Zone::Battlefield {
                return Err(ActionError::WrongZone);
            }
            let mut def = instance.def.clone();
            def.is_token = true;
            def.is_commander = false;
            let label = battlefield_label(instance);
            let (x, y, power_toughness) =
                (instance.x, instance.y, instance.power_toughness.clone());
            let pos = require_seat(state, actor)?;
            let id = rng.card_id(&state.cards);
            let copy = CardInstance {
                id,
                def,
                owner: actor,
                controller: actor,
                zone: Zone::Battlefield,
                tapped: false,
                face_down: false,
                face_index: 0,
                revealed: false,
                counters: BTreeMap::new(),
                x: clamp01(x + TOKEN_SPREAD),
                y: clamp01(y),
                attached_to: None,
                power_toughness,
            };
            state.cards.insert(id, copy);
            state.seats[pos].battlefield.push(id);
            changes.cards.insert(id);
            changes.seats.insert(actor);
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} copied {label}"),
            );
            Ok(None)
        }

        Action::Life { delta } => {
            if delta == 0 || delta.abs() > MAX_LIFE_DELTA {
                return Err(ActionError::Invalid("that life change is out of bounds"));
            }
            let pos = require_seat(state, actor)?;
            let life = state.seats[pos]
                .life
                .saturating_add(delta)
                .clamp(MIN_LIFE, MAX_LIFE);
            state.seats[pos].life = life;
            changes.seats.insert(actor);
            let verb = if delta > 0 { "gained" } else { "lost" };
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} {verb} {} life ({life})", delta.abs()),
            );
            Ok(None)
        }

        Action::PlayerCounter { name, delta } => {
            let name = name.trim().to_string();
            if !PLAYER_COUNTERS.contains(&name.as_str()) {
                return Err(ActionError::Invalid("no such player counter"));
            }
            if delta == 0 || delta.abs() > MAX_LIFE_DELTA {
                return Err(ActionError::Invalid("that counter change is out of bounds"));
            }
            let pos = require_seat(state, actor)?;
            let seat = &mut state.seats[pos];
            let total = (seat.counters.get(&name).copied().unwrap_or(0) + delta).max(0);
            if total == 0 {
                seat.counters.remove(&name);
            } else {
                seat.counters.insert(name.clone(), total);
            }
            changes.seats.insert(actor);
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who}'s {name} is now {total}"),
            );
            Ok(None)
        }

        Action::CommanderDamage { from_seat, delta } => {
            if delta == 0 || delta.abs() > MAX_LIFE_DELTA {
                return Err(ActionError::Invalid("that damage change is out of bounds"));
            }
            let source = seat_pos(state, from_seat).ok_or(ActionError::NoSuchSeat)?;
            let source_name = state.seats[source].name.clone();
            let pos = require_seat(state, actor)?;
            let seat = &mut state.seats[pos];
            let total =
                (seat.commander_damage.get(&from_seat).copied().unwrap_or(0) + delta).max(0);
            if total == 0 {
                seat.commander_damage.remove(&from_seat);
            } else {
                seat.commander_damage.insert(from_seat, total);
            }
            changes.seats.insert(actor);
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} has taken {total} commander damage from {source_name}"),
            );
            Ok(None)
        }

        Action::PassTurn => {
            let order = turn_order(state);
            if order.is_empty() {
                return Err(ActionError::NoSuchSeat);
            }
            let current = state
                .turn
                .active_seat
                .and_then(|id| order.iter().position(|i| state.seats[*i].id == id));
            let start = current.map(|p| p + 1).unwrap_or(0);
            let mut next = None;
            for step in 0..order.len() {
                let index = order[(start + step) % order.len()];
                if !state.seats[index].out {
                    next = Some(index);
                    break;
                }
            }
            let Some(index) = next else {
                return Err(ActionError::Invalid("no seat is left to take a turn"));
            };
            let next_id = state.seats[index].id;
            let next_name = state.seats[index].name.clone();
            state.turn.active_seat = Some(next_id);
            state.turn.phase = Phase::Untap;
            state.turn.number = state.turn.number.saturating_add(1);
            let number = state.turn.number;
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} passed the turn to {next_name} (turn {number})"),
            );
            Ok(None)
        }

        Action::SetPhase { phase } => {
            state.turn.phase = phase;
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} moved to {}", phase.as_str()),
            );
            Ok(None)
        }

        Action::SetActive { seat } => {
            require_host(state, actor)?;
            let pos = seat_pos(state, seat).ok_or(ActionError::NoSuchSeat)?;
            let name = state.seats[pos].name.clone();
            state.turn.active_seat = Some(seat);
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} made it {name}'s turn"),
            );
            Ok(None)
        }

        Action::Roll { sides } => {
            if !(MIN_DIE_SIDES..=MAX_DIE_SIDES).contains(&sides) {
                return Err(ActionError::Invalid("a die has between 2 and 1000 sides"));
            }
            let value = rng.below(sides) + 1;
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} rolled a d{sides}: {value}"),
            );
            Ok(None)
        }

        Action::FlipCoin => {
            let heads = rng.below(2) == 0;
            let face = if heads { "heads" } else { "tails" };
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} flipped a coin: {face}"),
            );
            Ok(None)
        }

        Action::Chat { text } => {
            let text = text.trim().to_string();
            let length = text.chars().count();
            if length == 0 || length > MAX_CHAT {
                return Err(ActionError::Invalid("that message is out of bounds"));
            }
            push_log(state, now, LogKind::Chat, Some(actor), text);
            Ok(None)
        }

        Action::LookTop { n } => {
            if n == 0 || n > MAX_DRAW {
                return Err(ActionError::Invalid("look at between 1 and 30 cards"));
            }
            let pos = require_seat(state, actor)?;
            let take = (n as usize).min(state.seats[pos].library.len());
            let ids: Vec<CardId> = state.seats[pos].library[0..take].to_vec();
            let cards = ids
                .iter()
                .filter_map(|id| state.cards.get(id))
                .map(full_card_view)
                .collect();
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!(
                    "{who} looked at the top {} of their library",
                    cards_phrase(take)
                ),
            );
            Ok(Some(Peek {
                cards,
                kind: PeekKind::LookTop,
            }))
        }

        Action::SearchLibrary => {
            let pos = require_seat(state, actor)?;
            let ids: Vec<CardId> = state.seats[pos].library.clone();
            let cards = ids
                .iter()
                .filter_map(|id| state.cards.get(id))
                .map(full_card_view)
                .collect();
            push_log(
                state,
                now,
                LogKind::Action,
                Some(actor),
                format!("{who} searched their library"),
            );
            Ok(Some(Peek {
                cards,
                kind: PeekKind::SearchLibrary,
            }))
        }

        Action::Concede => {
            let pos = require_seat(state, actor)?;
            state.seats[pos].out = true;
            changes.seats.insert(actor);
            let alive: Vec<usize> = (0..state.seats.len())
                .filter(|i| !state.seats[*i].out)
                .collect();
            let text = if alive.len() == 1 {
                let winner = state.seats[alive[0]].id;
                let winner_name = state.seats[alive[0]].name.clone();
                state.status = RoomStatus::Finished;
                state.winner = Some(winner);
                changes.seats.insert(winner);
                format!("{who} conceded — {winner_name} wins")
            } else {
                format!("{who} conceded")
            };
            push_log(state, now, LogKind::Action, Some(actor), text);
            Ok(None)
        }

        Action::EndGame { winner } => {
            require_host(state, actor)?;
            let winner_name = match winner {
                Some(seat) => {
                    let pos = seat_pos(state, seat).ok_or(ActionError::NoSuchSeat)?;
                    Some(state.seats[pos].name.clone())
                }
                None => None,
            };
            state.status = RoomStatus::Finished;
            state.winner = winner;
            let text = match winner_name {
                Some(name) => format!("{who} ended the game — {name} wins"),
                None => format!("{who} ended the game"),
            };
            push_log(state, now, LogKind::System, Some(actor), text);
            Ok(None)
        }
    }
}

// ---------- Moving ----------

/// The shared body of `move_card` / `move_library_card`, after the caller has established
/// that `id` exists and the actor may move it.
#[allow(clippy::too_many_arguments)]
fn move_card_core(
    state: &mut RoomState,
    actor: SeatId,
    id: CardId,
    to: Zone,
    placement: Option<Placement>,
    x: Option<f32>,
    y: Option<f32>,
    face_down: Option<bool>,
    now: DateTime<Utc>,
    changes: &mut Changes,
) {
    let Some(card) = state.cards.get(&id) else {
        return;
    };
    let from = card.zone;
    let holder = holder_of(card);
    let owner = card.owner;
    let was_face_down = card.face_down;
    let is_token = card.def.is_token;
    let name = card.def.name.clone();
    let who = seat_name(state, actor);

    let going_face_down = to == Zone::Battlefield && face_down == Some(true);
    let label = move_label(&name, from, to, was_face_down, going_face_down);
    let text = format!("{who} moved {label} to the {}", zone_label(to));

    // Only the battlefield is the *controller's*: every other zone belongs to the card's
    // owner, so a borrowed permanent goes to its owner's graveyard and the loan ends there.
    let destination = if to == Zone::Battlefield {
        actor
    } else {
        owner
    };

    remove_from_zone(state, holder, from, id);
    changes.seats.insert(holder);
    changes.seats.insert(destination);

    if from == Zone::Battlefield {
        // Whatever was strapped to this card stays where it is, unattached.
        let attached: Vec<CardId> = state
            .cards
            .values()
            .filter(|c| c.attached_to == Some(id))
            .map(|c| c.id)
            .collect();
        for other in attached {
            if let Some(card) = state.cards.get_mut(&other) {
                card.attached_to = None;
            }
            changes.cards.insert(other);
        }
    }

    let leaving_battlefield = from == Zone::Battlefield && to != Zone::Battlefield;
    if leaving_battlefield && is_token {
        state.cards.remove(&id);
        changes.removed.insert(id);
        push_log(state, now, LogKind::Action, Some(actor), text);
        return;
    }

    let touching_battlefield = from == Zone::Battlefield || to == Zone::Battlefield;
    if let Some(card) = state.cards.get_mut(&id) {
        card.zone = to;
        card.controller = destination;
        card.revealed = false;
        if touching_battlefield {
            card.tapped = false;
            card.attached_to = None;
            card.face_index = 0;
            card.face_down = going_face_down;
        }
        if leaving_battlefield {
            card.counters.clear();
        }
        if to == Zone::Battlefield {
            card.x = clamp01(x.unwrap_or(0.5));
            card.y = clamp01(y.unwrap_or(0.5));
        }
    }

    if let Some(pos) = seat_pos(state, destination) {
        let list = zone_vec_mut(&mut state.seats[pos], to);
        match to {
            Zone::Hand | Zone::Battlefield => list.push(id),
            _ => match placement.unwrap_or(Placement::Top) {
                Placement::Top => list.insert(0, id),
                Placement::Bottom => list.push(id),
            },
        }
    }
    changes.cards.insert(id);
    push_log(state, now, LogKind::Action, Some(actor), text);
}

/// How the log names a card that moved, without leaking what nobody was shown.
fn move_label(
    name: &str,
    from: Zone,
    to: Zone,
    was_face_down: bool,
    going_face_down: bool,
) -> String {
    let hidden_source = matches!(from, Zone::Library | Zone::Hand) || was_face_down;
    let hidden_destination = matches!(to, Zone::Library | Zone::Hand);
    if going_face_down || (hidden_source && hidden_destination) {
        "a card".to_string()
    } else {
        name.to_string()
    }
}

// ---------- Small shared pieces ----------

fn seat_pos(state: &RoomState, seat: SeatId) -> Option<usize> {
    state.seats.iter().position(|s| s.id == seat)
}

fn require_seat(state: &RoomState, seat: SeatId) -> Result<usize, ActionError> {
    seat_pos(state, seat).ok_or(ActionError::NoSuchSeat)
}

fn require_host(state: &RoomState, seat: SeatId) -> Result<(), ActionError> {
    let pos = require_seat(state, seat)?;
    if state.seats[pos].is_host {
        Ok(())
    } else {
        Err(ActionError::HostOnly)
    }
}

fn seat_name(state: &RoomState, seat: SeatId) -> String {
    seat_pos(state, seat)
        .map(|p| state.seats[p].name.clone())
        .unwrap_or_default()
}

/// Seat positions in table order (`seat_index`, then id as the tie-break).
fn turn_order(state: &RoomState) -> Vec<usize> {
    let mut order: Vec<usize> = (0..state.seats.len()).collect();
    order.sort_by_key(|i| (state.seats[*i].seat_index, state.seats[*i].id));
    order
}

/// The seat whose zone list holds this card: the controller on the battlefield, the owner
/// everywhere else.
fn holder_of(card: &CardInstance) -> SeatId {
    if card.zone == Zone::Battlefield {
        card.controller
    } else {
        card.owner
    }
}

/// Look up a card `actor` controls. A library card is never addressable this way — the
/// actor was not shown it, so it reads as unknown.
fn controlled(state: &RoomState, id: CardId, actor: SeatId) -> Result<&CardInstance, ActionError> {
    let card = state.cards.get(&id).ok_or(ActionError::NoSuchCard)?;
    if card.zone == Zone::Library {
        return Err(ActionError::NoSuchCard);
    }
    if holder_of(card) != actor {
        return Err(ActionError::NotYourCard);
    }
    Ok(card)
}

fn zone_vec_mut(seat: &mut SeatState, zone: Zone) -> &mut Vec<CardId> {
    match zone {
        Zone::Library => &mut seat.library,
        Zone::Hand => &mut seat.hand,
        Zone::Battlefield => &mut seat.battlefield,
        Zone::Graveyard => &mut seat.graveyard,
        Zone::Exile => &mut seat.exile,
        Zone::Command => &mut seat.command,
    }
}

fn remove_from_zone(state: &mut RoomState, holder: SeatId, zone: Zone, id: CardId) {
    if let Some(pos) = seat_pos(state, holder) {
        let list = zone_vec_mut(&mut state.seats[pos], zone);
        if let Some(at) = list.iter().position(|c| *c == id) {
            list.remove(at);
        }
    }
}

/// Would attaching something to `target` reach `card` again? Walks the `attached_to` chain.
fn attachment_loops(state: &RoomState, target: CardId, card: CardId) -> bool {
    let mut seen: BTreeSet<CardId> = BTreeSet::new();
    let mut at = Some(target);
    while let Some(id) = at {
        if id == card {
            return true;
        }
        if !seen.insert(id) {
            // An existing loop in the data: stop rather than spin.
            return true;
        }
        at = state.cards.get(&id).and_then(|c| c.attached_to);
    }
    false
}

/// A battlefield card the table can't see is never named in the log.
fn battlefield_label(card: &CardInstance) -> String {
    if card.zone == Zone::Battlefield && card.face_down {
        "a face-down card".to_string()
    } else {
        card.def.name.clone()
    }
}

fn zone_label(zone: Zone) -> &'static str {
    match zone {
        Zone::Command => "command zone",
        other => other.as_str(),
    }
}

fn cards_phrase(n: usize) -> String {
    if n == 1 {
        "1 card".to_string()
    } else {
        format!("{n} cards")
    }
}

fn clamp01(v: f32) -> f32 {
    if v.is_nan() { 0.5 } else { v.clamp(0.0, 1.0) }
}

/// The catalog game the table's cards come from; a token minted before any card exists
/// falls back to the default.
fn table_game(state: &RoomState) -> String {
    state
        .cards
        .values()
        .next()
        .map(|c| c.def.game.clone())
        .or_else(|| {
            state
                .seats
                .iter()
                .find_map(|s| s.deck.first().map(|d| d.game.clone()))
        })
        .unwrap_or_else(|| DEFAULT_GAME.to_string())
}

fn push_log(
    state: &mut RoomState,
    now: DateTime<Utc>,
    kind: LogKind,
    seat: Option<SeatId>,
    text: String,
) {
    let id = state.next_log_id;
    state.next_log_id = state.next_log_id.wrapping_add(1);
    state.log.push(LogEntry {
        id,
        at: now,
        kind,
        seat,
        text,
    });
}

/// Trim the log to `MAX_LOG` and answer the index of the first entry this call appended —
/// computed *after* the trim, so `state.log[log_from..]` is exactly the new entries.
fn settle_log(state: &mut RoomState, len_before: usize) -> usize {
    let added = state.log.len().saturating_sub(len_before);
    if state.log.len() > MAX_LOG {
        let excess = state.log.len() - MAX_LOG;
        state.log.drain(0..excess);
    }
    state.log.len().saturating_sub(added)
}

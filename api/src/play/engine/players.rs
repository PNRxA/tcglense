//! What a seat does to itself and to the table's clock: life, the three player counters,
//! commander damage taken, whose turn it is, dice and chat, and the two ways a game ends.
//!
//! A seat only ever changes its **own** totals — commander damage is keyed by the source
//! seat but stored on, and written by, the seat that took it. The host powers
//! (`set_active`, `end_game`) check [`require_host`] first.

use chrono::{DateTime, Utc};

use crate::play::engine::log::push_log;
use crate::play::engine::table::{require_host, require_seat, seat_name, seat_pos, turn_order};
use crate::play::engine::{ActionError, Changes, MAX_LIFE, MAX_TOTAL, MIN_LIFE, valid_delta};
use crate::play::engine::{MAX_DIE_SIDES, MIN_DIE_SIDES};
use crate::play::rng::PlayRng;
use crate::play::types::{
    LogKind, MAX_CHAT, PLAYER_COUNTERS, Phase, RoomState, RoomStatus, SeatId,
};

pub(super) fn life(
    state: &mut RoomState,
    actor: SeatId,
    delta: i32,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
    // Bounded before anything touches it: `i32::MIN.abs()` panics.
    if !valid_delta(delta) {
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
        format!("{who} {verb} {} life ({life})", delta.unsigned_abs()),
    );

    Ok(())
}

/// One of [`PLAYER_COUNTERS`]; the result floors at zero (which removes the key) and
/// clamps at [`MAX_TOTAL`].
pub(super) fn player_counter(
    state: &mut RoomState,
    actor: SeatId,
    name: String,
    delta: i32,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
    let name = name.trim().to_string();
    if !PLAYER_COUNTERS.contains(&name.as_str()) {
        return Err(ActionError::Invalid("no such player counter"));
    }
    if !valid_delta(delta) {
        return Err(ActionError::Invalid("that counter change is out of bounds"));
    }
    let pos = require_seat(state, actor)?;
    let seat = &mut state.seats[pos];
    let total = seat
        .counters
        .get(&name)
        .copied()
        .unwrap_or(0)
        .saturating_add(delta)
        .clamp(0, MAX_TOTAL);
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

    Ok(())
}

/// Commander damage the actor **took** from `from_seat`'s commander; the total floors at
/// zero (which removes the entry) and clamps at [`MAX_TOTAL`].
pub(super) fn commander_damage(
    state: &mut RoomState,
    actor: SeatId,
    from_seat: SeatId,
    delta: i32,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
    if !valid_delta(delta) {
        return Err(ActionError::Invalid("that damage change is out of bounds"));
    }
    let source = seat_pos(state, from_seat).ok_or(ActionError::NoSuchSeat)?;
    let source_name = state.seats[source].name.clone();
    let pos = require_seat(state, actor)?;
    let seat = &mut state.seats[pos];
    let total = seat
        .commander_damage
        .get(&from_seat)
        .copied()
        .unwrap_or(0)
        .saturating_add(delta)
        .clamp(0, MAX_TOTAL);
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

    Ok(())
}

/// Hand the turn to the next seat still in the game, wrapping.
pub(super) fn pass_turn(
    state: &mut RoomState,
    actor: SeatId,
    now: DateTime<Utc>,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

pub(super) fn set_phase(
    state: &mut RoomState,
    actor: SeatId,
    phase: Phase,
    now: DateTime<Utc>,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
    state.turn.phase = phase;
    push_log(
        state,
        now,
        LogKind::Action,
        Some(actor),
        format!("{who} moved to {}", phase.as_str()),
    );

    Ok(())
}

/// Host only: point the turn at a seat without advancing the count.
pub(super) fn set_active(
    state: &mut RoomState,
    actor: SeatId,
    seat: SeatId,
    now: DateTime<Utc>,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

pub(super) fn roll(
    state: &mut RoomState,
    actor: SeatId,
    sides: u32,
    rng: &mut PlayRng,
    now: DateTime<Utc>,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

pub(super) fn flip_coin(
    state: &mut RoomState,
    actor: SeatId,
    rng: &mut PlayRng,
    now: DateTime<Utc>,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
    let heads = rng.below(2) == 0;
    let face = if heads { "heads" } else { "tails" };
    push_log(
        state,
        now,
        LogKind::Action,
        Some(actor),
        format!("{who} flipped a coin: {face}"),
    );

    Ok(())
}

/// The one action a finished game and an out seat still allow.
pub(super) fn chat(
    state: &mut RoomState,
    actor: SeatId,
    text: String,
    now: DateTime<Utc>,
) -> Result<(), ActionError> {
    let text = text.trim().to_string();
    let length = text.chars().count();
    if length == 0 || length > MAX_CHAT {
        return Err(ActionError::Invalid("that message is out of bounds"));
    }
    push_log(state, now, LogKind::Chat, Some(actor), text);

    Ok(())
}

/// The actor drops out; the last seat standing wins.
pub(super) fn concede(
    state: &mut RoomState,
    actor: SeatId,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

/// Host only: finish the game, optionally naming a winner.
pub(super) fn end_game(
    state: &mut RoomState,
    actor: SeatId,
    winner: Option<SeatId>,
    now: DateTime<Utc>,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

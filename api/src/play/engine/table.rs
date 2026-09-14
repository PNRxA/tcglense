//! The small shared lookups every action does over a [`RoomState`]: finding a seat, the
//! table's turn order, which seat's list holds a card, and the ownership check that decides
//! whether the actor may touch it at all.
//!
//! Nothing here mutates anything interesting — it is the vocabulary the reducer's arms are
//! written in, extracted so the ownership rule (`holder_of` + [`controlled`]) has exactly
//! one definition.

use crate::play::engine::ActionError;
use crate::play::types::{CardId, CardInstance, RoomState, SeatId, SeatState, Zone};

pub(super) fn seat_pos(state: &RoomState, seat: SeatId) -> Option<usize> {
    state.seats.iter().position(|s| s.id == seat)
}

pub(super) fn require_seat(state: &RoomState, seat: SeatId) -> Result<usize, ActionError> {
    seat_pos(state, seat).ok_or(ActionError::NoSuchSeat)
}

pub(super) fn require_host(state: &RoomState, seat: SeatId) -> Result<(), ActionError> {
    let pos = require_seat(state, seat)?;
    if state.seats[pos].is_host {
        Ok(())
    } else {
        Err(ActionError::HostOnly)
    }
}

pub(super) fn seat_name(state: &RoomState, seat: SeatId) -> String {
    seat_pos(state, seat)
        .map(|p| state.seats[p].name.clone())
        .unwrap_or_default()
}

/// Seat positions in table order (`seat_index`, then id as the tie-break).
pub(super) fn turn_order(state: &RoomState) -> Vec<usize> {
    let mut order: Vec<usize> = (0..state.seats.len()).collect();
    order.sort_by_key(|i| (state.seats[*i].seat_index, state.seats[*i].id));
    order
}

/// The seat whose zone list holds this card: the controller on the battlefield, the owner
/// everywhere else.
pub(super) fn holder_of(card: &CardInstance) -> SeatId {
    if card.zone == Zone::Battlefield {
        card.controller
    } else {
        card.owner
    }
}

/// Look up a card `actor` controls. A library card is never addressable this way — the
/// actor was not shown it, so it reads as unknown.
pub(super) fn controlled(
    state: &RoomState,
    id: CardId,
    actor: SeatId,
) -> Result<&CardInstance, ActionError> {
    let card = state.cards.get(&id).ok_or(ActionError::NoSuchCard)?;
    if card.zone == Zone::Library {
        return Err(ActionError::NoSuchCard);
    }
    if holder_of(card) != actor {
        return Err(ActionError::NotYourCard);
    }
    Ok(card)
}

pub(super) fn zone_vec_mut(seat: &mut SeatState, zone: Zone) -> &mut Vec<CardId> {
    match zone {
        Zone::Library => &mut seat.library,
        Zone::Hand => &mut seat.hand,
        Zone::Battlefield => &mut seat.battlefield,
        Zone::Graveyard => &mut seat.graveyard,
        Zone::Exile => &mut seat.exile,
        Zone::Command => &mut seat.command,
    }
}

pub(super) fn remove_from_zone(state: &mut RoomState, holder: SeatId, zone: Zone, id: CardId) {
    if let Some(pos) = seat_pos(state, holder) {
        let list = zone_vec_mut(&mut state.seats[pos], zone);
        if let Some(at) = list.iter().position(|c| *c == id) {
            list.remove(at);
        }
    }
}

pub(super) fn clamp01(v: f32) -> f32 {
    if v.is_nan() { 0.5 } else { v.clamp(0.0, 1.0) }
}

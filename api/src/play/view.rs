//! What one viewer may see. The only place hidden information is filtered — the engine and
//! the persisted state hold everything in the clear, and the socket layer never sends a
//! `RoomState`, only what these functions return.
//!
//! Visibility rules:
//! - **Library**: never visible to anyone (a count). A `look_top` / `search_library` answer
//!   is a one-off [`Peek`](crate::play::types::Peek), not a change in visibility.
//! - **Hand**: the owner sees ids + cards; everyone else sees a count, plus any card the
//!   owner `revealed` (which then appears in `SeatSnapshot::hand` for them too).
//! - **Battlefield / graveyard / exile / command**: public — except a `face_down`
//!   battlefield card, which its controller sees whole and everyone else sees as an id with
//!   `def: None`, no `face_index` and no `power_toughness` (its `counters` stay: they are
//!   public on a face-down permanent in paper too).
//! - A spectator (`viewer == None`) sees what "everyone else" sees.

use crate::play::engine::Changes;
use crate::play::types::{
    CardId, CardInstance, CardView, Patch, RoomState, SNAPSHOT_LOG, SeatId, SeatSnapshot,
    SeatState, Snapshot, Zone,
};

/// The card in the clear — every field, `def` included. Used for the viewers who are
/// allowed to see it, and for the private `peek` answers (which show library cards to their
/// owner without changing anyone's visibility).
pub fn full_card_view(card: &CardInstance) -> CardView {
    CardView {
        id: card.id,
        def: Some(card.def.clone()),
        owner: card.owner,
        controller: card.controller,
        zone: card.zone,
        tapped: card.tapped,
        face_down: card.face_down,
        face_index: card.face_index,
        revealed: card.revealed,
        counters: card.counters.clone(),
        x: card.x,
        y: card.y,
        attached_to: card.attached_to,
        power_toughness: card.power_toughness.clone(),
    }
}

/// The card as `viewer` sees it; `None` when the viewer may not know it is there at all
/// (in a library, or in someone else's hand and not revealed).
///
/// A face-down permanent someone else controls keeps only what a sleeve face-down on a real
/// table shows: it loses its `def`, its `face_index` (0 — which face is up is part of what
/// the card *is*, and a 1 would say "this is a double-faced card") and its
/// `power_toughness` (a printed 3/3 on a morph is a tell). Its `counters` stay: counters
/// sit on top of the card in paper and everyone can count them.
pub fn card_view(card: &CardInstance, viewer: Option<SeatId>) -> Option<CardView> {
    match card.zone {
        // A library is a count to everyone, its owner included — the order is the secret.
        Zone::Library => None,
        Zone::Hand => {
            if viewer == Some(card.owner) || card.revealed {
                Some(full_card_view(card))
            } else {
                None
            }
        }
        Zone::Battlefield if card.face_down && viewer != Some(card.controller) => {
            let mut view = full_card_view(card);
            view.def = None;
            view.face_index = 0;
            view.power_toughness = None;
            Some(view)
        }
        _ => Some(full_card_view(card)),
    }
}

/// One seat as `viewer` sees it (see the module docs for `hand`).
///
/// The room is a parameter because `revealed` lives on the card instances, not on the
/// seat: for a non-owner viewer the hand lists exactly the cards this seat has revealed.
pub fn seat_snapshot(state: &RoomState, seat: &SeatState, viewer: Option<SeatId>) -> SeatSnapshot {
    let hand = if viewer == Some(seat.id) {
        seat.hand.clone()
    } else {
        seat.hand
            .iter()
            .copied()
            .filter(|id| state.cards.get(id).is_some_and(|c| c.revealed))
            .collect()
    };
    SeatSnapshot {
        id: seat.id,
        seat_index: seat.seat_index,
        name: seat.name.clone(),
        is_host: seat.is_host,
        deck_name: seat.deck_name.clone(),
        life: seat.life,
        counters: seat.counters.clone(),
        commander_damage: seat.commander_damage.clone(),
        out: seat.out,
        connected: seat.connections > 0,
        library_count: seat.library.len() as u32,
        hand_count: seat.hand.len() as u32,
        hand: Some(hand),
        battlefield: seat.battlefield.clone(),
        graveyard: seat.graveyard.clone(),
        exile: seat.exile.clone(),
        command: seat.command.clone(),
    }
}

/// Seats in table order (`seat_index`, then id as the tie-break).
fn ordered_seats(state: &RoomState) -> Vec<&SeatState> {
    let mut seats: Vec<&SeatState> = state.seats.iter().collect();
    seats.sort_by_key(|s| (s.seat_index, s.id));
    seats
}

/// The whole table for `viewer`: every seat, every visible card, the last
/// `SNAPSHOT_LOG` log entries.
pub fn snapshot_for(state: &RoomState, viewer: Option<SeatId>) -> Snapshot {
    let seats = ordered_seats(state)
        .into_iter()
        .map(|seat| seat_snapshot(state, seat, viewer))
        .collect();
    let cards = state
        .cards
        .values()
        .filter_map(|card| card_view(card, viewer))
        .collect();
    let from = state.log.len().saturating_sub(SNAPSHOT_LOG);
    Snapshot {
        version: state.version,
        status: state.status,
        format: state.format.clone(),
        starting_life: state.starting_life,
        viewer_seat: viewer,
        seats,
        cards,
        turn: state.turn,
        log: state.log[from..].to_vec(),
        winner: state.winner,
    }
}

/// The delta `changes` produced, as `viewer` sees it: every changed seat in full, every
/// changed card that is visible, every changed-but-not-visible or removed card in
/// `removed`, the log entries from `changes.log_from`.
pub fn patch_for(state: &RoomState, changes: &Changes, viewer: Option<SeatId>) -> Patch {
    let seats = ordered_seats(state)
        .into_iter()
        .filter(|seat| changes.seats.contains(&seat.id))
        .map(|seat| seat_snapshot(state, seat, viewer))
        .collect();

    let mut cards = Vec::new();
    let mut removed: Vec<CardId> = changes.removed.iter().copied().collect();
    for id in &changes.cards {
        match state.cards.get(id) {
            // A card that changed into a zone this viewer can't see reads the same as one
            // that left the table: drop it from their local map.
            Some(card) => match card_view(card, viewer) {
                Some(view) => cards.push(view),
                None => removed.push(*id),
            },
            None => removed.push(*id),
        }
    }
    removed.sort_unstable();
    removed.dedup();

    let from = changes.log_from.min(state.log.len());
    Patch {
        version: state.version,
        status: state.status,
        seats,
        cards,
        removed,
        turn: state.turn,
        log: state.log[from..].to_vec(),
        winner: state.winner,
    }
}

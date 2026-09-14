//! Everything that moves a card between zones, plus the two library reads that answer with
//! a private [`Peek`].
//!
//! One body ([`move_card_core`]) does every move: it lifts the card out of whichever seat's
//! list holds it, applies the battlefield resets, deletes a token that left the battlefield,
//! detaches whatever was strapped to it, and puts it down at the placement asked for. The
//! two public doors differ only in where the card is allowed to start — `move_card` refuses
//! a library card as unknown, `move_library_card` requires one.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};

use crate::play::engine::log::{cards_phrase, move_label, push_log, zone_label};
use crate::play::engine::table::{
    clamp01, controlled, holder_of, remove_from_zone, require_seat, seat_name, seat_pos,
    zone_vec_mut,
};
use crate::play::engine::{ActionError, Changes};
use crate::play::rng::PlayRng;
use crate::play::types::{
    CardId, LogKind, MAX_DRAW, MAX_HAND_SIZE, Peek, PeekKind, Placement, RoomState, SeatId, Zone,
};
use crate::play::view::full_card_view;

/// Draw `n` from the top of the library into the hand, clamped to what is there.
pub(super) fn draw(
    state: &mut RoomState,
    actor: SeatId,
    n: u32,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

/// Shuffle the actor's library.
pub(super) fn shuffle(
    state: &mut RoomState,
    actor: SeatId,
    rng: &mut PlayRng,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

/// Hand back into the library, shuffle, redraw `hand_size`.
pub(super) fn mulligan(
    state: &mut RoomState,
    actor: SeatId,
    hand_size: u32,
    rng: &mut PlayRng,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

/// Move a card the actor controls and can see.
#[allow(clippy::too_many_arguments)]
pub(super) fn move_card(
    state: &mut RoomState,
    actor: SeatId,
    card: CardId,
    zone: Zone,
    placement: Option<Placement>,
    x: Option<f32>,
    y: Option<f32>,
    face_down: Option<bool>,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    // A library card is not addressable here: the actor was never shown it, so it
    // reads as unknown (`move_library_card` is the peek-driven door).
    controlled(state, card, actor)?;
    move_card_core(
        state, actor, card, zone, placement, x, y, face_down, now, changes,
    );

    Ok(())
}

/// Move a card a peek showed the actor, out of their own library.
#[allow(clippy::too_many_arguments)]
pub(super) fn move_library_card(
    state: &mut RoomState,
    actor: SeatId,
    card: CardId,
    zone: Zone,
    placement: Option<Placement>,
    x: Option<f32>,
    y: Option<f32>,
    face_down: Option<bool>,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
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

    Ok(())
}

/// Put the cards a peek showed back on top in the given order (the tail of a scry).
pub(super) fn reorder_top(
    state: &mut RoomState,
    actor: SeatId,
    cards: Vec<CardId>,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

/// Look at the top `n` cards — answered privately, and logged only as a count.
pub(super) fn look_top(
    state: &mut RoomState,
    actor: SeatId,
    n: u32,
    now: DateTime<Utc>,
) -> Result<Peek, ActionError> {
    let who = seat_name(state, actor);
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
    Ok(Peek {
        cards,
        kind: PeekKind::LookTop,
    })
}

/// Read the whole library in order — answered privately.
pub(super) fn search_library(
    state: &mut RoomState,
    actor: SeatId,
    now: DateTime<Utc>,
) -> Result<Peek, ActionError> {
    let who = seat_name(state, actor);
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
    Ok(Peek {
        cards,
        kind: PeekKind::SearchLibrary,
    })
}

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

//! The actions that change a permanent rather than move it: tapping, counters, faces,
//! attachments, changing controller, and the two ways a new card appears on the table
//! (a token typed in at the table, and a copy of something already there).
//!
//! Every one of these starts with the same ownership question — [`controlled`] — and most
//! then insist on the battlefield, which is the only zone where these concepts mean
//! anything.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::play::engine::log::{battlefield_label, push_log};
use crate::play::engine::table::{
    clamp01, controlled, remove_from_zone, require_seat, seat_name, seat_pos,
};
use crate::play::engine::{ActionError, Changes, MAX_TOKEN_COUNT};
use crate::play::rng::PlayRng;
use crate::play::types::{
    CardDef, CardFace, CardId, CardInstance, LogKind, MAX_COUNTER_NAME, RoomState, SeatId, Zone,
};

/// A typed-in token's name / type line bound.
const MAX_TOKEN_TEXT: usize = 120;
/// How far each extra copy of a token is nudged so a stack of them is separable.
const TOKEN_SPREAD: f32 = 0.02;
/// The game slug an empty table falls back to when a token is made before any card exists.
const DEFAULT_GAME: &str = "mtg";

/// Drag a permanent around its controller's battlefield. Deliberately not logged.
pub(super) fn set_position(
    state: &mut RoomState,
    actor: SeatId,
    card: CardId,
    x: f32,
    y: f32,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let instance = controlled(state, card, actor)?;
    if instance.zone != Zone::Battlefield {
        return Err(ActionError::WrongZone);
    }
    if let Some(instance) = state.cards.get_mut(&card) {
        instance.x = clamp01(x);
        instance.y = clamp01(y);
    }
    changes.cards.insert(card);

    Ok(())
}

pub(super) fn tap(
    state: &mut RoomState,
    actor: SeatId,
    card: CardId,
    tapped: bool,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

/// Untap everything the actor controls.
pub(super) fn untap_all(
    state: &mut RoomState,
    actor: SeatId,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

/// Transform / flip a double-faced card; a single-faced one just turns over.
pub(super) fn toggle_face(
    state: &mut RoomState,
    actor: SeatId,
    card: CardId,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

pub(super) fn set_face_down(
    state: &mut RoomState,
    actor: SeatId,
    card: CardId,
    face_down: bool,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

/// Add `delta` to a named counter on a card; a result at or below zero removes it.
pub(super) fn counter(
    state: &mut RoomState,
    actor: SeatId,
    card: CardId,
    name: String,
    delta: i32,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

/// Show a hand card to the table (or stop showing it).
pub(super) fn reveal(
    state: &mut RoomState,
    actor: SeatId,
    card: CardId,
    revealed: bool,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

/// Attach to another battlefield card, or detach (`to: None`).
pub(super) fn attach(
    state: &mut RoomState,
    actor: SeatId,
    card: CardId,
    to: Option<CardId>,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

/// Gain control of a battlefield card another seat controls.
pub(super) fn take_control(
    state: &mut RoomState,
    actor: SeatId,
    card: CardId,
    x: f32,
    y: f32,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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

    Ok(())
}

/// Mint `count` identical tokens on the actor's battlefield.
#[allow(clippy::too_many_arguments)]
pub(super) fn create_token(
    state: &mut RoomState,
    actor: SeatId,
    name: String,
    card_id: Option<String>,
    type_line: Option<String>,
    power_toughness: Option<String>,
    colors: Vec<String>,
    x: f32,
    y: f32,
    count: u32,
    rng: &mut PlayRng,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
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
    let has_image = card_id.is_some();
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
        has_image,
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

    Ok(())
}

/// Copy a battlefield card the actor controls as a token beside it.
pub(super) fn clone_card(
    state: &mut RoomState,
    actor: SeatId,
    card: CardId,
    rng: &mut PlayRng,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
    let instance = controlled(state, card, actor)?;
    if instance.zone != Zone::Battlefield {
        return Err(ActionError::WrongZone);
    }
    let mut def = instance.def.clone();
    def.is_token = true;
    def.is_commander = false;
    let label = battlefield_label(instance);
    let (x, y, power_toughness) = (instance.x, instance.y, instance.power_toughness.clone());
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

    Ok(())
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

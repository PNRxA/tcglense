//! The actions that change a permanent rather than move it: tapping, counters, faces,
//! attachments, changing controller, and the two ways a new card appears on the table
//! (a token typed in at the table, and a copy of something already there).
//!
//! Every one of these starts with the same ownership question — [`controlled`] — and most
//! then insist on the battlefield, which is the only zone where these concepts mean
//! anything.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::play::engine::log::{hidden_label, push_log};
use crate::play::engine::table::{
    clamp01, controlled, remove_from_zone, require_seat, seat_name, seat_pos,
};
use crate::play::engine::{ActionError, Changes, MAX_TOKEN_COUNT, MAX_TOTAL, valid_delta};
use crate::play::rng::PlayRng;
use crate::play::types::{
    CardDef, CardFace, CardId, CardInstance, LogKind, MAX_COUNTER_NAME, RoomState, SeatId, Zone,
};

/// A typed-in token's name / type line bound.
const MAX_TOKEN_TEXT: usize = 120;
/// A token's catalog id, when one was picked from the search (an `external_id`, never long).
const MAX_TOKEN_CARD_ID: usize = 64;
/// A typed-in token's power/toughness (`10/10`, `*/*`).
const MAX_TOKEN_PT: usize = 16;
/// The colour letters a token may carry, at most one of each.
const COLOR_LETTERS: [&str; 5] = ["W", "U", "B", "R", "G"];
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
    let label = hidden_label(instance);
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

/// Transform / flip a double-faced card; a single-faced one just turns over. Deliberately
/// not battlefield-only — reading the back of an MDFC in hand is the common case — which is
/// why the log goes through [`hidden_label`]: a card still in a hand must not be named.
pub(super) fn toggle_face(
    state: &mut RoomState,
    actor: SeatId,
    card: CardId,
    now: DateTime<Utc>,
    changes: &mut Changes,
) -> Result<(), ActionError> {
    let who = seat_name(state, actor);
    let instance = controlled(state, card, actor)?;
    let label = hidden_label(instance);
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
    // Going face down, the label is what the table can see *now* — so an already-hidden
    // card is not named on the way down. Coming face up, the card becomes public with this
    // very action, so it is named.
    let label = if face_down {
        hidden_label(instance)
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

/// Add `delta` to a named counter on a card; a result at or below zero removes it, and the
/// total clamps at [`MAX_TOTAL`]. Battlefield only, like [`tap`]: counters on a card in a
/// hidden zone would be bookkeeping nobody can check, and the log line would have to name
/// a card the table was never shown.
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
    // Bounded before any arithmetic: `i32::MIN.abs()` panics, and an unbounded step would
    // overflow the total it is added to.
    if !valid_delta(delta) {
        return Err(ActionError::Invalid("that counter change is out of bounds"));
    }
    let instance = controlled(state, card, actor)?;
    if instance.zone != Zone::Battlefield {
        return Err(ActionError::WrongZone);
    }
    let label = hidden_label(instance);
    let mut total = 0;
    if let Some(instance) = state.cards.get_mut(&card) {
        total = instance
            .counters
            .get(&trimmed)
            .copied()
            .unwrap_or(0)
            .saturating_add(delta)
            .min(MAX_TOTAL);
        if total <= 0 {
            instance.counters.remove(&trimmed);
            total = 0;
        } else {
            instance.counters.insert(trimmed.clone(), total);
        }
    }
    changes.cards.insert(card);
    let verb = if delta > 0 { "added" } else { "removed" };
    let step = delta.unsigned_abs();
    push_log(
        state,
        now,
        LogKind::Action,
        Some(actor),
        format!(
            "{who} {verb} {step} {trimmed} counter{} on {label} ({total})",
            if step == 1 { "" } else { "s" }
        ),
    );

    Ok(())
}

/// Show a hand card to the table (or stop showing it). The one action that names a hand
/// card in the log on purpose — showing it *is* the action — and only in that direction:
/// stopping says "a card", because the card goes back to being hidden.
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
    // Not `hidden_label`: this action is what makes the card public.
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
    let label = hidden_label(instance);
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
            Some(hidden_label(other))
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
    let label = hidden_label(instance);
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
///
/// Every field is free text a client typed, so every field is bounded here: the name
/// (1..=[`MAX_TOKEN_TEXT`] after trim), the catalog id it claims to be
/// ([`MAX_TOKEN_CARD_ID`]), the type line ([`MAX_TOKEN_TEXT`]), the power/toughness
/// ([`MAX_TOKEN_PT`]) and the colours (letters from [`COLOR_LETTERS`], deduplicated, so at
/// most five). A token lives in `state.cards` and rides every snapshot to every viewer —
/// an unbounded one would be a cheap way to make the table unusable for everyone.
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
    if let Some(id) = &card_id
        && id.chars().count() > MAX_TOKEN_CARD_ID
    {
        return Err(ActionError::Invalid("that card id is too long"));
    }
    if let Some(pt) = &power_toughness
        && pt.chars().count() > MAX_TOKEN_PT
    {
        return Err(ActionError::Invalid("that power/toughness is too long"));
    }
    let colors = dedup_colors(colors)?;
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
    let label = hidden_label(instance);
    // A copy of a face-down permanent is itself face down: making the copy must not be a
    // way to show the table what the original is (and the log says "a face-down card").
    let (face_down, face_index) = (instance.face_down, instance.face_index);
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
        face_down,
        face_index,
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

/// The colour letters a token may carry: each one of [`COLOR_LETTERS`], at most one of
/// each, in the order given. Anything else is [`ActionError::Invalid`] rather than silently
/// dropped — a colour the table can't render is a client bug worth telling it about.
fn dedup_colors(colors: Vec<String>) -> Result<Vec<String>, ActionError> {
    let mut out: Vec<String> = Vec::with_capacity(COLOR_LETTERS.len());
    for color in colors {
        let letter = color.trim().to_uppercase();
        if !COLOR_LETTERS.contains(&letter.as_str()) {
            return Err(ActionError::Invalid("that isn't a colour"));
        }
        if !out.contains(&letter) {
            out.push(letter);
        }
    }
    Ok(out)
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

//! How the table talks about itself: the phrasing helpers every action shares, and the
//! two bookkeeping functions that append to [`RoomState::log`] and keep it capped.
//!
//! The log is public — every seat and every spectator reads the same lines — so the two
//! naming helpers here are the seam that keeps hidden information hidden. A card the table
//! was never shown is "a card"; a face-down permanent is "a face-down card".
//!
//! **One hidden-label helper.** Every log line that names a card goes through
//! [`hidden_label`] (the card as it stands) or [`move_label`] (the card as it crosses
//! zones, which decides on the source and destination instead). No arm may format
//! `card.def.name` into a log line itself: the one exception is `cards::reveal`, where
//! naming the card *is* the action, and it names it only in the `revealed: true`
//! branch. A new action that can name a card is a new caller of these two, never a new
//! rule — the view (`view::card_view`) hides exactly what they hide, and the two must stay
//! in step.

use chrono::{DateTime, Utc};

use crate::play::types::{CardInstance, LogEntry, LogKind, MAX_LOG, RoomState, SeatId, Zone};

/// How the log names a card *where it sits*: anything the table has not been shown is
/// unnamed. A face-down permanent is "a face-down card" (everyone can see something is
/// there); a card in a hand that its seat has not `revealed`, and any library card, are
/// "a card". Everything else — the battlefield face up, a graveyard, exile, the command
/// zone — is public, and named.
pub(super) fn hidden_label(card: &CardInstance) -> String {
    match card.zone {
        Zone::Battlefield if card.face_down => "a face-down card".to_string(),
        Zone::Hand if !card.revealed => "a card".to_string(),
        Zone::Library => "a card".to_string(),
        _ => card.def.name.clone(),
    }
}

pub(super) fn zone_label(zone: Zone) -> &'static str {
    match zone {
        Zone::Command => "command zone",
        other => other.as_str(),
    }
}

pub(super) fn cards_phrase(n: usize) -> String {
    if n == 1 {
        "1 card".to_string()
    } else {
        format!("{n} cards")
    }
}

/// Append one entry, minting its room-monotonic id.
pub(super) fn push_log(
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
pub(super) fn settle_log(state: &mut RoomState, len_before: usize) -> usize {
    let added = state.log.len().saturating_sub(len_before);
    if state.log.len() > MAX_LOG {
        let excess = state.log.len() - MAX_LOG;
        state.log.drain(0..excess);
    }
    state.log.len().saturating_sub(added)
}

/// How the log names a card that moved, without leaking what nobody was shown — the
/// crossing-zones half of [`hidden_label`]: a card whose source *and* destination are both
/// hidden (or that is going face down) was never shown to the table and stays "a card",
/// while one landing somewhere public is named, because everyone is about to see it.
pub(super) fn move_label(
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

//! How the table talks about itself: the phrasing helpers every action shares, and the
//! two bookkeeping functions that append to [`RoomState::log`] and keep it capped.
//!
//! The log is public — every seat and every spectator reads the same lines — so the two
//! naming helpers here are the seam that keeps hidden information hidden. A card the table
//! was never shown is "a card"; a face-down permanent is "a face-down card".

use chrono::{DateTime, Utc};

use crate::play::types::{CardInstance, LogEntry, LogKind, MAX_LOG, RoomState, SeatId, Zone};

/// A battlefield card the table can't see is never named in the log.
pub(super) fn battlefield_label(card: &CardInstance) -> String {
    if card.zone == Zone::Battlefield && card.face_down {
        "a face-down card".to_string()
    } else {
        card.def.name.clone()
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

/// How the log names a card that moved, without leaking what nobody was shown.
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

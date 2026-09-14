//! What one viewer may see. The only place hidden information is filtered — the engine and
//! the persisted state hold everything in the clear, and the socket layer never sends a
//! `RoomState`, only what these functions return.
//!
//! Visibility rules:
//! - **Library**: never visible to anyone (a count). A `look_top` / `search_library` answer
//!   is a one-off [`Peek`], not a change in visibility.
//! - **Hand**: the owner sees ids + cards; everyone else sees a count, plus any card the
//!   owner `revealed` (which then appears in `SeatSnapshot::hand` for them too).
//! - **Battlefield / graveyard / exile / command**: public — except a `face_down`
//!   battlefield card, which its controller sees whole and everyone else sees as an id with
//!   `def: None`.
//! - A spectator (`viewer == None`) sees what "everyone else" sees.

use crate::play::engine::Changes;
use crate::play::types::{
    CardInstance, CardView, Patch, RoomState, SeatId, SeatSnapshot, SeatState, Snapshot,
};

/// The card as `viewer` sees it; `None` when the viewer may not know it is there at all
/// (in a library, or in someone else's hand and not revealed).
pub fn card_view(card: &CardInstance, viewer: Option<SeatId>) -> Option<CardView> {
    let _ = (card, viewer);
    todo!("engine implementer")
}

/// One seat as `viewer` sees it (see the module docs for `hand`).
pub fn seat_snapshot(seat: &SeatState, viewer: Option<SeatId>) -> SeatSnapshot {
    let _ = (seat, viewer);
    todo!("engine implementer")
}

/// The whole table for `viewer`: every seat, every visible card, the last
/// `SNAPSHOT_LOG` log entries.
pub fn snapshot_for(state: &RoomState, viewer: Option<SeatId>) -> Snapshot {
    let _ = (state, viewer);
    todo!("engine implementer")
}

/// The delta `changes` produced, as `viewer` sees it: every changed seat in full, every
/// changed card that is visible, every changed-but-not-visible or removed card in
/// `removed`, the log entries from `changes.log_from`.
pub fn patch_for(state: &RoomState, changes: &Changes, viewer: Option<SeatId>) -> Patch {
    let _ = (state, changes, viewer);
    todo!("engine implementer")
}

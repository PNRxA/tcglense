//! The pure life-history fold.
//!
//! A seat's current total is the result of replaying its [`life_events`] from its starting
//! life. Normally that fold is incremental — a tap appends one event and moves the total by
//! its delta — but an **undo** removes an event from anywhere in the chain, which makes every
//! later `life_after` (and the seat's current total) stale. Rather than trust arithmetic on
//! rows that just changed under it, the undo re-folds the remaining chain through here.
//!
//! The fold has to honour the two event kinds differently, which is the whole reason this is
//! worth isolating and testing:
//!
//! - an `adjust` is **relative** — it re-applies its own `delta` to whatever the total is now,
//!   so removing an earlier event shifts every later total by the same amount;
//! - a `set` is **absolute** — it pins the total to the number the user typed, so an earlier
//!   removal changes nothing at or after it. Its `delta` is derived, not stored input, and is
//!   recomputed against the new preceding total.
//!
//! Clamping happens here too, so a chain that would walk past the counter's bounds folds to the
//! bound and the recorded delta describes the movement that actually happened.
//!
//! Since #595 a seat carries more than one number, so the unit of the fold is a **chain**: one
//! per `(counter, source)` its events name. Chains are independent — poison doesn't move life,
//! and damage from one commander doesn't move damage from another — so [`replay_seat`] splits
//! the seat's rows into chains, folds each from its own starting value and within its own
//! bounds, and stitches the corrected rows back into input order. Only the `life` chain feeds
//! the seat's denormalised total.
//!
//! No I/O: the caller loads the rows, folds them here, and writes back only what changed.
//!
//! [`life_events`]: crate::entities::life_event

use std::collections::HashMap;

use crate::entities::{life_event, life_session_player};

use super::counters::{COUNTER_LIFE, COUNTERS, bounds, start_value};
use super::{KIND_SET, LifeCounterResponse};

/// One event as the fold sees it: which chain it belongs to, what kind of change it was, the
/// relative delta it carried, and (for a `set`) the absolute value it pinned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReplayEvent {
    pub counter: String,
    pub source_player_id: Option<i32>,
    pub is_set: bool,
    pub delta: i32,
    pub life_after: i32,
}

impl ReplayEvent {
    /// Read one stored event row into the fold's view of it.
    pub(super) fn from_row(
        kind: &str,
        counter: &str,
        source_player_id: Option<i32>,
        delta: i32,
        life_after: i32,
    ) -> Self {
        Self {
            counter: counter.to_string(),
            source_player_id,
            is_set: kind == KIND_SET,
            delta,
            life_after,
        }
    }
}

/// The chain an event folds within: one counter, and — for commander damage — one source seat.
pub(super) type ChainKey = (String, Option<i32>);

/// The corrected chain: one `(delta, life_after)` pair per input event, in the same order, the
/// value every chain ended on, and the seat's resulting life total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Replayed {
    pub events: Vec<(i32, i32)>,
    /// Where each `(counter, source)` chain finished — the seat's counter state, derived rather
    /// than stored.
    pub totals: HashMap<ChainKey, i32>,
    pub life: i32,
}

/// Clamp a value into `counter`'s storable range.
pub(super) fn clamp_value(counter: &str, value: i64) -> i32 {
    let (min, max) = bounds(counter);
    value.clamp(i64::from(min), i64::from(max)) as i32
}

/// Re-fold a **whole seat**: split its events into one chain per `(counter, source)`, fold each
/// from its own starting value, and return the corrected `(delta, life_after)` for every event
/// in input order, where every chain ended, and the seat's resulting life.
///
/// Only the `life` chain feeds the seat's denormalised total — a poison tap must never move a
/// life total as a side effect, which is what lets "life is written in exactly two places"
/// survive the second axis.
pub(super) fn replay_seat(starting_life: i32, events: &[ReplayEvent]) -> Replayed {
    let mut totals: HashMap<ChainKey, i32> = HashMap::new();
    let mut out = Vec::with_capacity(events.len());
    for event in events {
        let chain: ChainKey = (event.counter.clone(), event.source_player_id);
        let counter = event.counter.as_str();
        let before = *totals
            .entry(chain.clone())
            .or_insert_with(|| start_value(counter, starting_life));
        let after = if event.is_set {
            // Absolute: the value the user typed, clamped. Earlier edits don't move it.
            clamp_value(counter, i64::from(event.life_after))
        } else {
            // Relative: re-apply the tap. Widen first so a huge chain can't overflow before
            // the clamp sees it.
            clamp_value(counter, i64::from(before) + i64::from(event.delta))
        };
        totals.insert(chain, after);
        // The delta is what the change *actually* did after clamping, so a history row never
        // claims a movement the total didn't make.
        out.push((after - before, after));
    }
    // A seat whose life never moved is still on the life it was seated with, not on zero.
    let life = totals
        .get(&(COUNTER_LIFE.to_string(), None))
        .copied()
        .unwrap_or(starting_life);
    Replayed {
        events: out,
        totals,
        life,
    }
}

/// Read a stored event row into the fold's view of it.
pub(super) fn replay_event(row: &life_event::Model) -> ReplayEvent {
    ReplayEvent::from_row(
        &row.kind,
        &row.counter,
        row.source_player_id,
        row.delta,
        row.life_after,
    )
}

/// Fold a whole game's history into the current value of every non-life counter.
///
/// This is the read-side twin of the undo's re-fold: the same [`replay_seat`] over the same
/// rows, so "what does the client show" and "what does an undo write back" can't disagree. Life
/// is excluded — it lives on the seat row, and re-deriving it here would be the second writer
/// the module's invariant exists to prevent.
///
/// `events` must be the session's rows in `id` order; `seats` fixes the output order (seat
/// order, then counter vocabulary order, then the source seat's own order) so the response is
/// deterministic rather than however the fold's map happened to hash.
pub(super) fn counter_values(
    seats: &[life_session_player::Model],
    events: &[life_event::Model],
) -> Vec<LifeCounterResponse> {
    // A seat's own order, for sorting the commander-damage sources into table order. A source
    // that has left the table isn't here, and sorts last — orphan-tolerant, like every other
    // seat reference in this module.
    let seat_order: HashMap<i32, usize> = seats
        .iter()
        .enumerate()
        .map(|(index, seat)| (seat.id, index))
        .collect();

    let mut by_player: HashMap<i32, Vec<ReplayEvent>> = HashMap::new();
    for row in events {
        by_player
            .entry(row.player_id)
            .or_default()
            .push(replay_event(row));
    }

    let mut out = Vec::new();
    for seat in seats {
        let Some(rows) = by_player.get(&seat.id) else {
            continue;
        };
        let folded = replay_seat(seat.starting_life, rows);
        let mut chains: Vec<(ChainKey, i32)> = folded
            .totals
            .into_iter()
            .filter(|((counter, _), _)| counter != COUNTER_LIFE)
            .collect();
        chains.sort_by_key(|((counter, source), _)| {
            (
                COUNTERS.iter().position(|known| known == counter),
                source.map(|id| (seat_order.get(&id).copied().unwrap_or(usize::MAX), id)),
            )
        });
        out.extend(
            chains
                .into_iter()
                .map(|((counter, source_player_id), value)| LifeCounterResponse {
                    player_id: seat.id,
                    counter,
                    source_player_id,
                    value,
                }),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::counters::{COUNTER_COMMANDER_DAMAGE, COUNTER_MAX, COUNTER_POISON};
    use super::super::{LIFE_MAX, LIFE_MIN};
    use super::*;

    fn adjust(delta: i32) -> ReplayEvent {
        counter_event(COUNTER_LIFE, None, false, delta, 0)
    }

    fn set(life: i32) -> ReplayEvent {
        counter_event(COUNTER_LIFE, None, true, 0, life)
    }

    fn counter_event(
        counter: &str,
        source: Option<i32>,
        is_set: bool,
        delta: i32,
        life_after: i32,
    ) -> ReplayEvent {
        ReplayEvent {
            counter: counter.to_string(),
            source_player_id: source,
            is_set,
            delta,
            life_after,
        }
    }

    /// The whole-seat fold, keeping the single-counter tests reading as they always did.
    fn replay(starting_life: i32, events: &[ReplayEvent]) -> Replayed {
        replay_seat(starting_life, events)
    }

    /// Where one chain finished.
    fn total(replayed: &Replayed, counter: &str, source: Option<i32>) -> Option<i32> {
        replayed.totals.get(&(counter.to_string(), source)).copied()
    }

    #[test]
    fn empty_chain_leaves_the_seat_on_its_starting_life() {
        let got = replay(40, &[]);
        assert_eq!(got.events, Vec::<(i32, i32)>::new());
        assert_eq!(got.life, 40);
        assert!(got.totals.is_empty());
    }

    #[test]
    fn adjusts_accumulate_and_report_running_totals() {
        let got = replay(20, &[adjust(-3), adjust(-5), adjust(2)]);
        assert_eq!(got.life, 14);
        assert_eq!(got.events, vec![(-3, 17), (-5, 12), (2, 14)]);
    }

    #[test]
    fn a_set_pins_the_total_and_derives_its_own_delta() {
        // The absolute correction ignores where the chain was; its delta is the distance it
        // moved from the preceding total (-4 from 17 to 13), and the tap after it is relative
        // to the pinned value.
        let got = replay(20, &[adjust(-3), set(13), adjust(-1)]);
        assert_eq!(got.life, 12);
        assert_eq!(got.events, vec![(-3, 17), (-4, 13), (-1, 12)]);
    }

    #[test]
    fn removing_an_earlier_adjust_shifts_later_adjusts_but_not_a_later_set() {
        // The full chain: -3, then an absolute 13, then -1.
        let full = replay(20, &[adjust(-3), set(13), adjust(-1)]);
        assert_eq!(full.life, 12);
        // Undo the leading -3 by re-folding without it. The `set` still pins 13, so the
        // outcome is unchanged — only the set's own derived delta moves (-7 from 20).
        let undone = replay(20, &[set(13), adjust(-1)]);
        assert_eq!(undone.life, 12);
        assert_eq!(undone.events, vec![(-7, 13), (-1, 12)]);
        // Whereas with no `set` in the chain, dropping the -3 shifts every later total.
        assert_eq!(replay(20, &[adjust(-3), adjust(-5)]).life, 12);
        assert_eq!(replay(20, &[adjust(-5)]).life, 15);
    }

    #[test]
    fn totals_clamp_at_the_bounds_and_the_recorded_delta_matches_the_real_movement() {
        // A tap that would push past the floor stops at it, and the delta reports the 21
        // points it actually lost rather than the 100 it asked for.
        let got = replay(LIFE_MIN + 21, &[adjust(-100)]);
        assert_eq!(got.life, LIFE_MIN);
        assert_eq!(got.events, vec![(-21, LIFE_MIN)]);
        // A no-op tap at the bound records a zero delta, not a phantom loss.
        let pinned = replay(LIFE_MIN, &[adjust(-5)]);
        assert_eq!(pinned.events, vec![(0, LIFE_MIN)]);
        // Same at the ceiling, and via an absolute set that overshoots.
        assert_eq!(replay(LIFE_MAX - 1, &[adjust(50)]).life, LIFE_MAX);
        assert_eq!(replay(20, &[set(LIFE_MAX + 1000)]).life, LIFE_MAX);
    }

    #[test]
    fn a_long_chain_of_extreme_adjusts_cannot_overflow() {
        // 5,000 events (the per-session cap) each at the delta cap: the widened fold clamps
        // instead of wrapping, which an i32 sum would not.
        let events: Vec<ReplayEvent> = (0..5_000).map(|_| adjust(1_000)).collect();
        assert_eq!(replay(0, &events).life, LIFE_MAX);
    }

    #[test]
    fn each_counter_folds_in_its_own_chain_from_its_own_start() {
        // Life starts at 40, poison at 0 — interleaved, and neither moves the other.
        let got = replay_seat(
            40,
            &[
                adjust(-3),
                counter_event(COUNTER_POISON, None, false, 2, 0),
                adjust(-5),
                counter_event(COUNTER_POISON, None, false, 3, 0),
            ],
        );
        assert_eq!(got.life, 32, "poison never touches the life total");
        assert_eq!(got.events, vec![(-3, 37), (2, 2), (-5, 32), (3, 5)]);
        assert_eq!(total(&got, COUNTER_POISON, None), Some(5));
    }

    #[test]
    fn commander_damage_is_one_chain_per_source_seat() {
        // Two commanders hitting the same player: 7 from seat 2 and 6 from seat 3 is not 13
        // from either, which is exactly what decides whether the 21 threshold was reached.
        let got = replay_seat(
            40,
            &[
                counter_event(COUNTER_COMMANDER_DAMAGE, Some(2), false, 7, 0),
                counter_event(COUNTER_COMMANDER_DAMAGE, Some(3), false, 6, 0),
                counter_event(COUNTER_COMMANDER_DAMAGE, Some(2), false, 7, 0),
            ],
        );
        assert_eq!(got.events, vec![(7, 7), (6, 6), (7, 14)]);
        assert_eq!(total(&got, COUNTER_COMMANDER_DAMAGE, Some(2)), Some(14));
        assert_eq!(total(&got, COUNTER_COMMANDER_DAMAGE, Some(3)), Some(6));
        assert_eq!(got.life, 40, "damage counters don't move life themselves");
    }

    #[test]
    fn a_non_life_counter_floors_at_zero_rather_than_going_negative() {
        // Removing more poison than a player has leaves them on none, and the row records the
        // movement that happened — the life floor's contract, at the counter's own bound.
        let got = replay_seat(
            20,
            &[
                counter_event(COUNTER_POISON, None, false, 3, 0),
                counter_event(COUNTER_POISON, None, false, -10, 0),
            ],
        );
        assert_eq!(got.events, vec![(3, 3), (-3, 0)]);
        assert_eq!(
            replay_seat(
                20,
                &[counter_event(
                    COUNTER_POISON,
                    None,
                    true,
                    0,
                    COUNTER_MAX + 5
                )]
            )
            .events,
            vec![(COUNTER_MAX, COUNTER_MAX)],
            "an absolute correction clamps to the counter's ceiling, not life's"
        );
    }

    #[test]
    fn undoing_a_counter_event_leaves_the_other_chains_alone() {
        let full = [
            adjust(-3),
            counter_event(COUNTER_COMMANDER_DAMAGE, Some(2), false, 7, 0),
            counter_event(COUNTER_POISON, None, false, 4, 0),
            counter_event(COUNTER_COMMANDER_DAMAGE, Some(2), false, 7, 0),
        ];
        assert_eq!(replay_seat(40, &full).events.len(), 4);
        // Drop the first commander-damage row: the later one in that chain re-folds from 0,
        // while the life and poison rows are untouched.
        let undone = replay_seat(40, &[full[0].clone(), full[2].clone(), full[3].clone()]);
        assert_eq!(undone.events, vec![(-3, 37), (4, 4), (7, 7)]);
        assert_eq!(undone.life, 37);
    }

    #[test]
    fn a_seat_with_no_life_events_at_all_keeps_its_starting_life() {
        // Only counters moved, so the seat's denormalised total must stay exactly where the
        // seat was seated — not fall back to zero.
        let got = replay_seat(40, &[counter_event(COUNTER_POISON, None, false, 5, 0)]);
        assert_eq!(got.life, 40);
    }
}

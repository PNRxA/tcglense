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
//! Clamping happens here too, so a chain that would walk past [`LIFE_MIN`]/[`LIFE_MAX`] folds
//! to the bound and the recorded delta describes the movement that actually happened.
//!
//! No I/O: the caller loads the rows, folds them here, and writes back only what changed.
//!
//! [`life_events`]: crate::entities::life_event
//! [`LIFE_MIN`]: super::LIFE_MIN
//! [`LIFE_MAX`]: super::LIFE_MAX

use super::{KIND_SET, LIFE_MAX, LIFE_MIN};

/// One event as the fold sees it: what kind of change it was, the relative delta it carried,
/// and (for a `set`) the absolute total it pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReplayEvent {
    pub is_set: bool,
    pub delta: i32,
    pub life_after: i32,
}

impl ReplayEvent {
    /// Read one stored event row into the fold's view of it.
    pub(super) fn from_row(kind: &str, delta: i32, life_after: i32) -> Self {
        Self {
            is_set: kind == KIND_SET,
            delta,
            life_after,
        }
    }
}

/// The corrected chain: one `(delta, life_after)` pair per input event, in the same order,
/// plus the seat's resulting current total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Replayed {
    pub events: Vec<(i32, i32)>,
    pub life: i32,
}

/// Clamp a life total into the storable range.
pub(super) fn clamp_life(life: i64) -> i32 {
    life.clamp(LIFE_MIN as i64, LIFE_MAX as i64) as i32
}

/// Re-fold `events` from `starting_life`, returning each event's corrected `(delta,
/// life_after)` and the seat's resulting total. An empty chain leaves the seat on its
/// starting life.
pub(super) fn replay(starting_life: i32, events: &[ReplayEvent]) -> Replayed {
    let mut life = starting_life;
    let mut out = Vec::with_capacity(events.len());
    for event in events {
        let before = life;
        life = if event.is_set {
            // Absolute: the total the user typed, clamped. Earlier edits don't move it.
            clamp_life(i64::from(event.life_after))
        } else {
            // Relative: re-apply the tap. Widen first so a huge chain can't overflow before
            // the clamp sees it.
            clamp_life(i64::from(before) + i64::from(event.delta))
        };
        // The delta is what the change *actually* did after clamping, so a history row never
        // claims a movement the total didn't make.
        out.push((life - before, life));
    }
    Replayed { events: out, life }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adjust(delta: i32) -> ReplayEvent {
        ReplayEvent {
            is_set: false,
            delta,
            life_after: 0,
        }
    }

    fn set(life: i32) -> ReplayEvent {
        ReplayEvent {
            is_set: true,
            delta: 0,
            life_after: life,
        }
    }

    #[test]
    fn empty_chain_leaves_the_seat_on_its_starting_life() {
        assert_eq!(
            replay(40, &[]),
            Replayed {
                events: vec![],
                life: 40
            }
        );
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
}

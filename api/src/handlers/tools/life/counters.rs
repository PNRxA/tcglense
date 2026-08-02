//! The counter vocabulary: which numbers a seat can carry besides life, what each one is
//! allowed to hold, and which of them a given game tracks.
//!
//! A tracked game used to have exactly one number per seat. Commander damage is one of the two
//! ways a Commander game actually ends, so recording only life meant a pod that died to a
//! commander was logged as if it died to life loss (issue #595).
//!
//! Two shapes, not five:
//!
//! - **`life`** is the original, and stays the odd one out: it is the only counter denormalised
//!   onto the seat row, it starts at the seat's `starting_life`, and it may go negative.
//! - Every other counter is folded out of [`life_events`] instead of getting a column of its
//!   own. They all start at **0** and floor at 0 — you can't have negative poison — and
//!   `commander_damage` is the same fold keyed additionally by the *source* seat, which is the
//!   whole reason the counter axis carries a second column rather than five booleans.
//!
//! Which counters a game shows is a **session** setting ([`life_sessions.counters`]), not a
//! global one: a Standard pod has no business seeing a commander-damage matrix. It is stored as
//! a CSV of slugs, `life` implicit and never listed.
//!
//! [`life_events`]: crate::entities::life_event
//! [`life_sessions.counters`]: crate::entities::life_session::Model::counters

use crate::error::AppError;

use super::{LIFE_MAX, LIFE_MIN};

/// The seat's life total — the original counter, and the only one stored on the seat row.
pub(crate) const COUNTER_LIFE: &str = "life";
/// Poison counters. Ten is lethal in every format that has them.
pub(crate) const COUNTER_POISON: &str = "poison";
/// Energy counters.
pub(crate) const COUNTER_ENERGY: &str = "energy";
/// Experience counters.
pub(crate) const COUNTER_EXPERIENCE: &str = "experience";
/// Damage from one seat's commander to another. The only counter with a *source*.
pub(crate) const COUNTER_COMMANDER_DAMAGE: &str = "commander_damage";

/// Every counter an event may name, `life` included.
pub(crate) const COUNTERS: &[&str] = &[
    COUNTER_LIFE,
    COUNTER_POISON,
    COUNTER_ENERGY,
    COUNTER_EXPERIENCE,
    COUNTER_COMMANDER_DAMAGE,
];

/// The counters a session can switch on — everything but `life`, which is always tracked.
/// Order is display order: this is the list the SPA renders, and the order a stored CSV is
/// normalised into.
pub(crate) const OPTIONAL_COUNTERS: &[&str] = &[
    COUNTER_COMMANDER_DAMAGE,
    COUNTER_POISON,
    COUNTER_ENERGY,
    COUNTER_EXPERIENCE,
];

/// The ceiling for a non-life counter. Far above any real game (ten poison is lethal, 21
/// commander damage is lethal) but bounded, so a stuck client can't store a number that stops
/// rendering — the same job [`LIFE_MAX`] does for life.
pub(crate) const COUNTER_MAX: i32 = 999;

/// The formats whose games open with the commander-damage matrix on. Matched case-insensitively
/// against the session's free-form `format` label, and mirrored by `defaultCountersFor` in
/// `web/src/lib/lifeCounters.ts`.
pub(crate) const COMMANDER_FORMATS: &[&str] =
    &["commander", "edh", "brawl", "oathbreaker", "duel commander"];

/// Whether `counter` is keyed by a source seat as well as a target one.
pub(crate) fn has_source(counter: &str) -> bool {
    counter == COUNTER_COMMANDER_DAMAGE
}

/// The value `counter` starts at for a seat whose life starts at `starting_life`.
///
/// Life starts wherever the seat was seated; everything else starts at nothing, which is why
/// only life needs the seat's own number here.
pub(crate) fn start_value(counter: &str, starting_life: i32) -> i32 {
    if counter == COUNTER_LIFE {
        starting_life
    } else {
        0
    }
}

/// The storable range for `counter`, which the replay fold clamps into.
///
/// Life may go below zero (you are dead, but the number is still the number); no other counter
/// can — "minus two poison" is not a state, so a tap below the floor records a `0` movement the
/// same way a tap at the life floor does.
pub(crate) fn bounds(counter: &str) -> (i32, i32) {
    if counter == COUNTER_LIFE {
        (LIFE_MIN, LIFE_MAX)
    } else {
        (0, COUNTER_MAX)
    }
}

/// Validate a counter slug against [`COUNTERS`], defaulting an absent one to `life` — so every
/// pre-#595 client keeps working unchanged.
pub(crate) fn validate_counter(counter: Option<&str>) -> Result<&'static str, AppError> {
    let requested = counter.map(str::trim).unwrap_or(COUNTER_LIFE);
    COUNTERS
        .iter()
        .find(|known| **known == requested)
        .copied()
        .ok_or_else(|| {
            AppError::Validation(format!("counter must be one of {}", COUNTERS.join(", ")))
        })
}

/// Validate an absolute value for `counter`.
pub(crate) fn validate_value(counter: &str, value: i32) -> Result<i32, AppError> {
    let (min, max) = bounds(counter);
    if (min..=max).contains(&value) {
        return Ok(value);
    }
    Err(AppError::Validation(format!(
        "{counter} must be between {min} and {max}"
    )))
}

/// Read a stored CSV of counter slugs into the set the session tracks.
///
/// Deliberately lenient about what it *reads*: an unknown slug (a column written by a newer
/// build, then rolled back) is dropped rather than failing the read, because a session that
/// can't be loaded is worse than one missing a counter. Writes go through
/// [`validate_counters`], which is strict.
pub(crate) fn parse_counters(stored: &str) -> Vec<String> {
    normalise(stored.split(',').map(str::trim))
}

/// Validate a requested set of enabled counters, normalising it to display order with
/// duplicates removed.
///
/// `life` is rejected rather than silently dropped: it is always tracked, so accepting it in
/// the list would imply it could also be left out.
pub(crate) fn validate_counters(requested: &[String]) -> Result<Vec<String>, AppError> {
    for slug in requested {
        let slug = slug.trim();
        if !OPTIONAL_COUNTERS.contains(&slug) {
            return Err(AppError::Validation(format!(
                "counters must each be one of {} (life is always tracked)",
                OPTIONAL_COUNTERS.join(", ")
            )));
        }
    }
    Ok(normalise(requested.iter().map(|s| s.trim())))
}

/// Keep only known optional slugs, in [`OPTIONAL_COUNTERS`] order, each at most once — so a
/// stored value round-trips to the same list whatever order it was written in.
fn normalise<'a, I: Iterator<Item = &'a str>>(slugs: I) -> Vec<String> {
    let requested: Vec<&str> = slugs.collect();
    OPTIONAL_COUNTERS
        .iter()
        .filter(|known| requested.contains(*known))
        .map(|known| (*known).to_string())
        .collect()
}

/// Serialise an enabled set back to the stored CSV.
pub(crate) fn join_counters(counters: &[String]) -> String {
    counters.join(",")
}

/// The counters a new game opens with, from its format label.
///
/// A Commander pod was tracking commander damage in someone's head whether or not we stored it,
/// so it starts switched on there; every other format starts with none, because a counter nobody
/// uses is noise on the mat and it is one tap to turn on. Mirrored by `defaultCountersFor` in
/// `web/src/lib/lifeCounters.ts`.
pub(crate) fn default_counters_for(format: Option<&str>) -> Vec<String> {
    let Some(format) = format else {
        return Vec::new();
    };
    if COMMANDER_FORMATS.contains(&format.trim().to_lowercase().as_str()) {
        vec![COUNTER_COMMANDER_DAMAGE.to_string()]
    } else {
        Vec::new()
    }
}

/// Refuse a write to a counter this game doesn't track.
///
/// The enabled set is what the client renders, so a value recorded against a counter the mat
/// doesn't show would be invisible state — and, for commander damage, invisible state that
/// decides who won. `life` is always allowed.
pub(crate) fn require_enabled(enabled: &[String], counter: &str) -> Result<(), AppError> {
    if counter == COUNTER_LIFE || enabled.iter().any(|slug| slug == counter) {
        return Ok(());
    }
    Err(AppError::Validation(format!(
        "this game isn't tracking {counter} — turn it on for the game first"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn life_is_the_only_counter_that_starts_anywhere_but_zero_or_goes_negative() {
        assert_eq!(start_value(COUNTER_LIFE, 40), 40);
        assert_eq!(bounds(COUNTER_LIFE), (LIFE_MIN, LIFE_MAX));
        for counter in OPTIONAL_COUNTERS {
            assert_eq!(start_value(counter, 40), 0, "{counter}");
            assert_eq!(bounds(counter), (0, COUNTER_MAX), "{counter}");
        }
    }

    #[test]
    fn the_vocabulary_matches_the_client_which_renders_against_its_own_copy() {
        // Mirrored by LIFE_COUNTERS in `web/src/lib/lifeCounters.ts`, order included: this list
        // is what a session's `counters` is validated against *and* the order a stored CSV
        // normalises into, so a slug added on one side only would either be rejected by the API
        // or render in a different place from where it is stored.
        assert_eq!(
            OPTIONAL_COUNTERS,
            &["commander_damage", "poison", "energy", "experience"]
        );
        // Plus life, which is always tracked and so never appears in the optional list.
        assert_eq!(COUNTERS.len(), OPTIONAL_COUNTERS.len() + 1);
        assert!(COUNTERS.contains(&COUNTER_LIFE));
        assert!(!OPTIONAL_COUNTERS.contains(&COUNTER_LIFE));
        // The formats the client's `defaultCountersFor` mirrors.
        assert_eq!(
            COMMANDER_FORMATS,
            &["commander", "edh", "brawl", "oathbreaker", "duel commander"]
        );
    }

    #[test]
    fn only_commander_damage_carries_a_source() {
        assert!(has_source(COUNTER_COMMANDER_DAMAGE));
        for counter in [
            COUNTER_LIFE,
            COUNTER_POISON,
            COUNTER_ENERGY,
            COUNTER_EXPERIENCE,
        ] {
            assert!(!has_source(counter), "{counter}");
        }
    }

    #[test]
    fn an_absent_counter_is_life_so_a_pre_595_client_keeps_working() {
        assert_eq!(validate_counter(None).unwrap(), COUNTER_LIFE);
        assert_eq!(validate_counter(Some(" poison ")).unwrap(), COUNTER_POISON);
        assert!(validate_counter(Some("stun")).is_err());
    }

    #[test]
    fn values_clamp_to_the_counters_own_range() {
        assert!(validate_value(COUNTER_LIFE, -20).is_ok());
        assert!(validate_value(COUNTER_LIFE, LIFE_MAX + 1).is_err());
        // Negative poison is not a state a game can be in.
        assert!(validate_value(COUNTER_POISON, -1).is_err());
        assert!(validate_value(COUNTER_POISON, 0).is_ok());
        assert!(validate_value(COUNTER_POISON, COUNTER_MAX).is_ok());
        assert!(validate_value(COUNTER_POISON, COUNTER_MAX + 1).is_err());
    }

    #[test]
    fn the_enabled_set_normalises_to_display_order_and_round_trips() {
        let requested = vec!["poison".to_string(), "commander_damage".to_string()];
        let validated = validate_counters(&requested).unwrap();
        assert_eq!(validated, vec!["commander_damage", "poison"]);
        assert_eq!(
            parse_counters(&join_counters(&validated)),
            validated,
            "a stored CSV reads back as what was written"
        );
        // Duplicates collapse rather than doubling the list the SPA renders.
        assert_eq!(
            validate_counters(&["poison".to_string(), "poison".to_string()]).unwrap(),
            vec!["poison"]
        );
        // Life is always tracked, so naming it would imply it could be left out.
        assert!(validate_counters(&["life".to_string()]).is_err());
        assert!(validate_counters(&["stun".to_string()]).is_err());
    }

    #[test]
    fn a_stored_slug_the_build_no_longer_knows_is_dropped_rather_than_failing_the_read() {
        // A column written by a newer build and then rolled back must not make the game
        // unloadable — the read is lenient where the write is strict.
        assert_eq!(parse_counters("poison,stun"), vec!["poison"]);
        assert!(parse_counters("").is_empty());
        assert!(parse_counters("   ").is_empty());
    }

    #[test]
    fn commander_pods_open_with_the_matrix_on_and_nothing_else_does() {
        assert_eq!(
            default_counters_for(Some("Commander")),
            vec![COUNTER_COMMANDER_DAMAGE]
        );
        assert_eq!(
            default_counters_for(Some(" edh ")),
            vec![COUNTER_COMMANDER_DAMAGE]
        );
        assert!(default_counters_for(Some("standard")).is_empty());
        assert!(default_counters_for(None).is_empty());
        // Every default has to be a slug a session can actually store.
        for format in COMMANDER_FORMATS {
            for slug in default_counters_for(Some(format)) {
                assert!(OPTIONAL_COUNTERS.contains(&slug.as_str()), "{format}");
            }
        }
    }

    #[test]
    fn a_counter_the_game_does_not_track_is_refused_but_life_always_passes() {
        let enabled = vec![COUNTER_POISON.to_string()];
        assert!(require_enabled(&enabled, COUNTER_POISON).is_ok());
        assert!(require_enabled(&enabled, COUNTER_LIFE).is_ok());
        assert!(require_enabled(&[], COUNTER_LIFE).is_ok());
        assert!(require_enabled(&enabled, COUNTER_COMMANDER_DAMAGE).is_err());
    }
}

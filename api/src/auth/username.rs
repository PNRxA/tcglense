//! Username validation, normalization, and Discord-style discriminator allocation.
//!
//! Usernames are **opt-in** (a nullable column on `users`): existing accounts keep
//! `NULL`, and a username is only required the first time a user makes one of their
//! per-game collections public (#361/#362). Two users may share the same
//! case-insensitive username, disambiguated by a 4-digit discriminator, so the public
//! handle is `{username}-{discriminator}` in URLs (`{username}#{discriminator}` when
//! shown with a `#`).
//!
//! Uniqueness is on the pair `(lower(username), discriminator)`, enforced by a DB
//! unique index (SQLite `COLLATE NOCASE` / Postgres functional `lower()` index — the
//! same case-insensitive pattern the `email` column uses). This module never trusts
//! that index for *validity*: it screens length, charset, structure, a reserved-word
//! list, and the `rustrict` profanity blocklist before any row is written. The index
//! is only the tie-breaker for the concurrent-allocation race (see
//! [`allocate_discriminator`]).

use std::collections::HashSet;
use std::sync::LazyLock;

use rand::RngExt;
use rustrict::CensorStr;
use sea_orm::sea_query::{Expr, Func};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect,
};

use crate::db::Dialect;
use crate::entities::{prelude::User, user};
use crate::error::AppError;

/// Inclusive character-length bounds for the display username.
pub const MIN_USERNAME_LEN: usize = 3;
pub const MAX_USERNAME_LEN: usize = 20;

/// Inclusive discriminator range: Discord-style `0001..=9999`. Stored as `i32`,
/// rendered zero-padded to 4 digits in the public handle.
pub const MIN_DISCRIMINATOR: i32 = 1;
pub const MAX_DISCRIMINATOR: i32 = 9999;

/// Random slots to probe before falling back to a linear first-free scan.
const PROBE_ATTEMPTS: usize = 12;

/// Reserved handles that may never be claimed. Compared against the normalized
/// (trimmed, lower-cased) username — brand/impersonation names, system roles, and the
/// top-level route words that would collide with `/u/`, `/collection`, etc.
static RESERVED: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        // Brand / impersonation
        "tcglense",
        "tcglens",
        "official",
        "staff",
        "team",
        "support",
        "helpdesk",
        "moderator",
        "moderators",
        "mod",
        "mods",
        "admin",
        "admins",
        "administrator",
        "owner",
        "billing",
        "security",
        "abuse",
        "legal",
        "privacy",
        "terms",
        // Roles / system
        "root",
        "superuser",
        "sysadmin",
        "system",
        "service",
        "bot",
        "robot",
        "anonymous",
        "anon",
        "everyone",
        "here",
        "channel",
        "guest",
        "user",
        "users",
        "me",
        "self",
        "null",
        "undefined",
        "none",
        "nil",
        "nan",
        // Routes / technical (future-proofs the /u/ and top-level namespace)
        "api",
        "u",
        "auth",
        "oauth",
        "login",
        "logout",
        "signin",
        "signup",
        "register",
        "account",
        "accounts",
        "settings",
        "profile",
        "home",
        "about",
        "help",
        "contact",
        "faq",
        "docs",
        "doc",
        "blog",
        "status",
        "search",
        "collection",
        "collections",
        "wishlist",
        "wishlists",
        "card",
        "cards",
        "set",
        "sets",
        "price",
        "prices",
        "game",
        "games",
        "mail",
        "email",
        "webmaster",
        "postmaster",
        "noreply",
        "no-reply",
        "test",
        "testing",
        "demo",
        "example",
        "dev",
        "developer",
        "favicon",
        "robots",
        "sitemap",
    ]
    .into_iter()
    .collect()
});

/// Case-fold a username to the key used for uniqueness / reserved / blocklist
/// comparisons. Display casing is preserved by storing the raw (trimmed) value; the
/// normalized form is only ever a lookup key, never persisted as the display name.
pub fn normalize(username: &str) -> String {
    username.trim().to_ascii_lowercase()
}

/// Validate a user-supplied username and return the trimmed **display** form to store.
///
/// Checks run cheapest/most-deterministic first (a structural reject never pays for the
/// profanity trie); every rejection is a `422 Validation`, matching the
/// `email`/`password` precedent in `handlers/auth.rs`:
///   1. length (`MIN_USERNAME_LEN..=MAX_USERNAME_LEN`, in chars),
///   2. charset (`[A-Za-z0-9_]`),
///   3. structure (no leading/trailing `_`, no consecutive `__`),
///   4. reserved-word list (exact, case-insensitive),
///   5. `rustrict` profanity blocklist.
///
/// The returned string keeps the caller's casing, so `Ada_Lovelace` displays as typed
/// but collides case-insensitively with `ada_lovelace` at the DB layer.
pub fn validate(raw: &str) -> Result<String, AppError> {
    let display = raw.trim();

    let len = display.chars().count();
    if len < MIN_USERNAME_LEN || len > MAX_USERNAME_LEN {
        return Err(AppError::Validation(format!(
            "username must be between {MIN_USERNAME_LEN} and {MAX_USERNAME_LEN} characters"
        )));
    }

    if !display
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(AppError::Validation(
            "username may only contain letters, numbers, and underscores".to_string(),
        ));
    }

    if display.starts_with('_') || display.ends_with('_') || display.contains("__") {
        return Err(AppError::Validation(
            "username may not start or end with, or repeat, an underscore".to_string(),
        ));
    }

    let normalized = normalize(display);
    if RESERVED.contains(normalized.as_str()) {
        return Err(AppError::Validation(
            "that username is reserved; please choose another".to_string(),
        ));
    }

    // rustrict's default trie carries its own false-positive list (so "class",
    // "assassin", "scunthorpe" pass) and normalizes leet-speak; the ASCII charset gate
    // above already removes the homoglyph-evasion surface.
    if display.is_inappropriate() {
        return Err(AppError::Validation(
            "that username isn't allowed; please choose another".to_string(),
        ));
    }

    Ok(display.to_string())
}

/// Allocate a currently-free discriminator for `normalized_username` (pass the output
/// of [`normalize`]). Reads the taken discriminators for that case-insensitive username
/// — excluding the caller's own row so a rename can keep its current tag — honours
/// `prefer` when it is still free (stable handle across a rename), then probes random
/// slots and falls back to a linear first-free scan. Returns `409 Conflict` when all
/// 9,999 slots are taken.
///
/// Only *chooses* a candidate — it does not insert. Two concurrent allocations can pick
/// the same slot, so the caller must treat the unique index on
/// `(lower(username), discriminator)` as truth and re-allocate on a
/// `UniqueConstraintViolation` (see the handler retry loop in `handlers/auth.rs`).
pub async fn allocate_discriminator(
    db: &DatabaseConnection,
    normalized_username: &str,
    prefer: Option<i32>,
    exclude_user_id: i32,
) -> Result<i32, AppError> {
    // Case-insensitive username match, and each dialect's form is served by
    // `idx_users_username_discriminator` (a seek, not a scan) — mirroring `resolve_public_user`:
    // Postgres matches the functional `lower(username)` index; SQLite matches the plain
    // `COLLATE NOCASE` column directly (`username = ?` — a predicate on `lower(username)` there
    // would be an expression the NOCASE index can't seek, so it would full-scan). Excluding the
    // caller's own row lets a rename to a free name keep — or the same name re-submit and keep —
    // its current discriminator.
    let query = User::find()
        .select_only()
        .column(user::Column::Discriminator)
        .filter(user::Column::Discriminator.is_not_null())
        .filter(user::Column::Id.ne(exclude_user_id));
    let query = match Dialect::from_backend(db.get_database_backend()) {
        Dialect::Postgres => query.filter(
            Expr::expr(Func::lower(Expr::col(user::Column::Username))).eq(normalized_username),
        ),
        Dialect::Sqlite => query.filter(user::Column::Username.eq(normalized_username)),
    };
    let taken: HashSet<i32> = query
        .into_tuple::<i32>()
        .all(db)
        .await?
        .into_iter()
        .collect();

    // Keep the preferred (current) discriminator across a rename when it is still free.
    if let Some(d) = prefer {
        if (MIN_DISCRIMINATOR..=MAX_DISCRIMINATOR).contains(&d) && !taken.contains(&d) {
            return Ok(d);
        }
    }

    let total = (MAX_DISCRIMINATOR - MIN_DISCRIMINATOR + 1) as usize; // 9,999
    if taken.len() >= total {
        return Err(AppError::Conflict(
            "this username is full; please choose another".to_string(),
        ));
    }

    pick_free(&taken, &mut rand::rng()).ok_or_else(|| {
        AppError::Conflict("this username is full; please choose another".to_string())
    })
}

/// Pure slot-picker (unit-testable without a DB): random probing then linear
/// first-free fallback. `None` only if every slot in range is taken.
fn pick_free(taken: &HashSet<i32>, rng: &mut impl RngExt) -> Option<i32> {
    for _ in 0..PROBE_ATTEMPTS {
        let candidate = rng.random_range(MIN_DISCRIMINATOR..=MAX_DISCRIMINATOR);
        if !taken.contains(&candidate) {
            return Some(candidate);
        }
    }
    (MIN_DISCRIMINATOR..=MAX_DISCRIMINATOR).find(|d| !taken.contains(d))
}

/// Render a stored `(username, discriminator)` pair as the canonical URL handle,
/// e.g. `("Ada_Lovelace", 7)` -> `"Ada_Lovelace-0007"`.
pub fn format_handle(username: &str, discriminator: i32) -> String {
    format!("{username}-{discriminator:04}")
}

/// The public handle for a user, or `None` until they have chosen a username. Both
/// `username` and `discriminator` are always set/cleared together.
pub fn handle_of(user: &user::Model) -> Option<String> {
    match (user.username.as_deref(), user.discriminator) {
        (Some(name), Some(disc)) => Some(format_handle(name, disc)),
        _ => None,
    }
}

/// Parse a URL handle (`{username}-{discriminator}`) back into its parts, or `None` if
/// malformed. Usernames never contain `-` (charset is `[A-Za-z0-9_]`), so the last `-`
/// unambiguously separates the two. Returns the username in its URL casing; callers
/// resolve it case-insensitively via [`normalize`].
pub fn parse_handle(handle: &str) -> Option<(String, i32)> {
    let (username, disc) = handle.rsplit_once('-')?;
    // Only the canonical zero-padded 4-digit form `format_handle` emits (`{disc:04}`; the
    // 1..=9999 range never needs more than 4 digits) resolves. Reject every other spelling —
    // a leading `+`, or short/over-padded zeros that `i32::from_str` would otherwise accept —
    // so exactly one handle string maps to a collection (no duplicate URLs splitting the CDN
    // cache or canonical link).
    if disc.len() != 4 || !disc.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let disc: i32 = disc.parse().ok()?;
    if !(MIN_DISCRIMINATOR..=MAX_DISCRIMINATOR).contains(&disc) {
        return None;
    }
    if username.is_empty() {
        return None;
    }
    Some((username.to_string(), disc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn validate_accepts_reasonable_names() {
        for name in ["ada", "Ada_Lovelace", "x_9", "collector42", "MTG_fan"] {
            assert!(validate(name).is_ok(), "expected {name} to be accepted");
        }
    }

    #[test]
    fn validate_preserves_display_casing_and_trims() {
        assert_eq!(validate("  Ada_Lovelace  ").unwrap(), "Ada_Lovelace");
    }

    #[test]
    fn validate_rejects_bad_length() {
        assert!(matches!(validate("ab"), Err(AppError::Validation(_))));
        assert!(matches!(
            validate(&"a".repeat(21)),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn validate_rejects_bad_charset() {
        for name in ["bad-name", "bad name", "näme", "dot.dot", "at@sign"] {
            assert!(
                matches!(validate(name), Err(AppError::Validation(_))),
                "{name}"
            );
        }
    }

    #[test]
    fn validate_rejects_bad_underscore_structure() {
        for name in ["_lead", "trail_", "a__b"] {
            assert!(
                matches!(validate(name), Err(AppError::Validation(_))),
                "{name}"
            );
        }
    }

    #[test]
    fn validate_rejects_reserved_case_insensitively() {
        for name in ["admin", "ADMIN", "TCGLense", "Support"] {
            assert!(
                matches!(validate(name), Err(AppError::Validation(_))),
                "{name}"
            );
        }
    }

    #[test]
    fn validate_rejects_profanity() {
        assert!(matches!(validate("fuck"), Err(AppError::Validation(_))));
    }

    #[test]
    fn normalize_folds_case() {
        assert_eq!(normalize("  Ada_Lovelace "), "ada_lovelace");
    }

    #[test]
    fn pick_free_avoids_taken() {
        let taken: HashSet<i32> = (1..=5000).collect();
        let mut rng = StdRng::seed_from_u64(7);
        let chosen = pick_free(&taken, &mut rng).unwrap();
        assert!(!taken.contains(&chosen));
        assert!((MIN_DISCRIMINATOR..=MAX_DISCRIMINATOR).contains(&chosen));
    }

    #[test]
    fn pick_free_none_when_full() {
        let taken: HashSet<i32> = (MIN_DISCRIMINATOR..=MAX_DISCRIMINATOR).collect();
        let mut rng = StdRng::seed_from_u64(1);
        assert_eq!(pick_free(&taken, &mut rng), None);
    }

    #[test]
    fn handle_round_trips() {
        assert_eq!(format_handle("Ada_Lovelace", 7), "Ada_Lovelace-0007");
        assert_eq!(
            parse_handle("Ada_Lovelace-0007"),
            Some(("Ada_Lovelace".to_string(), 7))
        );
    }

    #[test]
    fn parse_handle_rejects_malformed() {
        assert_eq!(parse_handle("nodash"), None);
        assert_eq!(parse_handle("x-0"), None); // discriminator out of range (and not 4-digit)
        assert_eq!(parse_handle("x-10000"), None);
        assert_eq!(parse_handle("-0007"), None); // empty username
        assert_eq!(parse_handle("x-abc"), None); // non-numeric discriminator
    }

    #[test]
    fn parse_handle_requires_canonical_four_digit_tag() {
        // Only the exact zero-padded 4-digit form resolves; every other spelling of the same
        // number is rejected, so one collection has one canonical handle URL.
        assert_eq!(parse_handle("x-0007"), Some(("x".to_string(), 7))); // canonical
        assert_eq!(parse_handle("x-7"), None); // unpadded
        assert_eq!(parse_handle("x-007"), None); // short pad
        assert_eq!(parse_handle("x-00007"), None); // over-padded
        assert_eq!(parse_handle("x-+007"), None); // leading sign
        assert_eq!(parse_handle("x-0000"), None); // 4-digit but out of range
    }
}

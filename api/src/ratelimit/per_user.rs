//! Per-user limiter: the [`UserRateLimiters`] limiter set + the [`user_rate_limit`]
//! axum middleware guarding the authenticated API surface. See the module docs in
//! [`super`].

use std::time::Duration;

use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use governor::{
    Quota, RateLimiter,
    clock::{Clock, DefaultClock},
    state::keyed::DefaultKeyedStateStore,
};
use nonzero_ext::nonzero;
use serde_json::json;

use crate::{auth::jwt::decode_token, config::Config, state::AppState};

use super::retry_after_from_wait;

/// A per-user limiter keyed by the authenticated user's id (`users.id`), decoded
/// from the request's access token.
pub(super) type UserKeyedLimiter = RateLimiter<i32, DefaultKeyedStateStore<i32>, DefaultClock>;

/// The per-user rate-limit classes for the authenticated API surface. Each gets its
/// own keyed limiter + quota, so a burst of imports can't spend a browse session's
/// budget and vice versa (mirroring [`super::per_ip::AuthRoute`]'s per-IP split).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UserRoute {
    /// General authenticated requests — collection reads, absolute-count edits,
    /// batch owned-count lookups, `me`. A generous ceiling for a signed-in human.
    General,
    /// The expensive collection import / sync / CSV-upload endpoints, which do real
    /// server-side work (an upstream fetch, a CSV parse, or a full-collection DB
    /// reconcile). A much tighter cap.
    Import,
}

impl UserRoute {
    /// Classify an authenticated request path into its per-user quota class. `{game}`
    /// is a path variable, so this matches on the trailing segments after
    /// `/api/collection/{game}`; everything else (reads, edits, `me`, an unknown
    /// path) falls into the generous [`Self::General`] bucket.
    pub(super) fn from_path(path: &str) -> Self {
        if let Some(rest) = path.strip_prefix("/api/collection/")
            && let Some((_game, tail)) = rest.split_once('/')
            && matches!(tail, "import" | "import/csv" | "sync")
        {
            return Self::Import;
        }
        Self::General
    }

    /// Per-user quota: a sustained rate with an initial burst allowance (governor's
    /// GCRA starts the cell full, so up to `burst` requests pass immediately, then
    /// one every `period/burst`). `Quota::per_minute(n)` sets both to `n`.
    pub(super) fn quota(self) -> Quota {
        match self {
            // ~300/min (5/s): plenty for a human browsing + editing a collection
            // (list + summary + batch owned-count lookups per page), tight for a
            // script grinding the API.
            Self::General => Quota::per_minute(nonzero!(300u32)),
            // Imports are heavy and already globally serialised; a low per-user cap
            // stops one account queuing / CSV-spamming them.
            Self::Import => Quota::per_minute(nonzero!(10u32)),
        }
    }

    /// Stable Redis key-class token for this route (`rl:user:<class>:<uid>`). Kept
    /// next to the enum so the key naming can't drift from the variants.
    pub(super) fn class(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Import => "import",
        }
    }
}

/// One keyed limiter per per-user route class, plus the shared clock (for computing
/// `Retry-After`). Held in [`AppState`] and cloned cheaply (each limiter is `Arc`-y
/// internally via its shared state store). The per-user complement to
/// [`super::per_ip::RateLimiters`].
pub struct UserRateLimiters {
    general: UserKeyedLimiter,
    import: UserKeyedLimiter,
    clock: DefaultClock,
}

impl Default for UserRateLimiters {
    fn default() -> Self {
        Self {
            general: RateLimiter::keyed(UserRoute::General.quota()),
            import: RateLimiter::keyed(UserRoute::Import.quota()),
            clock: DefaultClock::default(),
        }
    }
}

impl UserRateLimiters {
    fn limiter(&self, route: UserRoute) -> &UserKeyedLimiter {
        match route {
            UserRoute::General => &self.general,
            UserRoute::Import => &self.import,
        }
    }

    /// Check one request against `route`'s limiter for `user_id`. `Ok(())` if
    /// allowed; `Err(retry_after)` (rounded up to whole seconds, min 1) if limited.
    pub(super) fn check(&self, route: UserRoute, user_id: i32) -> Result<(), Duration> {
        match self.limiter(route).check_key(&user_id) {
            Ok(()) => Ok(()),
            Err(not_until) => {
                let wait = not_until.wait_time_from(self.clock.now());
                Err(retry_after_from_wait(wait))
            }
        }
    }

    /// Drop keys whose limiter cell has fully replenished, bounding memory. Called
    /// periodically from the maintenance task alongside
    /// [`super::per_ip::RateLimiters::retain_recent`].
    pub(super) fn retain_recent(&self) {
        self.general.retain_recent();
        self.import.retain_recent();
    }
}

/// Pull the raw `Authorization: Bearer <token>` value out of a request, if present
/// and non-empty. Shared by the JWT and API-key user-id resolvers below.
fn bearer_token(request: &Request) -> Option<&str> {
    let value = request.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// Pull the authenticated user id out of a request's `Authorization: Bearer` access
/// token, decoding (but deliberately *not* DB-loading) it — the rate check must be
/// cheap and run before any query. Returns `None` for an unauthenticated request or
/// an invalid/expired token: either way there's no user to key on, so the per-user
/// limiter is skipped.
fn bearer_user_id(request: &Request, config: &Config) -> Option<i32> {
    let token = bearer_token(request)?;
    decode_token(token, config).ok()?.sub.parse::<i32>().ok()
}

/// Extract an **owned** API-key bearer credential (a `tcgl_`-prefixed token), if the
/// request carries one. Returning an owned `String` — rather than a borrow of the
/// request — is deliberate: the caller resolves it against the DB across an `.await`,
/// and holding a `&Request` across that await would make the middleware future
/// non-`Send` (axum's `Body` isn't `Sync`). `None` for a missing / non-key token.
fn bearer_api_key_token(request: &Request) -> Option<String> {
    bearer_token(request)
        .filter(|t| t.starts_with(crate::auth::api_key::KEY_PLAINTEXT_PREFIX))
        .map(str::to_owned)
}

/// Axum middleware: enforce the per-user limit for an authenticated request's class.
/// An unauthenticated request (no/invalid bearer token) or a disabled limiter passes
/// through untouched. A limited request is a `429` carrying `Retry-After` and the
/// standard `{ "error": … }` body — mirroring [`super::per_ip::rate_limit`], and
/// layered inside `no_store_layer` so the `429` is never shared-cached.
pub async fn user_rate_limit(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if !state.config.rate_limit_enabled {
        return next.run(request).await;
    }
    // A session JWT resolves without a DB hit; fall back to an API-key lookup so
    // key-authenticated traffic is throttled per user rather than bypassing the cap.
    // The API-key token is extracted (owned) before the await so no `&request` borrow
    // is held across it (which would make this future non-`Send`).
    let user_id = match bearer_user_id(&request, &state.config) {
        Some(id) => id,
        None => match bearer_api_key_token(&request) {
            Some(token) => match crate::auth::api_key::resolve_user_id(&state.db, &token).await {
                Ok(Some(id)) => id,
                // Not a valid key (unknown/revoked/expired) or a lookup error: no
                // user to key on — the extractor rejects it downstream.
                _ => return next.run(request).await,
            },
            // No authenticated user to key on: nothing to limit here.
            None => return next.run(request).await,
        },
    };

    let route = UserRoute::from_path(request.uri().path());
    match state.user_rate_limiters.check(route, user_id).await {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            tracing::info!(
                user_id,
                path = %request.uri().path(),
                "authenticated request rate-limited per user"
            );
            (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, retry_after.as_secs().to_string())],
                axum::Json(json!({ "error": "too many requests; please slow down" })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_import_class_allows_its_burst_then_blocks() {
        let limiters = UserRateLimiters::default();
        let user = 7;

        // The import class allows a burst of 10, then the 11th is limited.
        for i in 0..10 {
            assert!(
                limiters.check(UserRoute::Import, user).is_ok(),
                "import {i} within the burst should pass"
            );
        }
        let retry = limiters
            .check(UserRoute::Import, user)
            .expect_err("the 11th import is limited");
        assert!(retry.as_secs() >= 1);
    }

    #[test]
    fn user_general_class_allows_its_burst_then_blocks() {
        let limiters = UserRateLimiters::default();
        let user = 9;

        // The general class allows a burst of 300, then the 301st is limited — proof
        // that the authenticated read/edit surface IS throttled per user, not just the
        // tight import class. (At 300/min a replenished cell is ~200ms away, so the
        // retry is a positive sub-second wait rather than the import class's >=1s.)
        for i in 0..300 {
            assert!(
                limiters.check(UserRoute::General, user).is_ok(),
                "general request {i} within the burst should pass"
            );
        }
        let retry = limiters
            .check(UserRoute::General, user)
            .expect_err("the 301st general request is limited");
        assert!(!retry.is_zero(), "a limited request reports a positive retry-after");
    }

    #[test]
    fn user_limits_are_per_user() {
        let limiters = UserRateLimiters::default();

        // Exhaust the import burst for user 1.
        for _ in 0..10 {
            let _ = limiters.check(UserRoute::Import, 1);
        }
        assert!(limiters.check(UserRoute::Import, 1).is_err());

        // A different user has its own budget and is unaffected.
        assert!(limiters.check(UserRoute::Import, 2).is_ok());
    }

    #[test]
    fn user_route_classes_are_independent() {
        let limiters = UserRateLimiters::default();
        let user = 42;

        // Exhaust the tight import class for the user...
        for _ in 0..10 {
            let _ = limiters.check(UserRoute::Import, user);
        }
        assert!(limiters.check(UserRoute::Import, user).is_err());

        // ...the general class is a separate, far larger budget — well past the
        // import burst (10), a browse session keeps flowing.
        for i in 0..50 {
            assert!(
                limiters.check(UserRoute::General, user).is_ok(),
                "general request {i} should pass while imports are exhausted"
            );
        }
    }

    #[test]
    fn user_route_classifies_expensive_endpoints() {
        // The import / sync / CSV-upload endpoints are the tight class.
        assert_eq!(
            UserRoute::from_path("/api/collection/mtg/import"),
            UserRoute::Import
        );
        assert_eq!(
            UserRoute::from_path("/api/collection/mtg/import/csv"),
            UserRoute::Import
        );
        assert_eq!(
            UserRoute::from_path("/api/collection/mtg/sync"),
            UserRoute::Import
        );

        // Reads, edits, job polling, and non-collection authenticated routes are general
        // (the whole wishlist surface included — it has no expensive import twin).
        for general in [
            "/api/collection/mtg",
            "/api/collection/mtg/summary",
            "/api/collection/mtg/sets",
            "/api/collection/mtg/cards/some-external-id",
            "/api/collection/mtg/import/jobs/1",
            "/api/wishlist/mtg",
            "/api/wishlist/mtg/counts",
            "/api/wishlist/mtg/cards/some-external-id",
            "/api/auth/me",
        ] {
            assert_eq!(
                UserRoute::from_path(general),
                UserRoute::General,
                "{general} should be the general class"
            );
        }
    }
}

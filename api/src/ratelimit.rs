//! Abuse-protection rate limiting, in two complementary flavours:
//!
//! * **Per-IP** ([`RateLimiters`] + [`rate_limit`]) — guards the unauthenticated
//!   auth endpoints (login/register/email-send/token) against brute-force /
//!   mail-bombing, keyed by the resolved client IP (see [`crate::client_ip`]). When
//!   the IP can't be resolved (only the in-process test harness, which has no socket
//!   peer) the request fails open — a real deployment always has a peer address.
//! * **Per-user** ([`UserRateLimiters`] + [`user_rate_limit`]) — guards the
//!   *authenticated* API surface (the collection + wishlist endpoints + `me`), keyed by the
//!   user id in the access token, so it caps what one account can do regardless of
//!   the IP it comes from (issue #168). A request with no valid bearer token has no
//!   user to key on and passes through (it's a public route, or gets a `401` from
//!   the handler's `AuthUser` extractor — not the limiter's job).
//!
//! Each protected route class has its own keyed [`governor`] limiter (GCRA), so a
//! burst on one endpoint doesn't spend another's budget. All state is in-memory
//! (like the collection-import queue): limits are per-process and reset on restart,
//! and a multi-instance deploy would want a shared store. `retain_recent` (on both
//! limiter sets) is swept periodically so the keyspace can't grow unbounded.

use std::{
    net::{IpAddr, Ipv6Addr},
    time::Duration,
};

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

use crate::{
    auth::jwt::decode_token, client_ip::resolve_client_ip, config::Config, state::AppState,
};

type KeyedLimiter = RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>;

/// The key an IP is bucketed under. IPv4 is keyed whole; IPv6 is masked to its
/// /64 prefix — a single client is routinely handed a whole /64 (or larger), so
/// per-/128 keying would let it evade the limit just by rotating source
/// addresses (and would balloon the keyspace). /64 is the smallest block a host
/// is reliably assigned, so it's the natural per-client unit.
fn rate_limit_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => {
            let mut octets = v6.octets();
            octets[8..].fill(0);
            IpAddr::V6(Ipv6Addr::from(octets))
        }
    }
}

/// The rate-limited auth route classes. Each gets its own limiter + quota; the
/// quotas below are deliberately generous for a human and tight for a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthRoute {
    /// Credential submission — the brute-force / credential-stuffing surface.
    Login,
    /// Account creation — bot-signup surface.
    Register,
    /// Email-sending endpoints — mail-bombing surface (on top of the per-user
    /// token cooldown).
    EmailSend,
    /// Emailed-token consumption (verify-email / reset-password) — token-guessing
    /// surface (already infeasible against 256-bit tokens; this just caps abuse).
    Token,
}

impl AuthRoute {
    /// Map a request path to its rate-limit class, or `None` for a route we
    /// don't limit (refresh/logout/me are legitimate high-frequency session ops).
    fn from_path(path: &str) -> Option<Self> {
        match path {
            "/api/auth/login" => Some(Self::Login),
            "/api/auth/register" => Some(Self::Register),
            "/api/auth/forgot-password" | "/api/auth/resend-verification" => Some(Self::EmailSend),
            "/api/auth/verify-email" | "/api/auth/reset-password" => Some(Self::Token),
            _ => None,
        }
    }

    /// Per-IP quota: a sustained rate plus an initial burst allowance (governor's
    /// GCRA lets the cell start full, so up to `burst` requests are allowed
    /// immediately, then one every `period/burst`).
    fn quota(self) -> Quota {
        match self {
            // ~10 attempts/min: a human mistypes a few times; a stuffer can't grind.
            Self::Login => Quota::per_minute(nonzero!(10u32)),
            // A handful of accounts per minute from one IP.
            Self::Register => Quota::per_minute(nonzero!(5u32)).allow_burst(nonzero!(5u32)),
            // Tight — one address can't be mail-bombed via these.
            Self::EmailSend => Quota::per_minute(nonzero!(5u32)).allow_burst(nonzero!(5u32)),
            // Looser — a user may click an emailed link a couple of times.
            Self::Token => Quota::per_minute(nonzero!(20u32)),
        }
    }
}

/// One keyed limiter per auth route class, plus the shared clock (for computing
/// `Retry-After`). Held in `AppState` and cloned cheaply (each limiter is an
/// `Arc` internally via the shared state store).
pub struct RateLimiters {
    login: KeyedLimiter,
    register: KeyedLimiter,
    email_send: KeyedLimiter,
    token: KeyedLimiter,
    clock: DefaultClock,
}

impl Default for RateLimiters {
    fn default() -> Self {
        // `keyed` builds each limiter on `DefaultClock` (the global quanta clock);
        // the `clock` we keep for `Retry-After` reads the same timeline, so
        // `wait_time_from` lines up with each limiter's internal instants.
        Self {
            login: RateLimiter::keyed(AuthRoute::Login.quota()),
            register: RateLimiter::keyed(AuthRoute::Register.quota()),
            email_send: RateLimiter::keyed(AuthRoute::EmailSend.quota()),
            token: RateLimiter::keyed(AuthRoute::Token.quota()),
            clock: DefaultClock::default(),
        }
    }
}

impl RateLimiters {
    fn limiter(&self, route: AuthRoute) -> &KeyedLimiter {
        match route {
            AuthRoute::Login => &self.login,
            AuthRoute::Register => &self.register,
            AuthRoute::EmailSend => &self.email_send,
            AuthRoute::Token => &self.token,
        }
    }

    /// Check one request against `route`'s limiter for `ip`. `Ok(())` if allowed;
    /// `Err(retry_after)` (rounded up to whole seconds, min 1) if limited.
    fn check(&self, route: AuthRoute, ip: IpAddr) -> Result<(), Duration> {
        match self.limiter(route).check_key(&rate_limit_key(ip)) {
            Ok(()) => Ok(()),
            Err(not_until) => {
                let wait = not_until.wait_time_from(self.clock.now());
                Err(Duration::from_secs(wait.as_secs().max(1)))
            }
        }
    }

    /// Drop keys whose limiter cell has fully replenished, bounding memory. Called
    /// periodically from the maintenance task.
    pub fn retain_recent(&self) {
        self.login.retain_recent();
        self.register.retain_recent();
        self.email_send.retain_recent();
        self.token.retain_recent();
    }
}

/// Axum middleware: enforce the per-IP limit for the request's auth route class.
/// A non-auth path, a disabled limiter, or an unresolvable client IP all pass
/// through. A limited request is a `429` carrying `Retry-After` and the standard
/// `{ "error": … }` body.
pub async fn rate_limit(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if !state.config.rate_limit_enabled {
        return next.run(request).await;
    }
    let Some(route) = AuthRoute::from_path(request.uri().path()) else {
        return next.run(request).await;
    };

    let peer = request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip());
    let Some(ip) = resolve_client_ip(request.headers(), peer, state.config.trust_proxy_headers)
    else {
        // No resolvable client IP: nothing to key on, so fail open.
        return next.run(request).await;
    };

    match state.rate_limiters.check(route, ip) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            tracing::info!(%ip, path = %request.uri().path(), "auth request rate-limited");
            (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, retry_after.as_secs().to_string())],
                axum::Json(json!({ "error": "too many requests; please slow down" })),
            )
                .into_response()
        }
    }
}

// ---------- Per-user rate limiting (the authenticated API surface) ----------

/// A per-user limiter keyed by the authenticated user's id (`users.id`), decoded
/// from the request's access token.
type UserKeyedLimiter = RateLimiter<i32, DefaultKeyedStateStore<i32>, DefaultClock>;

/// The per-user rate-limit classes for the authenticated API surface. Each gets its
/// own keyed limiter + quota, so a burst of imports can't spend a browse session's
/// budget and vice versa (mirroring [`AuthRoute`]'s per-IP split).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserRoute {
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
    fn from_path(path: &str) -> Self {
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
    fn quota(self) -> Quota {
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
}

/// One keyed limiter per per-user route class, plus the shared clock (for computing
/// `Retry-After`). Held in [`AppState`] and cloned cheaply (each limiter is `Arc`-y
/// internally via its shared state store). The per-user complement to
/// [`RateLimiters`].
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
    fn check(&self, route: UserRoute, user_id: i32) -> Result<(), Duration> {
        match self.limiter(route).check_key(&user_id) {
            Ok(()) => Ok(()),
            Err(not_until) => {
                let wait = not_until.wait_time_from(self.clock.now());
                Err(Duration::from_secs(wait.as_secs().max(1)))
            }
        }
    }

    /// Drop keys whose limiter cell has fully replenished, bounding memory. Called
    /// periodically from the maintenance task alongside [`RateLimiters::retain_recent`].
    pub fn retain_recent(&self) {
        self.general.retain_recent();
        self.import.retain_recent();
    }
}

/// Pull the authenticated user id out of a request's `Authorization: Bearer` access
/// token, decoding (but deliberately *not* DB-loading) it — the rate check must be
/// cheap and run before any query. Returns `None` for an unauthenticated request or
/// an invalid/expired token: either way there's no user to key on, so the per-user
/// limiter is skipped.
fn bearer_user_id(request: &Request, config: &Config) -> Option<i32> {
    let value = request.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let token = value
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|t| !t.is_empty())?;
    decode_token(token, config).ok()?.sub.parse::<i32>().ok()
}

/// Axum middleware: enforce the per-user limit for an authenticated request's class.
/// An unauthenticated request (no/invalid bearer token) or a disabled limiter passes
/// through untouched. A limited request is a `429` carrying `Retry-After` and the
/// standard `{ "error": … }` body — mirroring [`rate_limit`], and layered inside
/// `no_store_layer` so the `429` is never shared-cached.
pub async fn user_rate_limit(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if !state.config.rate_limit_enabled {
        return next.run(request).await;
    }
    let Some(user_id) = bearer_user_id(&request, &state.config) else {
        // No authenticated user to key on: nothing to limit here.
        return next.run(request).await;
    };

    let route = UserRoute::from_path(request.uri().path());
    match state.user_rate_limiters.check(route, user_id) {
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
    fn limiter_allows_the_burst_then_blocks() {
        let limiters = RateLimiters::default();
        let ip: IpAddr = "203.0.113.7".parse().unwrap();

        // Register allows a burst of 5, then the 6th is limited.
        for i in 0..5 {
            assert!(
                limiters.check(AuthRoute::Register, ip).is_ok(),
                "request {i} within the burst should pass"
            );
        }
        let retry = limiters
            .check(AuthRoute::Register, ip)
            .expect_err("the 6th register is limited");
        assert!(retry.as_secs() >= 1);
    }

    #[test]
    fn ipv6_addresses_are_keyed_by_their_64_prefix() {
        let limiters = RateLimiters::default();
        // Two addresses in the same /64 share a bucket, so rotating within it
        // can't dodge the limit.
        let a: IpAddr = "2001:db8:abcd:1234::1".parse().unwrap();
        let b: IpAddr = "2001:db8:abcd:1234:ffff:ffff:ffff:ffff".parse().unwrap();
        for _ in 0..5 {
            let _ = limiters.check(AuthRoute::Register, a);
        }
        assert!(
            limiters.check(AuthRoute::Register, b).is_err(),
            "a sibling /128 in the same /64 shares the bucket"
        );

        // A different /64 has its own budget.
        let c: IpAddr = "2001:db8:abcd:9999::1".parse().unwrap();
        assert!(limiters.check(AuthRoute::Register, c).is_ok());
    }

    #[test]
    fn limits_are_per_ip_and_per_route() {
        let limiters = RateLimiters::default();
        let a: IpAddr = "203.0.113.1".parse().unwrap();
        let b: IpAddr = "203.0.113.2".parse().unwrap();

        // Exhaust register for `a`.
        for _ in 0..5 {
            let _ = limiters.check(AuthRoute::Register, a);
        }
        assert!(limiters.check(AuthRoute::Register, a).is_err());

        // A different IP is unaffected...
        assert!(limiters.check(AuthRoute::Register, b).is_ok());
        // ...and a different route for the same IP has its own budget.
        assert!(limiters.check(AuthRoute::Login, a).is_ok());
    }

    // ----- Per-user limiting -----

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

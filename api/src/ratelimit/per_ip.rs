//! Per-IP auth limiter: the [`RateLimiters`] limiter set + the [`rate_limit`] axum
//! middleware guarding the unauthenticated auth endpoints. See the module docs in
//! [`super`].

use std::{net::IpAddr, time::Duration};

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

use crate::{client_ip::resolve_client_ip, state::AppState};

use super::{rate_limit_key, retry_after_from_wait};

pub(super) type KeyedLimiter = RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>;

/// The per-IP rate-limited route classes: the four unauthenticated auth surfaces,
/// plus the two unauthenticated public read surfaces (issue #413 — before that the
/// public catalog and sharing routes had **no** limiter of any kind, so scripted
/// enumeration could drive expensive scans against the DB unthrottled). Each gets
/// its own limiter + quota; the quotas below are deliberately generous for a human
/// and tight for a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IpRoute {
    /// Credential submission — the brute-force / credential-stuffing surface.
    Login,
    /// Account creation — bot-signup surface.
    Register,
    /// Email-sending endpoints — mail-bombing surface (on top of the per-user
    /// token cooldown).
    EmailSend,
    /// Emailed-token consumption (verify-email / reset-password /
    /// complete-registration) — token-guessing surface (already infeasible
    /// against 256-bit tokens; this just caps abuse).
    Token,
    /// The DB-query public catalog reads (`/api/games/…` search, autocomplete,
    /// set/product/card pages). The image/icon proxies are deliberately excluded —
    /// one legitimate browse-grid page load fires dozens of art requests — as are
    /// the import-status poll (the SPA polls it several times a second during an
    /// import) and the authed scan POST (per-user limited already).
    PublicCatalog,
    /// The unauthenticated public-sharing surface (`/api/u/…` profile/collection
    /// reads plus the body-keyed owned-counts POST — the latter is uncacheable at
    /// every HTTP layer, so this limiter is its only shield).
    PublicHoldings,
}

impl IpRoute {
    /// Map a request path to its rate-limit class, or `None` for a route we
    /// don't limit (refresh/logout/me are legitimate high-frequency session ops;
    /// images/icons and the status poll are legitimate high-frequency reads).
    pub(super) fn from_path(path: &str) -> Option<Self> {
        match path {
            "/api/auth/login" => return Some(Self::Login),
            "/api/auth/register" => return Some(Self::Register),
            "/api/auth/forgot-password" | "/api/auth/resend-verification" => {
                return Some(Self::EmailSend);
            }
            "/api/auth/verify-email"
            | "/api/auth/reset-password"
            | "/api/auth/complete-registration" => return Some(Self::Token),
            _ => {}
        }

        if let Some(rest) = path.strip_prefix("/api/games/") {
            // Art, the status poll, and the authed scan stay un-limited (see the
            // variant docs); everything else under a game is a DB-query read.
            if rest.ends_with("/image")
                || rest.ends_with("/icon")
                || rest.ends_with("/status")
                || rest.ends_with("/scan")
            {
                return None;
            }
            return Some(Self::PublicCatalog);
        }
        if path.starts_with("/api/u/") {
            return Some(Self::PublicHoldings);
        }

        None
    }

    /// Per-IP quota: a sustained rate plus an initial burst allowance (governor's
    /// GCRA lets the cell start full, so up to `burst` requests are allowed
    /// immediately, then one every `period/burst`).
    pub(super) fn quota(self) -> Quota {
        match self {
            // ~10 attempts/min: a human mistypes a few times; a stuffer can't grind.
            Self::Login => Quota::per_minute(nonzero!(10u32)),
            // A handful of accounts per minute from one IP.
            Self::Register => Quota::per_minute(nonzero!(5u32)).allow_burst(nonzero!(5u32)),
            // Tight — one address can't be mail-bombed via these.
            Self::EmailSend => Quota::per_minute(nonzero!(5u32)).allow_burst(nonzero!(5u32)),
            // Looser — a user may click an emailed link a couple of times.
            Self::Token => Quota::per_minute(nonzero!(20u32)),
            // ~300/min (5/s sustained, burst 300): far above any human browse /
            // type-ahead session (matching the authed General quota) and above a
            // CDN edge's SWR back-fill, while capping a scraper grinding the
            // search/autocomplete scans against the weak prod Postgres.
            Self::PublicCatalog => Quota::per_minute(nonzero!(300u32)),
            // ~120/min (2/s sustained): a public collection view is 3-4 reads per
            // landing and 2-3 per browse page, so this is roomy for a human and a
            // hard wall for scraping someone's shared collection.
            Self::PublicHoldings => Quota::per_minute(nonzero!(120u32)),
        }
    }

    /// Stable Redis key-class token for this route (`rl:auth:<class>:<ip>`). Kept
    /// next to the enum so the key naming can't drift from the variants.
    pub(super) fn class(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Register => "register",
            Self::EmailSend => "email_send",
            Self::Token => "token",
            Self::PublicCatalog => "public_catalog",
            Self::PublicHoldings => "public_holdings",
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
    public_catalog: KeyedLimiter,
    public_holdings: KeyedLimiter,
    clock: DefaultClock,
}

impl Default for RateLimiters {
    fn default() -> Self {
        // `keyed` builds each limiter on `DefaultClock` (the global quanta clock);
        // the `clock` we keep for `Retry-After` reads the same timeline, so
        // `wait_time_from` lines up with each limiter's internal instants.
        Self {
            login: RateLimiter::keyed(IpRoute::Login.quota()),
            register: RateLimiter::keyed(IpRoute::Register.quota()),
            email_send: RateLimiter::keyed(IpRoute::EmailSend.quota()),
            token: RateLimiter::keyed(IpRoute::Token.quota()),
            public_catalog: RateLimiter::keyed(IpRoute::PublicCatalog.quota()),
            public_holdings: RateLimiter::keyed(IpRoute::PublicHoldings.quota()),
            clock: DefaultClock::default(),
        }
    }
}

impl RateLimiters {
    fn limiter(&self, route: IpRoute) -> &KeyedLimiter {
        match route {
            IpRoute::Login => &self.login,
            IpRoute::Register => &self.register,
            IpRoute::EmailSend => &self.email_send,
            IpRoute::Token => &self.token,
            IpRoute::PublicCatalog => &self.public_catalog,
            IpRoute::PublicHoldings => &self.public_holdings,
        }
    }

    /// Check one request against `route`'s limiter for `ip`. `Ok(())` if allowed;
    /// `Err(retry_after)` (rounded up to whole seconds, min 1) if limited.
    pub(super) fn check(&self, route: IpRoute, ip: IpAddr) -> Result<(), Duration> {
        match self.limiter(route).check_key(&rate_limit_key(ip)) {
            Ok(()) => Ok(()),
            Err(not_until) => {
                let wait = not_until.wait_time_from(self.clock.now());
                Err(retry_after_from_wait(wait))
            }
        }
    }

    /// Drop keys whose limiter cell has fully replenished, bounding memory. Called
    /// periodically from the maintenance task.
    pub(super) fn retain_recent(&self) {
        self.login.retain_recent();
        self.register.retain_recent();
        self.email_send.retain_recent();
        self.token.retain_recent();
        self.public_catalog.retain_recent();
        self.public_holdings.retain_recent();
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
    let Some(route) = IpRoute::from_path(request.uri().path()) else {
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

    match state.rate_limiters.check(route, ip).await {
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
                limiters.check(IpRoute::Register, ip).is_ok(),
                "request {i} within the burst should pass"
            );
        }
        let retry = limiters
            .check(IpRoute::Register, ip)
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
            let _ = limiters.check(IpRoute::Register, a);
        }
        assert!(
            limiters.check(IpRoute::Register, b).is_err(),
            "a sibling /128 in the same /64 shares the bucket"
        );

        // A different /64 has its own budget.
        let c: IpAddr = "2001:db8:abcd:9999::1".parse().unwrap();
        assert!(limiters.check(IpRoute::Register, c).is_ok());
    }

    #[test]
    fn auth_route_classifies_each_endpoint() {
        // The three token-consuming endpoints share the looser Token class — in
        // particular email-first registration's completion step, so it isn't
        // silently unlimited or lumped in with account creation.
        assert_eq!(
            IpRoute::from_path("/api/auth/complete-registration"),
            Some(IpRoute::Token)
        );
        assert_eq!(
            IpRoute::from_path("/api/auth/verify-email"),
            Some(IpRoute::Token)
        );
        assert_eq!(
            IpRoute::from_path("/api/auth/reset-password"),
            Some(IpRoute::Token)
        );

        // Account creation and the email-sending endpoints keep their own classes.
        assert_eq!(
            IpRoute::from_path("/api/auth/register"),
            Some(IpRoute::Register)
        );
        assert_eq!(
            IpRoute::from_path("/api/auth/forgot-password"),
            Some(IpRoute::EmailSend)
        );
        assert_eq!(
            IpRoute::from_path("/api/auth/resend-verification"),
            Some(IpRoute::EmailSend)
        );

        // Session ops we deliberately don't per-IP limit fall through to `None`.
        assert_eq!(IpRoute::from_path("/api/auth/refresh"), None);
    }

    #[test]
    fn public_surfaces_classify_with_their_exclusions() {
        // The DB-query catalog reads are limited (issue #413)...
        for catalog in [
            "/api/games/mtg/cards",
            "/api/games/mtg/card-names",
            "/api/games/mtg/cards/abc123",
            "/api/games/mtg/cards/abc123/prices",
            "/api/games/mtg/sets",
            "/api/games/mtg/products/17/cards/sections",
        ] {
            assert_eq!(
                IpRoute::from_path(catalog),
                Some(IpRoute::PublicCatalog),
                "{catalog} should be the public-catalog class"
            );
        }

        // ...but the art proxies (dozens per legitimate grid page), the SPA's
        // import-status poll, and the authed scan POST are deliberately not.
        for excluded in [
            "/api/games/mtg/cards/abc123/image",
            "/api/games/mtg/products/17/image",
            "/api/games/mtg/sets/neo/icon",
            "/api/games/mtg/import/status",
            "/api/games/mtg/scan",
        ] {
            assert_eq!(IpRoute::from_path(excluded), None, "{excluded} is un-limited");
        }

        // The bare game list is static (no DB) and un-limited.
        assert_eq!(IpRoute::from_path("/api/games"), None);

        // The public sharing surface — including the body-keyed owned POST, its
        // only shield — is the holdings class.
        for holdings in [
            "/api/u/alice-1234",
            "/api/u/alice-1234/mtg",
            "/api/u/alice-1234/mtg/summary",
            "/api/u/alice-1234/mtg/owned",
            "/api/u/alice-1234/decks/7",
        ] {
            assert_eq!(
                IpRoute::from_path(holdings),
                Some(IpRoute::PublicHoldings),
                "{holdings} should be the public-holdings class"
            );
        }

        // Sitemaps / OpenAPI / config / mirror stay un-limited (crawler-driven or
        // DB-free; the CDN owns their caching story).
        for other in ["/api/sitemap.xml", "/api/openapi.json", "/api/config"] {
            assert_eq!(IpRoute::from_path(other), None, "{other} is un-limited");
        }
    }

    #[test]
    fn limits_are_per_ip_and_per_route() {
        let limiters = RateLimiters::default();
        let a: IpAddr = "203.0.113.1".parse().unwrap();
        let b: IpAddr = "203.0.113.2".parse().unwrap();

        // Exhaust register for `a`.
        for _ in 0..5 {
            let _ = limiters.check(IpRoute::Register, a);
        }
        assert!(limiters.check(IpRoute::Register, a).is_err());

        // A different IP is unaffected...
        assert!(limiters.check(IpRoute::Register, b).is_ok());
        // ...and a different route for the same IP has its own budget.
        assert!(limiters.check(IpRoute::Login, a).is_ok());
    }
}

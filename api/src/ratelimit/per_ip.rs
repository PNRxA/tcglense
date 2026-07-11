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

/// The rate-limited auth route classes. Each gets its own limiter + quota; the
/// quotas below are deliberately generous for a human and tight for a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AuthRoute {
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
}

impl AuthRoute {
    /// Map a request path to its rate-limit class, or `None` for a route we
    /// don't limit (refresh/logout/me are legitimate high-frequency session ops).
    pub(super) fn from_path(path: &str) -> Option<Self> {
        match path {
            "/api/auth/login" => Some(Self::Login),
            "/api/auth/register" => Some(Self::Register),
            "/api/auth/forgot-password" | "/api/auth/resend-verification" => Some(Self::EmailSend),
            "/api/auth/verify-email"
            | "/api/auth/reset-password"
            | "/api/auth/complete-registration" => Some(Self::Token),
            _ => None,
        }
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
    pub(super) fn check(&self, route: AuthRoute, ip: IpAddr) -> Result<(), Duration> {
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
    fn auth_route_classifies_each_endpoint() {
        // The three token-consuming endpoints share the looser Token class — in
        // particular email-first registration's completion step, so it isn't
        // silently unlimited or lumped in with account creation.
        assert_eq!(
            AuthRoute::from_path("/api/auth/complete-registration"),
            Some(AuthRoute::Token)
        );
        assert_eq!(
            AuthRoute::from_path("/api/auth/verify-email"),
            Some(AuthRoute::Token)
        );
        assert_eq!(
            AuthRoute::from_path("/api/auth/reset-password"),
            Some(AuthRoute::Token)
        );

        // Account creation and the email-sending endpoints keep their own classes.
        assert_eq!(
            AuthRoute::from_path("/api/auth/register"),
            Some(AuthRoute::Register)
        );
        assert_eq!(
            AuthRoute::from_path("/api/auth/forgot-password"),
            Some(AuthRoute::EmailSend)
        );
        assert_eq!(
            AuthRoute::from_path("/api/auth/resend-verification"),
            Some(AuthRoute::EmailSend)
        );

        // Session ops we deliberately don't per-IP limit fall through to `None`.
        assert_eq!(AuthRoute::from_path("/api/auth/refresh"), None);
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
}

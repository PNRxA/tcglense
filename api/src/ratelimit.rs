//! Per-IP rate limiting for the auth endpoints — brute-force / mail-bombing /
//! abuse protection (issue #… follow-up to the auth feature).
//!
//! Each protected route class has its own keyed [`governor`] limiter (GCRA), so
//! a burst on one endpoint doesn't spend another's budget. The limiter is keyed
//! by the resolved client IP (see [`crate::client_ip`]); when the IP can't be
//! resolved (only the in-process test harness, which has no socket peer) the
//! request fails open — a real deployment always has a peer address.
//!
//! State is in-memory (like the collection-import queue): limits are per-process
//! and reset on restart, and a multi-instance deploy would want a shared store.
//! [`RateLimiters::retain_recent`] is swept periodically so the keyspace can't
//! grow unbounded.

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

type KeyedLimiter = RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, DefaultClock>;

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
        match self.limiter(route).check_key(&ip) {
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

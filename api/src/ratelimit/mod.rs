//! Abuse-protection rate limiting, in two complementary flavours:
//!
//! * **Per-IP** ([`RateLimiters`] + [`rate_limit`]) — guards the unauthenticated
//!   surfaces, keyed by the resolved client IP (see [`crate::client_ip`]): the auth
//!   endpoints (login/register/email-send/token) against brute-force /
//!   mail-bombing, and — since issue #413 — the public catalog and public-sharing
//!   reads against scripted enumeration driving expensive scans (generous quotas;
//!   the image/icon proxies and the status poll are deliberately un-limited). When
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
//! burst on one endpoint doesn't spend another's budget. By default all state is
//! in-memory (like the collection-import queue): limits are per-process and reset
//! on restart. `retain_recent` (on both limiter sets) is swept periodically so the
//! keyspace can't grow unbounded.
//!
//! **Optional Redis backend.** When `REDIS_URL` is set (and Redis is reachable at
//! boot) the two limiter sets are backed by a shared Redis instead, so a
//! multi-instance deploy enforces one budget across every replica. The
//! [`AuthRateLimiter`] / [`UserRateLimiter`] enums wrap the in-memory
//! [`RateLimiters`] / [`UserRateLimiters`] (kept untouched) with a Redis arm that
//! runs the *same* GCRA in a Lua script ([`backend::GCRA_LUA`]) — deriving its
//! parameters from the identical [`governor::Quota`] the in-memory arm uses, so the
//! two can't drift. Rate limiting is abuse protection, not integrity, so the Redis arm
//! **fails open**: a Redis error at boot starts the server degraded (in-memory) and
//! a Redis error on a live check falls back to the embedded in-memory limiter (with
//! a throttled warning). See [`AuthRateLimiter`].
//!
//! This module is split into three focused submodules:
//!
//! * [`per_ip`] — the per-IP auth limiter ([`RateLimiters`] + [`rate_limit`]).
//! * [`per_user`] — the per-user API limiter ([`UserRateLimiters`] + [`user_rate_limit`]).
//! * [`backend`] — the optional Redis GCRA backend + the [`AuthRateLimiter`] /
//!   [`UserRateLimiter`] wrapper enums held in [`crate::state::AppState`].

use std::{
    net::{IpAddr, Ipv6Addr},
    time::Duration,
};

mod backend;
mod per_ip;
mod per_user;

pub use backend::{AuthRateLimiter, UserRateLimiter, connect_redis};
pub use per_ip::rate_limit;
pub use per_user::user_rate_limit;

/// The key an IP is bucketed under. IPv4 is keyed whole; IPv6 is masked to its
/// /64 prefix — a single client is routinely handed a whole /64 (or larger), so
/// per-/128 keying would let it evade the limit just by rotating source
/// addresses (and would balloon the keyspace). /64 is the smallest block a host
/// is reliably assigned, so it's the natural per-client unit.
pub(super) fn rate_limit_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => {
            let mut octets = v6.octets();
            octets[8..].fill(0);
            IpAddr::V6(Ipv6Addr::from(octets))
        }
    }
}

/// Round a raw GCRA wait to the `Retry-After` value both limiter flavours emit:
/// whole seconds, **floored**, minimum 1s. Shared by the in-memory and Redis arms
/// so their rounding can never drift; reproduces the pre-Redis inline expression
/// `Duration::from_secs(wait.as_secs().max(1))` exactly (a floor, not a round-up).
pub(super) fn retry_after_from_wait(raw: Duration) -> Duration {
    Duration::from_secs(raw.as_secs().max(1))
}

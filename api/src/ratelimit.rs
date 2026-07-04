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
//! runs the *same* GCRA in a Lua script ([`GCRA_LUA`]) — deriving its parameters
//! from the identical [`governor::Quota`] the in-memory arm uses, so the two can't
//! drift. Rate limiting is abuse protection, not integrity, so the Redis arm
//! **fails open**: a Redis error at boot starts the server degraded (in-memory) and
//! a Redis error on a live check falls back to the embedded in-memory limiter (with
//! a throttled warning). See [`AuthRateLimiter`].

use std::{
    net::{IpAddr, Ipv6Addr},
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
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

/// Round a raw GCRA wait to the `Retry-After` value both limiter flavours emit:
/// whole seconds, **floored**, minimum 1s. Shared by the in-memory and Redis arms
/// so their rounding can never drift; reproduces the pre-Redis inline expression
/// `Duration::from_secs(wait.as_secs().max(1))` exactly (a floor, not a round-up).
fn retry_after_from_wait(raw: Duration) -> Duration {
    Duration::from_secs(raw.as_secs().max(1))
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
    /// Emailed-token consumption (verify-email / reset-password /
    /// complete-registration) — token-guessing surface (already infeasible
    /// against 256-bit tokens; this just caps abuse).
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
            "/api/auth/verify-email"
            | "/api/auth/reset-password"
            | "/api/auth/complete-registration" => Some(Self::Token),
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

    /// Stable Redis key-class token for this route (`rl:auth:<class>:<ip>`). Kept
    /// next to the enum so the key naming can't drift from the variants.
    fn class(self) -> &'static str {
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
    fn check(&self, route: AuthRoute, ip: IpAddr) -> Result<(), Duration> {
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

    /// Stable Redis key-class token for this route (`rl:user:<class>:<uid>`). Kept
    /// next to the enum so the key naming can't drift from the variants.
    fn class(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Import => "import",
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
                Err(retry_after_from_wait(wait))
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

// ---------- Optional Redis backend (distributed rate limiting) ----------
//
// The two enums below wrap the in-memory `RateLimiters` / `UserRateLimiters` above
// (kept exactly as they are, sync `check` and all) with an optional Redis arm. The
// Redis arm runs the same GCRA as governor, in a Lua script, against a shared Redis
// so a multi-instance deploy enforces one budget. Both arms read the identical
// `Quota` (via `AuthRoute::quota()` / `UserRoute::quota()`), so they can't diverge.

/// Derive the GCRA parameters — emission interval `t` and burst tolerance `tau`,
/// both in **microseconds** — from the *same* [`governor::Quota`] the in-memory arm
/// uses, so the Redis arm and the governor arm can never diverge. `tau = (burst-1)*t`
/// is governor's own delay-variation tolerance (`gcra.rs`: `t = replenish_1_per`,
/// `tau = t*(burst-1)`), i.e. a burst of `burst` cells, then one cell every `t`.
/// Microseconds keep full fidelity (all current intervals are exact ms, but µs is
/// future-proof) and a µs-since-epoch TAT (≈1.7e15) stays well under 2^53, so it is
/// exact in the Lua script's doubles.
fn gcra_params(quota: Quota) -> (u64, u64) {
    let t = quota.replenish_interval().as_micros() as u64;
    let burst = u64::from(quota.burst_size().get());
    // `burst >= 1` always (governor's `NonZeroU32`), so `burst - 1` can't underflow;
    // saturating_mul is belt-and-braces against an absurd future quota.
    let tau = t.saturating_mul(burst - 1);
    (t, tau)
}

/// GCRA (virtual-scheduling) rate limiter, matching governor's smooth-emission +
/// burst semantics cell-for-cell. State is a single key holding the theoretical
/// arrival time (TAT) as microseconds-since-epoch. It reads the Redis **server**
/// clock (`TIME`) so every app instance shares one clock (NTP skew between instances
/// can't corrupt the shared TAT); this is safe under effects replication (Redis
/// ≥5.0 default, always on Redis 7), which replicates the resulting `SET` rather
/// than the non-deterministic script.
///
/// Equivalence to `governor::gcra`: an empty key ⇒ `tat = now` (cell starts full);
/// deny when `now < tat - tau`; else store `max(tat, now) + t`. The deny path does
/// **not** write, so it never refreshes the key's TTL (the TAT is unchanged) — same
/// as governor's `measure_and_replace` not replacing on `Err`.
///
/// The stored TAT is set to expire (`PX`) exactly when the cell would fully
/// replenish (`new_tat - now`, ≤ `tau + t`); after that a fresh request sees an
/// empty key = the fully-replenished state. This is the Redis-native equivalent of
/// `retain_recent()`, so the Redis arm needs no periodic sweep.
///
/// The new TAT is stored via `string.format('%.0f', …)` so the integer is written in
/// full decimal form (never scientific notation), keeping it exactly round-trippable
/// through `GET`/`tonumber` regardless of the server's Lua number format.
///
/// KEYS[1] = limiter key                       (e.g. rl:auth:login:203.0.113.7)
/// ARGV[1] = t   (emission interval, µs)
/// ARGV[2] = tau (burst tolerance,  µs)
/// returns {allowed (1/0), retry_after_micros (0 when allowed)}
const GCRA_LUA: &str = r#"
local t = tonumber(ARGV[1])
local tau = tonumber(ARGV[2])
local now
do
  local time = redis.call('TIME')
  now = (tonumber(time[1]) * 1000000) + tonumber(time[2])
end
local tat = tonumber(redis.call('GET', KEYS[1]))
if tat == nil then
  tat = now
end
local allow_at = tat - tau
if now < allow_at then
  return {0, allow_at - now}
end
local new_tat = math.max(tat, now) + t
local ttl_ms = math.ceil((new_tat - now) / 1000)
if ttl_ms < 1 then ttl_ms = 1 end
redis.call('SET', KEYS[1], string.format('%.0f', new_tat), 'PX', ttl_ms)
return {1, 0}
"#;

/// One shared [`redis::Script`] instance (hashes the body once; `invoke_async`
/// EVALSHA-caches per connection, EVAL-ing on a `NOSCRIPT` reply).
static GCRA_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| redis::Script::new(GCRA_LUA));

/// Run one GCRA check against Redis for `key` under `quota`. Returns the *inner*
/// decision (`Ok(())` allowed / `Err(retry_after)` limited) on success, or the
/// `redis::RedisError` on any transport/script failure (the caller fails open).
/// `conn` is cheap to clone (`ConnectionManager` is `Arc`-internal + multiplexed).
async fn redis_gcra_check(
    conn: &redis::aio::ConnectionManager,
    key: &str,
    quota: Quota,
) -> Result<Result<(), Duration>, redis::RedisError> {
    let (t, tau) = gcra_params(quota);
    let mut conn = conn.clone();
    let (allowed, retry_micros): (i64, i64) = GCRA_SCRIPT
        .key(key)
        .arg(t)
        .arg(tau)
        .invoke_async(&mut conn)
        .await?;
    if allowed == 1 {
        Ok(Ok(()))
    } else {
        // Round through the shared helper so a Redis-arm 429 rounds identically to
        // an in-memory 429 (whole seconds, floored, min 1s).
        let raw = Duration::from_micros(retry_micros.max(0) as u64);
        Ok(Err(retry_after_from_wait(raw)))
    }
}

/// Emit at most one warning per 30s when a Redis check fails and we fall back to the
/// in-memory limiter, so a Redis outage can't flood the logs (one warn line per
/// request would). CAS on a unix-seconds stamp: a check-then-swap so only the racer
/// that advances the stamp logs.
fn note_redis_degraded(last_warn_secs: &AtomicU64, err: &redis::RedisError) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let prev = last_warn_secs.load(Ordering::Relaxed);
    if now.saturating_sub(prev) >= 30
        && last_warn_secs
            .compare_exchange(prev, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    {
        tracing::warn!(
            error = %err,
            "Redis rate-limiter check failed; falling back to the in-memory limiter"
        );
    }
}

/// Build an auto-reconnecting multiplexed connection to Redis. Called once at
/// startup ([`crate::main`]). Returns `Err` if the URL is malformed, points at a
/// TLS (`rediss://`) endpoint (this build compiles the no-TLS `redis` feature set),
/// or Redis is unreachable at boot; the caller degrades to in-memory (fail-open)
/// rather than aborting.
///
/// `ConnectionManager::new` establishes its first connection eagerly (so a dead
/// Redis surfaces here at boot) and thereafter reconnects automatically, so a
/// *mid-life* outage is handled by (1) that auto-reconnect and (2) the per-check
/// fail-open in [`AuthRateLimiter::check`] / [`UserRateLimiter::check`].
pub async fn connect_redis(url: &str) -> redis::RedisResult<redis::aio::ConnectionManager> {
    // This build links the no-TLS `redis` feature set, so a `rediss://`/`valkeys://`
    // URL can never connect. Reject it up front with an actionable message instead
    // of the crate's terser "the feature is not enabled" (the caller logs it, then
    // degrades to the in-memory limiter).
    let scheme = url.split("://").next().unwrap_or("").to_ascii_lowercase();
    if scheme == "rediss" || scheme == "valkeys" {
        return Err(redis::RedisError::from((
            redis::ErrorKind::InvalidClientConfig,
            "TLS Redis URLs (rediss://) are not supported by this build; use a \
             plain redis:// endpoint on a private network",
        )));
    }
    let client = redis::Client::open(url)?;
    redis::aio::ConnectionManager::new(client).await
}

/// Per-IP auth limiter: the in-memory governor limiters, or Redis GCRA with the
/// governor limiters kept as a fail-open fallback. Mirrors the enum-dispatch idiom
/// of [`crate::email::Emailer`] / [`crate::captcha::Captcha`]. Always held behind
/// `Arc` in [`AppState`], so it is never cloned (the `AtomicU64` needs no `Arc`).
pub enum AuthRateLimiter {
    /// No `REDIS_URL` (or Redis was unreachable at boot): per-process governor.
    InMemory(RateLimiters),
    /// Redis-backed: the shared GCRA, with the governor limiter kept as the
    /// fail-open fallback used whenever a Redis check errors.
    Redis {
        conn: redis::aio::ConnectionManager,
        fallback: RateLimiters,
        /// Unix-seconds of the last degradation warning, to throttle the log.
        last_warn_secs: AtomicU64,
    },
}

impl AuthRateLimiter {
    /// Redis-backed when a connection is supplied (`REDIS_URL` set + reachable at
    /// boot), else in-memory. Built once in [`AppState::new`].
    pub fn new(redis: Option<redis::aio::ConnectionManager>) -> Self {
        match redis {
            Some(conn) => Self::Redis {
                conn,
                fallback: RateLimiters::default(),
                last_warn_secs: AtomicU64::new(0),
            },
            None => Self::InMemory(RateLimiters::default()),
        }
    }

    /// Check `route` for `ip`. `Ok(())` allowed; `Err(retry_after)` limited. The
    /// Redis arm fails open to the in-memory fallback on any Redis error. Kept
    /// module-private (called only from the [`rate_limit`] middleware) so it doesn't
    /// leak the private [`AuthRoute`] in a public signature.
    async fn check(&self, route: AuthRoute, ip: IpAddr) -> Result<(), Duration> {
        match self {
            Self::InMemory(inner) => inner.check(route, ip),
            Self::Redis {
                conn,
                fallback,
                last_warn_secs,
            } => {
                let key = format!("rl:auth:{}:{}", route.class(), rate_limit_key(ip));
                match redis_gcra_check(conn, &key, route.quota()).await {
                    Ok(decision) => decision,
                    Err(err) => {
                        note_redis_degraded(last_warn_secs, &err);
                        fallback.check(route, ip)
                    }
                }
            }
        }
    }

    /// Bound the in-memory keyspace (the governor arm, or the Redis arm's fallback
    /// used during an outage). Redis keys self-evict via `PEXPIRE`, so the Redis arm
    /// only sweeps its fallback — cheap and harmless.
    pub fn retain_recent(&self) {
        match self {
            Self::InMemory(inner) => inner.retain_recent(),
            Self::Redis { fallback, .. } => fallback.retain_recent(),
        }
    }
}

/// Per-user limiter: the exact twin of [`AuthRateLimiter`] over
/// [`UserRateLimiters`] / [`UserRoute`] / `i32` user ids.
pub enum UserRateLimiter {
    InMemory(UserRateLimiters),
    Redis {
        conn: redis::aio::ConnectionManager,
        fallback: UserRateLimiters,
        last_warn_secs: AtomicU64,
    },
}

impl UserRateLimiter {
    /// Redis-backed when a connection is supplied, else in-memory. Built once in
    /// [`AppState::new`] (sharing the one multiplexed connection with the auth arm).
    pub fn new(redis: Option<redis::aio::ConnectionManager>) -> Self {
        match redis {
            Some(conn) => Self::Redis {
                conn,
                fallback: UserRateLimiters::default(),
                last_warn_secs: AtomicU64::new(0),
            },
            None => Self::InMemory(UserRateLimiters::default()),
        }
    }

    /// Check `route` for `user_id`. `Ok(())` allowed; `Err(retry_after)` limited.
    /// The Redis arm fails open to the in-memory fallback on any Redis error. Kept
    /// module-private (called only from the [`user_rate_limit`] middleware) so it
    /// doesn't leak the private [`UserRoute`] in a public signature.
    async fn check(&self, route: UserRoute, user_id: i32) -> Result<(), Duration> {
        match self {
            Self::InMemory(inner) => inner.check(route, user_id),
            Self::Redis {
                conn,
                fallback,
                last_warn_secs,
            } => {
                let key = format!("rl:user:{}:{}", route.class(), user_id);
                match redis_gcra_check(conn, &key, route.quota()).await {
                    Ok(decision) => decision,
                    Err(err) => {
                        note_redis_degraded(last_warn_secs, &err);
                        fallback.check(route, user_id)
                    }
                }
            }
        }
    }

    /// Bound the in-memory keyspace (see [`AuthRateLimiter::retain_recent`]).
    pub fn retain_recent(&self) {
        match self {
            Self::InMemory(inner) => inner.retain_recent(),
            Self::Redis { fallback, .. } => fallback.retain_recent(),
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

    // ----- Redis GCRA parity (no Redis needed) -----

    #[test]
    fn gcra_params_track_the_shared_quota() {
        // The Redis arm derives (t, tau) from the SAME `Quota` the governor arm
        // uses, so a quota retune can't silently drift the two apart. The reference
        // numbers below are computed from the current quotas — this test fails if a
        // quota changes without updating them, forcing a conscious re-check.
        let cases = [
            (AuthRoute::Login, 6_000_000u64, 54_000_000u64),
            (AuthRoute::Register, 12_000_000, 48_000_000),
            (AuthRoute::EmailSend, 12_000_000, 48_000_000),
            (AuthRoute::Token, 3_000_000, 57_000_000),
        ];
        for (route, t, tau) in cases {
            assert_eq!(gcra_params(route.quota()), (t, tau), "{route:?}");
        }
        assert_eq!(gcra_params(UserRoute::General.quota()), (200_000, 59_800_000));
        assert_eq!(gcra_params(UserRoute::Import.quota()), (6_000_000, 54_000_000));
    }

    #[test]
    fn tau_is_burst_minus_one_emission_intervals() {
        // The invariant the parity rests on, independent of the hard-coded numbers:
        // governor's own `tau = t * (burst - 1)` (see `gcra_params`).
        for route in [
            AuthRoute::Login,
            AuthRoute::Register,
            AuthRoute::EmailSend,
            AuthRoute::Token,
        ] {
            let q = route.quota();
            let (t, tau) = gcra_params(q);
            assert_eq!(tau, t * (u64::from(q.burst_size().get()) - 1), "{route:?}");
        }
        for route in [UserRoute::General, UserRoute::Import] {
            let q = route.quota();
            let (t, tau) = gcra_params(q);
            assert_eq!(tau, t * (u64::from(q.burst_size().get()) - 1), "{route:?}");
        }
    }

    #[test]
    fn redis_key_carries_the_64_prefix_mask() {
        // The Redis auth key reuses the same /64 normalisation as the in-memory
        // bucket, so a client can't dodge the shared limit by rotating within a /64.
        let ip: IpAddr = "2001:db8:abcd:1234::1".parse().unwrap();
        let key = format!("rl:auth:{}:{}", AuthRoute::Login.class(), rate_limit_key(ip));
        assert_eq!(key, "rl:auth:login:2001:db8:abcd:1234::");
    }

    // ----- Redis integration (env-gated; requires a live Redis) -----
    //
    // These are `#[ignore]`d so the default `cargo test` (in-memory SQLite, no
    // services) skips them. Run them against a Redis with
    // `TCGLENSE_TEST_REDIS_URL=redis://localhost:6379 cargo test -- --ignored`.
    // Each test uses a random uid / IP so keys never collide across a shared Redis
    // (and every key self-expires), so no FLUSHDB is needed.

    fn test_redis_url() -> Option<String> {
        std::env::var("TCGLENSE_TEST_REDIS_URL").ok()
    }

    /// A Redis-backed auth limiter over the test Redis, or `None` when the gate env
    /// var is unset (so a test can early-return instead of failing).
    async fn redis_auth_arm() -> Option<AuthRateLimiter> {
        let url = test_redis_url()?;
        let conn = connect_redis(&url).await.expect("connect test redis");
        Some(AuthRateLimiter::Redis {
            conn,
            fallback: RateLimiters::default(),
            last_warn_secs: AtomicU64::new(0),
        })
    }

    #[tokio::test]
    #[ignore = "requires a live Redis; set TCGLENSE_TEST_REDIS_URL, run with --ignored"]
    async fn redis_arm_allows_burst_then_limits() {
        let Some(limiter) = redis_auth_arm().await else {
            return;
        };
        // Fresh /64 per run so the key starts empty (Register burst = 5).
        let ip: IpAddr = format!("2001:db8:{:x}::1", rand::random::<u16>())
            .parse()
            .unwrap();
        for i in 0..5 {
            assert!(
                limiter.check(AuthRoute::Register, ip).await.is_ok(),
                "burst {i}"
            );
        }
        let retry = limiter
            .check(AuthRoute::Register, ip)
            .await
            .expect_err("the 6th is limited");
        assert!(retry.as_secs() >= 1); // Retry-After: min 1s
        assert!(retry.as_secs() <= 12); // ≤ emission interval (12s)
    }

    #[tokio::test]
    #[ignore = "requires a live Redis; set TCGLENSE_TEST_REDIS_URL, run with --ignored"]
    async fn redis_key_has_a_bounded_ttl() {
        let Some(url) = test_redis_url() else {
            return;
        };
        let conn = connect_redis(&url).await.unwrap();
        let limiter = AuthRateLimiter::Redis {
            conn: conn.clone(),
            fallback: RateLimiters::default(),
            last_warn_secs: AtomicU64::new(0),
        };
        let ip: IpAddr = format!("198.51.{}.{}", rand::random::<u8>(), rand::random::<u8>())
            .parse()
            .unwrap();
        limiter.check(AuthRoute::Register, ip).await.unwrap();
        let key = format!("rl:auth:register:{}", rate_limit_key(ip));
        let mut c = conn.clone();
        let pttl: i64 = redis::cmd("PTTL")
            .arg(&key)
            .query_async(&mut c)
            .await
            .unwrap();
        // 0 < pttl <= ceil(tau + t) = 60_000 ms for Register (tau 48s + t 12s).
        assert!(pttl > 0 && pttl <= 60_000, "pttl={pttl}");
    }

    #[tokio::test]
    #[ignore = "requires a live Redis; set TCGLENSE_TEST_REDIS_URL, run with --ignored"]
    async fn redis_and_inmemory_agree_on_the_burst_boundary() {
        // Same Quota, same decision sequence: both allow exactly `burst`, deny next.
        let Some(limiter) = redis_auth_arm().await else {
            return;
        };
        let inmem = RateLimiters::default();
        let ip_r: IpAddr = format!("203.0.{}.{}", rand::random::<u8>(), rand::random::<u8>())
            .parse()
            .unwrap();
        let ip_m: IpAddr = "203.0.113.200".parse().unwrap();
        for _ in 0..20 {
            assert!(inmem.check(AuthRoute::Token, ip_m).is_ok()); // Token burst=20
        }
        assert!(inmem.check(AuthRoute::Token, ip_m).is_err());
        for _ in 0..20 {
            assert!(limiter.check(AuthRoute::Token, ip_r).await.is_ok());
        }
        assert!(limiter.check(AuthRoute::Token, ip_r).await.is_err());
    }

    #[tokio::test]
    #[ignore = "requires the redis-server binary on PATH"]
    async fn redis_outage_fails_open_to_inmemory() {
        // Spawn a private redis-server on a random port (skip if the binary is
        // absent), verify limiting works, kill it, then assert a subsequent check
        // still returns Ok — via the in-memory fallback, not a propagated error.
        let port = 20000 + (rand::random::<u16>() % 20000);
        let Ok(mut child) = std::process::Command::new("redis-server")
            .args(["--port", &port.to_string(), "--save", "", "--appendonly", "no"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            return;
        };
        tokio::time::sleep(Duration::from_millis(300)).await;
        let url = format!("redis://127.0.0.1:{port}");
        let conn = connect_redis(&url).await.expect("connect spawned redis");
        let limiter = AuthRateLimiter::Redis {
            conn,
            fallback: RateLimiters::default(),
            last_warn_secs: AtomicU64::new(0),
        };
        let ip: IpAddr = "203.0.113.9".parse().unwrap();
        assert!(limiter.check(AuthRoute::Login, ip).await.is_ok()); // Redis up
        let _ = child.kill(); // Redis gone
        tokio::time::sleep(Duration::from_millis(200)).await;
        // Fails open to the fresh in-memory fallback (which allows), not an error.
        assert!(limiter.check(AuthRoute::Login, ip).await.is_ok());
    }
}

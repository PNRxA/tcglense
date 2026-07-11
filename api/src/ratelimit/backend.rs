//! Optional Redis backend (distributed rate limiting).
//!
//! The two enums below wrap the in-memory [`RateLimiters`] / [`UserRateLimiters`]
//! (kept exactly as they are, sync `check` and all) with an optional Redis arm. The
//! Redis arm runs the same GCRA as governor, in a Lua script, against a shared Redis
//! so a multi-instance deploy enforces one budget. Both arms read the identical
//! `Quota` (via `AuthRoute::quota()` / `UserRoute::quota()`), so they can't diverge.

use std::{
    net::IpAddr,
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use governor::Quota;

use super::per_ip::{AuthRoute, RateLimiters};
use super::per_user::{UserRateLimiters, UserRoute};
use super::{rate_limit_key, retry_after_from_wait};

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
/// startup ([`crate::main`]). Returns `Err` if the URL is malformed or Redis is
/// unreachable at boot; the caller degrades to in-memory (fail-open) rather than
/// aborting.
///
/// Both plain `redis://` and TLS `rediss://` (e.g. a hosted Upstash/Valkey endpoint
/// reached over the public internet) are supported: the build links redis's rustls
/// TLS feature set with bundled Mozilla roots, and [`crate::main`] installs the
/// shared aws-lc-rs rustls provider. `Client::open` selects TLS from the URL scheme.
///
/// `ConnectionManager::new` establishes its first connection eagerly (so a dead
/// Redis surfaces here at boot) and thereafter reconnects automatically, so a
/// *mid-life* outage is handled by (1) that auto-reconnect and (2) the per-check
/// fail-open in [`AuthRateLimiter::check`] / [`UserRateLimiter::check`].
pub async fn connect_redis(url: &str) -> redis::RedisResult<redis::aio::ConnectionManager> {
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
    /// module-private (called only from the [`super::rate_limit`] middleware) so it
    /// doesn't leak the private [`AuthRoute`] in a public signature.
    pub(super) async fn check(&self, route: AuthRoute, ip: IpAddr) -> Result<(), Duration> {
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
    /// module-private (called only from the [`super::user_rate_limit`] middleware) so
    /// it doesn't leak the private [`UserRoute`] in a public signature.
    pub(super) async fn check(&self, route: UserRoute, user_id: i32) -> Result<(), Duration> {
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

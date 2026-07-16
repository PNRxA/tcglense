//! Version-keyed response cache for the collection analytics pair — value-history
//! and movers (issues #413 / #365).
//!
//! Those two endpoints are the heaviest per-user reads in the app: each request
//! re-scans every held card/product's **entire** captured price history
//! (O(held items × captured days)) against the weak prod Postgres, they are
//! `no-store` per-user routes no HTTP/CDN layer can ever shield, and the SPA
//! refetches them on every landing mount, tab refocus, and holdings edit. Between
//! a user's own edits and the daily price capture their responses cannot change —
//! so they cache perfectly under two version counters:
//!
//! * `holdings version` — per `(user, game)`, bumped by every collection holdings
//!   mutation (card count writes, sealed-product count writes, import/sync
//!   reconciles). Bumping changes the body key, so stale entries orphan and age
//!   out via TTL rather than needing deletion.
//! * `price epoch` — per game, bumped when the background sync finishes a tick
//!   (price refresh + daily snapshot), when the historic backfill completes, and
//!   when foil enrichment finishes — i.e. whenever price/history rows may have
//!   changed outside any user's own actions.
//!
//! The current UTC date is also part of every body key: the windowed ranges
//! compute their cutoffs from the wall clock, so the same versions must not serve
//! yesterday's window after midnight (nor on a capture-gap day, issue #445).
//!
//! **Degradation contract** (mirrors [`crate::ratelimit`]'s enum-dispatch, with
//! the opposite failure bias): Redis-backed when `REDIS_URL` was reachable at
//! boot — shared across replicas — else a bounded in-process map (correct on the
//! single-instance shapes; multi-replica deploys already require Redis for the
//! rate limiters). Any Redis error **bypasses the cache entirely** for that
//! request and falls through to the database: this is a pure performance layer,
//! so the source of truth always wins. In particular, a failed *version* read
//! must never be defaulted — a guessed version could match a stale body.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::header;
use axum::response::{IntoResponse, Response};
use chrono::Utc;

/// TTL for cached response bodies. A backstop only — correctness comes from the
/// version keys — bounding both Redis storage and the staleness of any entry
/// whose invalidation path is missed by a future mutation seam.
const BODY_TTL: Duration = Duration::from_secs(3600);

/// TTL for the version counters. Must far exceed [`BODY_TTL`]: an expired counter
/// re-reads as 0, which is only safe because any body cached under the old count
/// is long gone by then.
const VERSION_TTL_SECS: i64 = 30 * 24 * 3600;

/// Bound on the in-process body map (entries, not bytes). Movers bodies are the
/// largest at a few tens of KB, so the worst case is a few MB per process.
const MEMORY_BODY_CAP: usize = 256;

/// Bodies larger than this are served but not cached — a sanity bound so a
/// pathological response can't bloat Redis/memory.
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// In-process arm: version counters + a bounded body map. `Mutex<HashMap>` over
/// anything fancier — the maps are small and the critical sections are a lookup
/// (the same idiom as the image proxy's negative cache).
#[derive(Default)]
struct Memory {
    versions: Mutex<HashMap<String, i64>>,
    bodies: Mutex<HashMap<String, (Instant, Vec<u8>)>>,
}

enum Backend {
    Redis(redis::aio::ConnectionManager),
    Memory(Memory),
}

/// The analytics response cache handle held in [`crate::state::AppState`]. All
/// methods are infallible at the call site: a degraded backend reads as a miss
/// and writes as a no-op (logged at debug), never an error.
pub struct AnalyticsCache {
    backend: Backend,
}

impl AnalyticsCache {
    /// Redis-backed when a shared connection was established at boot, else
    /// in-process (see the module docs for what each arm guarantees).
    pub fn new(redis: Option<redis::aio::ConnectionManager>) -> Self {
        let backend = match redis {
            Some(conn) => Backend::Redis(conn),
            None => Backend::Memory(Memory::default()),
        };
        Self { backend }
    }

    fn holdings_key(user_id: i32, game: &str) -> String {
        format!("an:holdver:{user_id}:{game}")
    }

    fn prices_key(game: &str) -> String {
        format!("an:pricever:{game}")
    }

    /// Compose the body key for one analytics request, embedding both version
    /// counters and the current UTC date. `None` means the backend is degraded —
    /// the caller must skip the cache (both get and put) for this request.
    pub async fn body_key(
        &self,
        user_id: i32,
        game: &str,
        endpoint: &str,
        params: &str,
    ) -> Option<String> {
        let (holdings, prices) = self.versions(user_id, game).await?;
        let day = Utc::now().date_naive();
        Some(format!(
            "an:body:{user_id}:{game}:{endpoint}:{params}:{holdings}:{prices}:{day}"
        ))
    }

    /// Both version counters for `(user, game)`, or `None` when the backend is
    /// degraded (never default a failed read — see the module docs).
    async fn versions(&self, user_id: i32, game: &str) -> Option<(i64, i64)> {
        match &self.backend {
            Backend::Redis(conn) => {
                let mut conn = conn.clone();
                let result: Result<(Option<i64>, Option<i64>), _> = redis::cmd("MGET")
                    .arg(Self::holdings_key(user_id, game))
                    .arg(Self::prices_key(game))
                    .query_async(&mut conn)
                    .await;
                match result {
                    Ok((holdings, prices)) => {
                        Some((holdings.unwrap_or(0), prices.unwrap_or(0)))
                    }
                    Err(err) => {
                        tracing::debug!(error = %err, "analytics cache version read failed; bypassing");
                        None
                    }
                }
            }
            Backend::Memory(memory) => {
                let versions = memory.versions.lock().expect("analytics versions mutex");
                let holdings = versions
                    .get(&Self::holdings_key(user_id, game))
                    .copied()
                    .unwrap_or(0);
                let prices = versions.get(&Self::prices_key(game)).copied().unwrap_or(0);
                Some((holdings, prices))
            }
        }
    }

    /// Record that `(user, game)`'s holdings changed — every cached analytics body
    /// for them orphans immediately. Best-effort: called after successful writes.
    pub async fn bump_holdings(&self, user_id: i32, game: &str) {
        self.bump(Self::holdings_key(user_id, game)).await;
    }

    /// Record that the game's price/history data changed (sync tick, backfill,
    /// foil enrichment) — every user's cached analytics bodies orphan.
    pub async fn bump_prices(&self, game: &str) {
        self.bump(Self::prices_key(game)).await;
    }

    async fn bump(&self, key: String) {
        match &self.backend {
            Backend::Redis(conn) => {
                let mut conn = conn.clone();
                let result: Result<((), ()), _> = redis::pipe()
                    .cmd("INCR")
                    .arg(&key)
                    .cmd("EXPIRE")
                    .arg(&key)
                    .arg(VERSION_TTL_SECS)
                    .query_async(&mut conn)
                    .await;
                if let Err(err) = result {
                    // The version didn't advance, so a cached body may now be stale:
                    // worth a warn (unlike a failed read, which merely bypasses).
                    // Bounded by BODY_TTL either way.
                    tracing::warn!(error = %err, "analytics cache version bump failed");
                }
            }
            Backend::Memory(memory) => {
                let mut versions = memory.versions.lock().expect("analytics versions mutex");
                *versions.entry(key).or_insert(0) += 1;
            }
        }
    }

    /// A cached body for `key`, if present and fresh.
    pub async fn get_body(&self, key: &str) -> Option<Vec<u8>> {
        match &self.backend {
            Backend::Redis(conn) => {
                let mut conn = conn.clone();
                let result: Result<Option<Vec<u8>>, _> =
                    redis::cmd("GET").arg(key).query_async(&mut conn).await;
                match result {
                    Ok(body) => body,
                    Err(err) => {
                        tracing::debug!(error = %err, "analytics cache body read failed; bypassing");
                        None
                    }
                }
            }
            Backend::Memory(memory) => {
                let bodies = memory.bodies.lock().expect("analytics bodies mutex");
                bodies.get(key).and_then(|(stored, body)| {
                    (stored.elapsed() < BODY_TTL).then(|| body.clone())
                })
            }
        }
    }

    /// Store a response body under `key` (TTL-bounded; oversized bodies skipped).
    pub async fn put_body(&self, key: &str, body: &[u8]) {
        if body.len() > MAX_BODY_BYTES {
            return;
        }
        match &self.backend {
            Backend::Redis(conn) => {
                let mut conn = conn.clone();
                let result: Result<(), _> = redis::cmd("SET")
                    .arg(key)
                    .arg(body)
                    .arg("EX")
                    .arg(BODY_TTL.as_secs())
                    .query_async(&mut conn)
                    .await;
                if let Err(err) = result {
                    tracing::debug!(error = %err, "analytics cache body write failed; skipped");
                }
            }
            Backend::Memory(memory) => {
                let mut bodies = memory.bodies.lock().expect("analytics bodies mutex");
                // Bounded: at cap, evict the oldest-stored entry (an O(n) scan over a
                // small map beats real LRU bookkeeping here). Expired entries win the
                // scan naturally since they're the oldest.
                if bodies.len() >= MEMORY_BODY_CAP && !bodies.contains_key(key) {
                    if let Some(oldest) = bodies
                        .iter()
                        .min_by_key(|(_, (stored, _))| *stored)
                        .map(|(k, _)| k.clone())
                    {
                        bodies.remove(&oldest);
                    }
                }
                bodies.insert(key.to_string(), (Instant::now(), body.to_vec()));
            }
        }
    }
}

/// Build the `application/json` response for an (already-serialized) analytics
/// body — the shape both a cache hit and a fresh computation return.
pub fn json_body_response(body: Vec<u8>) -> Response {
    (
        [(header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_cache() -> AnalyticsCache {
        AnalyticsCache::new(None)
    }

    #[tokio::test]
    async fn body_key_embeds_both_versions_and_changes_on_bump() {
        let cache = memory_cache();

        let initial = cache.body_key(7, "mtg", "movers", "").await.expect("key");

        // A holdings bump for the user changes their key...
        cache.bump_holdings(7, "mtg").await;
        let after_holdings = cache.body_key(7, "mtg", "movers", "").await.expect("key");
        assert_ne!(initial, after_holdings);

        // ...a price bump changes it again...
        cache.bump_prices("mtg").await;
        let after_prices = cache.body_key(7, "mtg", "movers", "").await.expect("key");
        assert_ne!(after_holdings, after_prices);

        // ...and another user's bump does not touch this user's key.
        cache.bump_holdings(8, "mtg").await;
        let after_other = cache.body_key(7, "mtg", "movers", "").await.expect("key");
        assert_eq!(after_prices, after_other);

        // Params and endpoint are part of the key, so ranges can't cross-serve.
        let full = cache
            .body_key(7, "mtg", "value-history", "full")
            .await
            .expect("key");
        let windowed = cache
            .body_key(7, "mtg", "value-history", "7d")
            .await
            .expect("key");
        assert_ne!(full, windowed);
    }

    #[tokio::test]
    async fn bodies_round_trip_and_orphan_on_version_bump() {
        let cache = memory_cache();

        let key = cache.body_key(1, "mtg", "movers", "").await.expect("key");
        assert!(cache.get_body(&key).await.is_none());

        cache.put_body(&key, b"{\"cached\":true}").await;
        assert_eq!(
            cache.get_body(&key).await.as_deref(),
            Some(b"{\"cached\":true}".as_slice())
        );

        // After a holdings mutation the key changes, so the old body is unreachable.
        cache.bump_holdings(1, "mtg").await;
        let new_key = cache.body_key(1, "mtg", "movers", "").await.expect("key");
        assert_ne!(key, new_key);
        assert!(cache.get_body(&new_key).await.is_none());
    }

    #[tokio::test]
    async fn memory_body_map_is_bounded() {
        let cache = memory_cache();
        for i in 0..(MEMORY_BODY_CAP + 10) {
            cache.put_body(&format!("an:body:test:{i}"), b"x").await;
        }
        let Backend::Memory(memory) = &cache.backend else {
            panic!("memory backend expected");
        };
        assert!(memory.bodies.lock().unwrap().len() <= MEMORY_BODY_CAP);
    }

    #[tokio::test]
    async fn oversized_bodies_are_not_cached() {
        let cache = memory_cache();
        let key = "an:body:test:big";
        cache.put_body(key, &vec![0u8; MAX_BODY_BYTES + 1]).await;
        assert!(cache.get_body(key).await.is_none());
    }

    // The Redis arm mirrors the memory arm through the same public API. `#[ignore]`d
    // so the default `cargo test` (no services) skips it; run against a live Redis
    // with `TCGLENSE_TEST_REDIS_URL=redis://localhost:6379 cargo test -- --ignored`.
    // Random per-run ids keep keys collision-free on a shared Redis; everything
    // self-expires.
    #[tokio::test]
    #[ignore = "requires a live Redis; set TCGLENSE_TEST_REDIS_URL, run with --ignored"]
    async fn redis_arm_versions_and_bodies_round_trip() {
        let Ok(url) = std::env::var("TCGLENSE_TEST_REDIS_URL") else {
            return;
        };
        let conn = crate::ratelimit::connect_redis(&url).await.expect("connect");
        let cache = AnalyticsCache::new(Some(conn));

        // A user id from the nanosecond clock so runs never collide.
        let uid = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .subsec_nanos() as i32)
            .abs();

        let key = cache.body_key(uid, "mtg", "movers", "").await.expect("key");
        assert!(cache.get_body(&key).await.is_none());
        cache.put_body(&key, b"{}").await;
        assert_eq!(cache.get_body(&key).await.as_deref(), Some(b"{}".as_slice()));

        cache.bump_holdings(uid, "mtg").await;
        let bumped = cache.body_key(uid, "mtg", "movers", "").await.expect("key");
        assert_ne!(key, bumped);
        assert!(cache.get_body(&bumped).await.is_none());
    }
}

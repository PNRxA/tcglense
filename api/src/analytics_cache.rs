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
//!
//! **Single-flight contract** ([`AnalyticsCache::get_or_compute`]): the store
//! above only *records* a finished body — nothing stops two requests that miss
//! the cache in the same instant from both running the (multi-second) computation,
//! which is exactly what the prod logs showed (the movers query logged twice,
//! concurrently). `get_or_compute` coalesces concurrent misses for one body key so
//! the work runs **once per process**: the first caller to miss becomes the
//! *leader* and computes; every other caller that misses the same key while the
//! leader is in flight *follows*, parking on a [`tokio::sync::watch`] channel until
//! the leader publishes the body, then returns that same bytes. This is
//! process-local by design — it rides no Redis state, so on a multi-replica deploy
//! each replica merely computes at most once (acceptable: prod is single-instance,
//! and the store still dedupes across replicas once anyone finishes). A `None` key
//! (degraded backend) skips coalescing entirely and just computes, matching the
//! pre-single-flight behaviour.
//!
//! **Cancellation story:** axum drops a request future when its client disconnects,
//! so a leader can vanish mid-compute at any await point. An `InflightGuard` removes
//! the in-flight map entry on *every* leader exit — success, error, **and drop** —
//! so an abandoned leader never strands its followers on a wake that can't come:
//! dropping the guard also drops the leader's `watch::Sender`, which closes the
//! channel and wakes every follower with an error; they loop, and one of them
//! becomes the next leader. The map therefore only ever holds genuinely in-flight
//! entries and stays naturally bounded (one entry per distinct in-flight key).

use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::header;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use tokio::sync::watch;

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

/// A leader's channel: `None` until it publishes the computed body. Followers
/// subscribe to it; the leader `send`s `Some(body)` on success. A dropped sender
/// (leader failed or was cancelled) closes the channel and wakes followers with an
/// error, which they treat as "retry".
type InflightMap = Mutex<HashMap<String, watch::Sender<Option<Vec<u8>>>>>;

/// The analytics response cache handle held in [`crate::state::AppState`]. All
/// methods are infallible at the call site: a degraded backend reads as a miss
/// and writes as a no-op (logged at debug), never an error.
pub struct AnalyticsCache {
    backend: Backend,
    /// Process-local single-flight registry: one entry per body key that some
    /// leader is currently computing (see the module docs). Kept out of the
    /// [`Backend`] because coalescing is intentionally *not* shared across replicas.
    inflight: InflightMap,
}

/// RAII guard held by a single-flight *leader*. Its `Drop` removes the in-flight
/// map entry on every exit path — success, error, or a cancellation-drop of the
/// request future — which is what keeps followers from waiting on a leader that
/// will never publish (see the module docs' cancellation story).
struct InflightGuard<'a> {
    inflight: &'a InflightMap,
    key: &'a str,
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        self.inflight
            .lock()
            .expect("analytics inflight mutex")
            .remove(self.key);
    }
}

impl AnalyticsCache {
    /// Redis-backed when a shared connection was established at boot, else
    /// in-process (see the module docs for what each arm guarantees).
    pub fn new(redis: Option<redis::aio::ConnectionManager>) -> Self {
        let backend = match redis {
            Some(conn) => Backend::Redis(conn),
            None => Backend::Memory(Memory::default()),
        };
        Self {
            backend,
            inflight: Mutex::new(HashMap::new()),
        }
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
                    Ok((holdings, prices)) => Some((holdings.unwrap_or(0), prices.unwrap_or(0))),
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
                bodies
                    .get(key)
                    .and_then(|(stored, body)| (stored.elapsed() < BODY_TTL).then(|| body.clone()))
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

    /// Return a cached body for `key`, or compute it — coalescing concurrent
    /// misses so `compute` runs at most once per process for a given key while any
    /// call is in flight (see the module docs' single-flight and cancellation
    /// contracts). `compute` is `Fn` (not `FnOnce`) because a leader whose compute
    /// fails, or whose followers are woken by a cancelled leader, re-runs it.
    ///
    /// `key` is `None` when the backend is degraded ([`Self::body_key`] returned
    /// `None`): there is no stable key to coalesce or cache under, so this just
    /// runs `compute` every time — exactly the pre-single-flight behaviour.
    pub async fn get_or_compute<E, F, Fut>(
        &self,
        key: Option<String>,
        compute: F,
    ) -> Result<Vec<u8>, E>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<Vec<u8>, E>>,
    {
        let Some(key) = key else {
            return compute().await;
        };

        // Which side of the single-flight this iteration takes. Decided under the
        // map lock and carried *out* of the locked scope so nothing is held across
        // an await.
        enum Role {
            /// This caller owns the computation; carries the publish channel.
            Leader(watch::Sender<Option<Vec<u8>>>),
            /// Another caller is computing; wait on its channel.
            Follower(watch::Receiver<Option<Vec<u8>>>),
        }

        loop {
            // A prior leader may have finished (or a later one published) since the
            // last time we looked — always re-check the store before deciding a role.
            if let Some(body) = self.get_body(&key).await {
                return Ok(body);
            }

            let role = {
                let mut inflight = self.inflight.lock().expect("analytics inflight mutex");
                match inflight.get(&key) {
                    // Subscribe *before* releasing the lock so we can't miss the
                    // leader's publish in the gap between lookup and subscribe.
                    Some(sender) => Role::Follower(sender.subscribe()),
                    None => {
                        let (tx, _rx) = watch::channel(None);
                        inflight.insert(key.clone(), tx.clone());
                        Role::Leader(tx)
                    }
                }
            };

            match role {
                Role::Follower(mut rx) => {
                    // `changed()` Ok + `Some(body)` — the leader published. Ok +
                    // `None` can't happen (leaders only ever send `Some`). `Err` —
                    // the sender dropped: the leader failed or was cancelled, so no
                    // body is coming. In every non-body case, loop and try to become
                    // the leader ourselves.
                    if rx.changed().await.is_ok()
                        && let Some(body) = rx.borrow_and_update().clone()
                    {
                        return Ok(body);
                    }
                    continue;
                }
                Role::Leader(tx) => {
                    // Removes the map entry on every exit below — and, crucially, on
                    // a cancellation-drop of this future between here and its scope
                    // end (dropping the guard also drops `tx`, closing the channel
                    // and waking followers to retry).
                    let guard = InflightGuard {
                        inflight: &self.inflight,
                        key: &key,
                    };
                    return match compute().await {
                        Ok(body) => {
                            self.put_body(&key, &body).await;
                            // Publish to followers even if the `put_body` above
                            // failed (Redis error) or skipped an oversized body —
                            // they still get the result without recomputing. `send`
                            // only errs when no receivers remain; ignore that.
                            let _ = tx.send(Some(body.clone()));
                            drop(guard);
                            Ok(body)
                        }
                        Err(err) => {
                            // Drop the guard (remove the entry) and let `tx` drop at
                            // scope end: followers wake with a closed channel, loop,
                            // and one becomes the next leader. Failures aren't cached.
                            drop(guard);
                            Err(err)
                        }
                    };
                }
            }
        }
    }
}

/// Build the `application/json` response for an (already-serialized) analytics
/// body — the shape both a cache hit and a fresh computation return.
pub fn json_body_response(body: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], body).into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::sync::Notify;

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

    // ---- single-flight (`get_or_compute`) ----
    //
    // All of these run on the default `#[tokio::test]` current-thread runtime, so
    // scheduling is deterministic: `Notify` handshakes gate the leader in place
    // while followers subscribe, and a single `yield_now` drains the ready queue —
    // no sleeps, no wall-clock races.

    #[tokio::test]
    async fn concurrent_misses_compute_once() {
        let cache = Arc::new(memory_cache());
        let key = cache.body_key(1, "mtg", "movers", "").await.expect("key");

        let calls = Arc::new(AtomicUsize::new(0));
        // Holds the leader inside `compute` until every follower has subscribed, so
        // the coalescing path is actually exercised and not won by a fast leader.
        let gate = Arc::new(Notify::new());
        let entered = Arc::new(Notify::new());

        const N: usize = 8;

        // The leader enters `compute`, announces itself, then parks until released.
        let leader = {
            let (cache, key, calls, gate, entered) = (
                cache.clone(),
                key.clone(),
                calls.clone(),
                gate.clone(),
                entered.clone(),
            );
            tokio::spawn(async move {
                cache
                    .get_or_compute(Some(key), move || {
                        let (calls, gate, entered) = (calls.clone(), gate.clone(), entered.clone());
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            entered.notify_one();
                            gate.notified().await;
                            Ok::<Vec<u8>, ()>(b"body".to_vec())
                        }
                    })
                    .await
            })
        };

        // Once the leader is inside `compute` its map entry exists, so everyone
        // spawned now can only ever follow it — a second compute is impossible.
        entered.notified().await;

        let mut followers = Vec::new();
        for _ in 0..(N - 1) {
            let (cache, key, calls) = (cache.clone(), key.clone(), calls.clone());
            followers.push(tokio::spawn(async move {
                cache
                    .get_or_compute(Some(key), move || {
                        // Never expected to run; if it does, `calls` catches it.
                        let calls = calls.clone();
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            Ok::<Vec<u8>, ()>(b"body".to_vec())
                        }
                    })
                    .await
            }));
        }

        // Let the followers reach their `changed()` park points, then release.
        tokio::task::yield_now().await;
        gate.notify_one();

        assert_eq!(leader.await.unwrap(), Ok(b"body".to_vec()));
        for f in followers {
            assert_eq!(f.await.unwrap(), Ok(b"body".to_vec()));
        }
        // Exactly one caller computed; the rest coalesced onto its result.
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn leader_error_lets_a_follower_recompute() {
        let cache = Arc::new(memory_cache());
        let key = cache.body_key(2, "mtg", "movers", "").await.expect("key");

        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Notify::new());
        let entered = Arc::new(Notify::new());

        const N: usize = 6;

        // The first invocation (initial leader) parks then fails; any later
        // invocation (a follower that took over) succeeds immediately.
        let mut handles = Vec::new();
        for _ in 0..N {
            let (cache, key, calls, gate, entered) = (
                cache.clone(),
                key.clone(),
                calls.clone(),
                gate.clone(),
                entered.clone(),
            );
            handles.push(tokio::spawn(async move {
                cache
                    .get_or_compute(Some(key), move || {
                        let (calls, gate, entered) = (calls.clone(), gate.clone(), entered.clone());
                        async move {
                            let nth = calls.fetch_add(1, Ordering::SeqCst);
                            if nth == 0 {
                                entered.notify_one();
                                gate.notified().await;
                                Err::<Vec<u8>, ()>(())
                            } else {
                                Ok::<Vec<u8>, ()>(b"recovered".to_vec())
                            }
                        }
                    })
                    .await
            }));
        }

        // Leader is in-flight and followers have subscribed; release it to fail.
        entered.notified().await;
        gate.notify_one();

        let mut errors = 0;
        let mut recovered = 0;
        for h in handles {
            match h.await.unwrap() {
                Err(()) => errors += 1,
                Ok(body) => {
                    assert_eq!(body, b"recovered".to_vec());
                    recovered += 1;
                }
            }
        }
        // Exactly the initial leader failed; everyone else got the recomputed body,
        // and no caller deadlocked. Two computes: the failure plus one recovery.
        assert_eq!(errors, 1);
        assert_eq!(recovered, N - 1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cancelled_leader_hands_off_to_a_follower() {
        let cache = Arc::new(memory_cache());
        let key = cache.body_key(3, "mtg", "movers", "").await.expect("key");

        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Notify::new());
        let entered = Arc::new(Notify::new());

        const N: usize = 5;

        // The leader parks forever inside `compute` (its gate is never released) and
        // is aborted mid-flight.
        let leader = {
            let (cache, key, calls, gate, entered) = (
                cache.clone(),
                key.clone(),
                calls.clone(),
                gate.clone(),
                entered.clone(),
            );
            tokio::spawn(async move {
                cache
                    .get_or_compute(Some(key), move || {
                        let (calls, gate, entered) = (calls.clone(), gate.clone(), entered.clone());
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            entered.notify_one();
                            gate.notified().await; // never released; aborted here
                            Ok::<Vec<u8>, ()>(b"unreachable".to_vec())
                        }
                    })
                    .await
            })
        };

        entered.notified().await; // leader is in-flight, parked in compute

        let mut followers = Vec::new();
        for _ in 0..(N - 1) {
            let (cache, key, calls) = (cache.clone(), key.clone(), calls.clone());
            followers.push(tokio::spawn(async move {
                cache
                    .get_or_compute(Some(key), move || {
                        let calls = calls.clone();
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            Ok::<Vec<u8>, ()>(b"took-over".to_vec())
                        }
                    })
                    .await
            }));
        }

        // Let followers subscribe and park on `changed`, then cancel the leader.
        tokio::task::yield_now().await;
        leader.abort();

        // Aborting drops the leader's future: the `InflightGuard` removes the map
        // entry and the leader's `Sender` drops, waking followers to retry. One of
        // them takes over and completes — no follower is stranded.
        for f in followers {
            assert_eq!(f.await.unwrap(), Ok(b"took-over".to_vec()));
        }
        assert!(leader.await.unwrap_err().is_cancelled());
        // The aborted leader incremented once before parking; one follower recomputed.
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn none_key_always_computes_and_never_caches() {
        let cache = memory_cache();
        let calls = AtomicUsize::new(0);

        for _ in 0..3 {
            let body = cache
                .get_or_compute::<(), _, _>(None, || async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(b"x".to_vec())
                })
                .await
                .unwrap();
            assert_eq!(body, b"x".to_vec());
        }
        // No coalescing and no caching under a `None` key: every call computed...
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        // ...and the single-flight map was never touched.
        assert!(cache.inflight.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn distinct_keys_do_not_serialize() {
        let cache = Arc::new(memory_cache());
        let key_a = cache.body_key(4, "mtg", "movers", "").await.expect("key");
        let key_b = cache.body_key(5, "mtg", "movers", "").await.expect("key");
        assert_ne!(key_a, key_b);

        // A single shared gate holds both leaders; both must reach it before either
        // is released, proving neither key's computation blocks the other's.
        let gate = Arc::new(Notify::new());
        let entered_a = Arc::new(Notify::new());
        let entered_b = Arc::new(Notify::new());

        let spawn_leader = |key: String, entered: Arc<Notify>, marker: &'static [u8]| {
            let (cache, gate) = (cache.clone(), gate.clone());
            tokio::spawn(async move {
                cache
                    .get_or_compute(Some(key), move || {
                        let (gate, entered) = (gate.clone(), entered.clone());
                        async move {
                            entered.notify_one();
                            gate.notified().await;
                            Ok::<Vec<u8>, ()>(marker.to_vec())
                        }
                    })
                    .await
            })
        };

        let task_a = spawn_leader(key_a, entered_a.clone(), b"a");
        let task_b = spawn_leader(key_b, entered_b.clone(), b"b");

        // If distinct keys serialized, only one could be in-flight and the other
        // `entered` would never fire — this would hang.
        entered_a.notified().await;
        entered_b.notified().await;

        gate.notify_waiters(); // both leaders are parked; wake them together

        assert_eq!(task_a.await.unwrap(), Ok(b"a".to_vec()));
        assert_eq!(task_b.await.unwrap(), Ok(b"b".to_vec()));
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
        let conn = crate::ratelimit::connect_redis(&url)
            .await
            .expect("connect");
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
        assert_eq!(
            cache.get_body(&key).await.as_deref(),
            Some(b"{}".as_slice())
        );

        cache.bump_holdings(uid, "mtg").await;
        let bumped = cache.body_key(uid, "mtg", "movers", "").await.expect("key");
        assert_ne!(key, bumped);
        assert!(cache.get_body(&bumped).await.is_none());
    }
}

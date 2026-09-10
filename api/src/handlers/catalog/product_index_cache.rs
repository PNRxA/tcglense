//! Process-local memo for a sealed product's card index, with **single-flight**
//! coalescing.
//!
//! [`super::products::build_product_card_index`] folds a product's whole membership set
//! into the orderings and section buckets both `/products/{id}/cards` and
//! `/products/{id}/cards/sections` page out of. It is the same fold for both — the same
//! rows, the same sort — and the SPA fires them **together** on a cold product page, so a
//! single page load computed it twice, concurrently, at every DB round-trip it costs
//! (production logs show the identical 1,121-row membership read and the identical 900-id
//! sort-key read landing within 5 ms of each other, each taking seconds under load).
//!
//! Caching on completion alone would not fix that: both requests miss an empty map and both
//! compute. The coalescing is the load-bearing half — the second arrival waits on the
//! first's result instead of starting its own. The leader/follower loop below is
//! [`crate::analytics_cache::AnalyticsCache::get_or_compute`]'s, ported to hold an
//! `Arc<ProductCardIndex>` instead of a serialized body, including its `InflightGuard`:
//! dropping it on *every* exit path — error, or a cancellation-drop when the client hangs
//! up — is what stops followers waiting on a leader that will never publish.
//!
//! **Invalidation is a TTL**, deliberately, and that is defensible only because of the
//! contract these two responses already ship under: they sit in the public catalog cache
//! group, which lets any CDN serve them `s-maxage=3600` plus a day of
//! `stale-while-revalidate`. A [`TTL`] an order of magnitude shorter than that cannot make
//! a reader see anything staler than the HTTP layer already may. The alternatives were
//! considered and rejected: `products.updated_at` moves on every price tick and does *not*
//! move when the sealed sync rebuilds `sealed_contents`, so it is wrong in both directions;
//! an `ingest_state` probe would add a query to the request it is meant to save; and a
//! sync-bumped generation counter would need `AppState` threaded into
//! `catalog::refresh_all`, which takes only a `&DatabaseConnection`.
//!
//! Held per-[`AppState`] rather than in a `static`: the test suites build many independent
//! `AppState`s over independent in-memory databases in one process, and their product-id
//! fixtures collide, so a process-global would serve one test's index to another's
//! assertions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::watch;

use super::products::ProductCardIndex;

/// How long a folded index may be served before it is recomputed. Far inside the hour of
/// CDN staleness the same two responses already permit (see the module docs), and long
/// enough to cover a reader paging through a booster's card list.
const TTL: Duration = Duration::from_secs(300);

/// How many products' indexes to hold. An index is much larger than an analytics body — a
/// big collector booster carries a `(rank, membership, foil)` entry per card in *two* maps
/// plus an ordering per view, on the order of a megabyte for an 8,000-card pool — so this
/// is small on purpose: it exists to coalesce one page's fan-out and to carry a reader
/// across their own paging, not to hold the catalog.
const CAPACITY: usize = 32;

/// `(game, product_id)` — a product id is only unique within its game.
type Key = (String, i32);

type Inflight = Mutex<HashMap<Key, watch::Sender<Option<Arc<ProductCardIndex>>>>>;

/// The memo handle held in [`crate::state::AppState`].
#[derive(Default)]
pub struct ProductCardIndexCache {
    entries: Mutex<HashMap<Key, (Instant, Arc<ProductCardIndex>)>>,
    /// Process-local single-flight registry: one entry per key some leader is computing.
    inflight: Inflight,
}

/// RAII guard held by a single-flight *leader*. Its `Drop` removes the in-flight entry on
/// every exit path — success, error, or a cancellation-drop of the request future — which
/// is what keeps followers from waiting on a leader that will never publish.
struct InflightGuard<'a> {
    inflight: &'a Inflight,
    key: &'a Key,
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        self.inflight
            .lock()
            .expect("product index inflight mutex")
            .remove(self.key);
    }
}

impl ProductCardIndexCache {
    /// Return the memoized index for `(game, product_id)`, computing it at most once across
    /// concurrent callers. A `compute` error is returned to every waiter and never cached.
    pub(crate) async fn get_or_compute<E, F, Fut>(
        &self,
        game: &str,
        product_id: i32,
        compute: F,
    ) -> Result<Arc<ProductCardIndex>, E>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<ProductCardIndex, E>>,
    {
        let key: Key = (game.to_string(), product_id);

        // Which side of the single-flight this iteration takes. Decided under the map lock
        // and carried *out* of the locked scope, so no `std::sync` guard is held across an
        // await (the handler's future must stay `Send`).
        enum Role {
            Leader(watch::Sender<Option<Arc<ProductCardIndex>>>),
            Follower(watch::Receiver<Option<Arc<ProductCardIndex>>>),
        }

        loop {
            // A prior leader may have published since we last looked — always re-check the
            // store before deciding a role.
            if let Some(index) = self.get(&key) {
                return Ok(index);
            }

            let role = {
                let mut inflight = self.inflight.lock().expect("product index inflight mutex");
                match inflight.get(&key) {
                    // Subscribe *before* releasing the lock so the leader's publish can't
                    // land in the gap between lookup and subscribe.
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
                    // Ok + `Some` — the leader published. `Err` — the sender dropped, so
                    // the leader failed or was cancelled and no index is coming. Either
                    // way that isn't a body: loop and try to lead ourselves.
                    if rx.changed().await.is_ok()
                        && let Some(index) = rx.borrow_and_update().clone()
                    {
                        return Ok(index);
                    }
                    continue;
                }
                Role::Leader(tx) => {
                    let guard = InflightGuard {
                        inflight: &self.inflight,
                        key: &key,
                    };
                    return match compute().await {
                        Ok(index) => {
                            let index = Arc::new(index);
                            self.put(key.clone(), Arc::clone(&index));
                            // `send` only errs when no receivers remain; ignore that.
                            let _ = tx.send(Some(Arc::clone(&index)));
                            drop(guard);
                            Ok(index)
                        }
                        Err(err) => {
                            // Dropping the guard removes the entry and lets `tx` drop at
                            // scope end: followers wake on a closed channel, loop, and one
                            // becomes the next leader. Failures aren't cached.
                            drop(guard);
                            Err(err)
                        }
                    };
                }
            }
        }
    }

    /// A live entry for `key`, if one is present and inside [`TTL`]. An expired entry is
    /// dropped on the way past rather than left to the capacity sweep.
    fn get(&self, key: &Key) -> Option<Arc<ProductCardIndex>> {
        let mut entries = self.entries.lock().expect("product index cache mutex");
        match entries.get(key) {
            Some((stored, index)) if stored.elapsed() < TTL => Some(Arc::clone(index)),
            Some(_) => {
                entries.remove(key);
                None
            }
            None => None,
        }
    }

    /// Store an index, evicting expired entries first and then the oldest until the map is
    /// back inside [`CAPACITY`].
    fn put(&self, key: Key, index: Arc<ProductCardIndex>) {
        let mut entries = self.entries.lock().expect("product index cache mutex");
        entries.retain(|_, (stored, _)| stored.elapsed() < TTL);
        while entries.len() >= CAPACITY {
            let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, (stored, _))| *stored)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            entries.remove(&oldest);
        }
        entries.insert(key, (Instant::now(), index));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::sync::Notify;

    use super::*;

    /// The whole point of the module: callers racing for one product — the SPA's `/cards`
    /// + `/cards/sections` pair — must compute the index once, not twice. A plain memo
    /// would fail this, because both miss the empty map before either has published.
    ///
    /// Deterministic on the current-thread runtime, the idiom
    /// `analytics_cache::tests::concurrent_misses_compute_once` sets: the leader announces
    /// itself from inside `compute` and parks, so every follower spawned afterwards can
    /// only ever find its in-flight entry; one `yield_now` drains them to their park
    /// points before the gate opens.
    #[tokio::test]
    async fn concurrent_misses_compute_once() {
        let cache = Arc::new(ProductCardIndexCache::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Notify::new());
        let entered = Arc::new(Notify::new());

        const N: usize = 4;

        let leader = {
            let (cache, calls, gate, entered) =
                (cache.clone(), calls.clone(), gate.clone(), entered.clone());
            tokio::spawn(async move {
                cache
                    .get_or_compute("mtg", 100, move || {
                        let (calls, gate, entered) = (calls.clone(), gate.clone(), entered.clone());
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            entered.notify_one();
                            gate.notified().await;
                            Ok::<_, ()>(ProductCardIndex::empty())
                        }
                    })
                    .await
            })
        };

        // Once the leader is inside `compute` its map entry exists, so everyone spawned
        // now can only ever follow it — a second fold is impossible.
        entered.notified().await;

        let mut followers = Vec::new();
        for _ in 0..(N - 1) {
            let (cache, calls) = (cache.clone(), calls.clone());
            followers.push(tokio::spawn(async move {
                cache
                    .get_or_compute("mtg", 100, move || {
                        // Never expected to run; if it does, `calls` catches it.
                        let calls = calls.clone();
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            Ok::<_, ()>(ProductCardIndex::empty())
                        }
                    })
                    .await
            }));
        }

        // Let the followers reach their `changed()` park points, then release.
        tokio::task::yield_now().await;
        gate.notify_one();

        let leader = leader.await.unwrap().expect("leader publishes");
        for f in followers {
            let followed = f
                .await
                .unwrap()
                .expect("follower receives the leader's index");
            assert!(
                Arc::ptr_eq(&leader, &followed),
                "a follower must receive the leader's very index, not an equal one"
            );
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "followers must coalesce onto the leader's fold"
        );
    }

    /// A hit inside the TTL is served from the memo; the compute closure runs once.
    #[tokio::test]
    async fn repeat_calls_hit_the_memo() {
        let cache = ProductCardIndexCache::default();
        let calls = AtomicUsize::new(0);
        let compute = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::convert::Infallible>(ProductCardIndex::empty())
        };

        for _ in 0..3 {
            cache.get_or_compute("mtg", 100, compute).await.unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// Keys are per game: a product id repeated across games is not one entry.
    #[tokio::test]
    async fn keys_are_scoped_by_game() {
        let cache = ProductCardIndexCache::default();
        let calls = AtomicUsize::new(0);
        let compute = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::convert::Infallible>(ProductCardIndex::empty())
        };

        cache.get_or_compute("mtg", 100, compute).await.unwrap();
        cache.get_or_compute("pkm", 100, compute).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// A failed computation is not cached, and leaves no in-flight entry behind for the
    /// next caller to wait on forever.
    #[tokio::test]
    async fn failures_are_not_cached() {
        let cache = ProductCardIndexCache::default();
        let calls = AtomicUsize::new(0);
        let failing = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err::<ProductCardIndex, _>("boom")
        };

        assert!(cache.get_or_compute("mtg", 100, failing).await.is_err());
        assert!(cache.get_or_compute("mtg", 100, failing).await.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2, "a failure must be retried");
        assert!(
            cache.inflight.lock().expect("inflight mutex").is_empty(),
            "the leader's guard must clear the in-flight entry on the error path"
        );
    }

    /// The capacity bound holds: an overflowing insert evicts rather than growing.
    #[tokio::test]
    async fn capacity_is_bounded() {
        let cache = ProductCardIndexCache::default();
        for id in 0..(CAPACITY as i32 + 8) {
            cache
                .get_or_compute("mtg", id, || async {
                    Ok::<_, std::convert::Infallible>(ProductCardIndex::empty())
                })
                .await
                .unwrap();
        }
        assert!(
            cache.entries.lock().expect("entries mutex").len() <= CAPACITY,
            "the memo must not grow without bound"
        );
    }
}

//! Lazy, on-disk cache for card images.
//!
//! Card art is fetched from the upstream provider (Scryfall) the first time it
//! is requested, written to a directory, and served from disk thereafter — so
//! we never bulk-download the provider's entire image catalogue, yet every image
//! shown is persisted locally. Generic over `game`, so any TCG reuses it.
//!
//! In **CDN mode** (`cdn_mode`) the on-disk step is skipped entirely: the image
//! is fetched from upstream and streamed straight back without being persisted.
//! It is meant for deployments that sit behind a CDN which caches the immutable
//! image responses, so the origin needs no writable image directory and is only
//! hit on a CDN cache miss (see `CDN_MODE` in [`crate::config`]).
//!
//! **Negative cache:** not every id has an image upstream — the TCGplayer product
//! CDN, for one, `403`s the URL for a product with no art. Without memory the proxy
//! would re-fetch (and re-log, and 500) that dead URL on every single page view. So a
//! definitive upstream "not available" (a 4xx that isn't a rate-limit / timeout) is
//! remembered in-process for [`NEGATIVE_TTL`]: within that window the miss is served
//! straight from memory as [`ImageError::Unavailable`] without touching the provider,
//! and after it the proxy tries once more (the image may have appeared).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use reqwest::{Client, StatusCode};
use tokio::sync::Semaphore;

/// Monotonic counter giving each in-flight download a unique temp filename.
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// How long a "this asset isn't available upstream" result is remembered before the
/// proxy will ask the provider again — the negative-cache window (see the module docs).
/// Six hours: long enough that a dead URL isn't re-fetched on every view, short enough
/// that an image added later still shows up the same day, and aligned with the 6h
/// maintenance/sync cadence.
const NEGATIVE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// Soft cap on distinct remembered failures. Reaching it triggers a lazy prune of
/// expired entries before the next insert, so scraping a whole catalog of imageless
/// ids can't grow the negative cache without bound.
const NEGATIVE_CACHE_CAP: usize = 8192;

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    /// The provider answered that this asset isn't available (a definitive 4xx such as
    /// `403`/`404`/`410`). Distinct from [`Http`](ImageError::Http): it's the asset that's
    /// missing, not the request that failed, so the caller returns a `404` (not a `5xx`)
    /// and it feeds the negative cache. Carries the upstream status for logging.
    #[error("image unavailable upstream: HTTP {0}")]
    Unavailable(StatusCode),
    #[error("image fetch failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("image cache io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Bytes plus the MIME type to serve them with.
pub struct CachedImage {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
}

/// On-disk image cache. Files live at `<base_dir>/<game>/<size>/<key>.<ext>`.
pub struct ImageCache {
    base_dir: PathBuf,
    client: Client,
    /// Bounds concurrent upstream fetches so a burst of first-time views stays
    /// polite to the provider and bounded in memory.
    fetch_limit: Semaphore,
    /// When true, bypass the on-disk cache: fetch from upstream and return the
    /// bytes without reading or writing `base_dir`. For deployments fronted by a
    /// CDN that caches the immutable image responses (see the module docs).
    cdn_mode: bool,
    /// How long a recorded failure is honoured before the asset is re-probed. Always
    /// [`NEGATIVE_TTL`] in production; a field (not the const directly) purely so tests can
    /// dial it down and exercise window expiry without waiting six hours.
    negative_ttl: Duration,
    /// Remembered "not available upstream" results, keyed by the asset's cache
    /// identity (`game/category/key.ext`) → the time it failed and the status seen.
    /// A fresh entry (within [`Self::negative_ttl`]) short-circuits the fetch. In-process
    /// only: a restart re-probes each dead URL once, which is fine.
    negative: Mutex<HashMap<String, (Instant, StatusCode)>>,
}

impl ImageCache {
    pub fn new(base_dir: PathBuf, client: Client, cdn_mode: bool) -> Self {
        Self::with_negative_ttl(base_dir, client, cdn_mode, NEGATIVE_TTL)
    }

    /// [`new`](Self::new) with an explicit negative-cache TTL — the seam the tests use to
    /// drive window expiry deterministically; production always passes [`NEGATIVE_TTL`].
    fn with_negative_ttl(
        base_dir: PathBuf,
        client: Client,
        cdn_mode: bool,
        negative_ttl: Duration,
    ) -> Self {
        Self {
            base_dir,
            client,
            fetch_limit: Semaphore::new(8),
            cdn_mode,
            negative_ttl,
            negative: Mutex::new(HashMap::new()),
        }
    }

    /// The recorded status if `key` has a **fresh** negative-cache entry (failed within
    /// [`Self::negative_ttl`]), else `None`. A stale entry is treated as absent — the asset
    /// is re-probed — and swept on the next [`record_negative`](Self::record_negative).
    fn negative_hit(&self, key: &str) -> Option<StatusCode> {
        let cache = self.negative.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .get(key)
            .filter(|(at, _)| at.elapsed() < self.negative_ttl)
            .map(|(_, status)| *status)
    }

    /// Remember that `key` was unavailable upstream so the next TTL window serves the miss
    /// from memory. Prunes expired entries first if the map has grown to
    /// [`NEGATIVE_CACHE_CAP`], keeping it bounded under a scrape of imageless ids.
    fn record_negative(&self, key: String, status: StatusCode) {
        let mut cache = self.negative.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= NEGATIVE_CACHE_CAP {
            cache.retain(|_, (at, _)| at.elapsed() < self.negative_ttl);
        }
        cache.insert(key, (Instant::now(), status));
    }

    /// Drop any remembered failure for `key` — called after a successful fetch so an
    /// asset that has since appeared upstream stops being reported as unavailable and
    /// its slot is reclaimed.
    fn clear_negative(&self, key: &str) {
        let mut cache = self.negative.lock().unwrap_or_else(|e| e.into_inner());
        cache.remove(key);
    }

    /// Return the cached card image for `(game, size, key)`, downloading it from
    /// `source_url` on a miss and persisting it first. `source_url` is resolved
    /// by the caller from trusted stored data, never from user input.
    pub async fn get(
        &self,
        game: &str,
        size: &str,
        key: &str,
        source_url: &str,
    ) -> Result<CachedImage, ImageError> {
        let (ext, content_type) = match size {
            "png" => ("png", "image/png"),
            _ => ("jpg", "image/jpeg"),
        };
        self.get_cached(game, size, key, ext, content_type, source_url)
            .await
    }

    /// Return a cached SVG (e.g. a set icon) under `<base>/<game>/<category>/`,
    /// downloading it from `source_url` on a miss. Same caching guarantees as
    /// [`get`]; the bytes are served as `image/svg+xml`.
    pub async fn get_svg(
        &self,
        game: &str,
        category: &str,
        key: &str,
        source_url: &str,
    ) -> Result<CachedImage, ImageError> {
        self.get_cached(game, category, key, "svg", "image/svg+xml", source_url)
            .await
    }

    /// Core cache-or-fetch: serve `<base>/<game>/<category>/<key>.<ext>` from
    /// disk, downloading + persisting it (temp file + atomic rename) on a miss.
    async fn get_cached(
        &self,
        game: &str,
        category: &str,
        key: &str,
        ext: &str,
        content_type: &'static str,
        source_url: &str,
    ) -> Result<CachedImage, ImageError> {
        // The asset's cache identity, also the negative-cache key (matches the disk
        // layout so it's unique per game/size/key/ext; used in CDN mode too).
        let neg_key = format!(
            "{}/{}/{}.{ext}",
            sanitize(game),
            sanitize(category),
            sanitize(key)
        );

        // CDN mode: never touch `base_dir`. A fronting CDN caches the immutable
        // response, so fetch straight from upstream and stream the bytes through.
        if self.cdn_mode {
            if let Some(status) = self.negative_hit(&neg_key) {
                return Err(ImageError::Unavailable(status));
            }
            let _permit = self.fetch_limit.acquire().await.ok();
            let bytes = self.fetch_tracked(&neg_key, source_url).await?;
            return Ok(CachedImage {
                bytes,
                content_type,
            });
        }

        // Sanitise every path segment defensively against traversal.
        let dir = self.base_dir.join(sanitize(game)).join(sanitize(category));
        let path = dir.join(format!("{}.{ext}", sanitize(key)));

        if let Ok(bytes) = tokio::fs::read(&path).await {
            return Ok(CachedImage {
                bytes,
                content_type,
            });
        }

        // A recent upstream "not available" short-circuits before we hit the provider
        // again — the whole point of the negative cache (see the module docs).
        if let Some(status) = self.negative_hit(&neg_key) {
            return Err(ImageError::Unavailable(status));
        }

        // Bound concurrent upstream fetches. A closed semaphore (never expected)
        // degrades to proceeding without the limit rather than failing.
        let _permit = self.fetch_limit.acquire().await.ok();

        // Re-check: another task may have populated the cache — or recorded a failure —
        // while we waited on the permit.
        if let Ok(bytes) = tokio::fs::read(&path).await {
            return Ok(CachedImage {
                bytes,
                content_type,
            });
        }
        if let Some(status) = self.negative_hit(&neg_key) {
            return Err(ImageError::Unavailable(status));
        }

        let bytes = self.fetch_tracked(&neg_key, source_url).await?;

        tokio::fs::create_dir_all(&dir).await?;
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!(".{}.{ext}.{seq}.tmp", sanitize(key)));
        tokio::fs::write(&tmp, &bytes).await?;
        // Rename is atomic on one filesystem; if a concurrent fetch won, reuse it.
        if let Err(err) = tokio::fs::rename(&tmp, &path).await {
            let _ = tokio::fs::remove_file(&tmp).await;
            if let Ok(existing) = tokio::fs::read(&path).await {
                return Ok(CachedImage {
                    bytes: existing,
                    content_type,
                });
            }
            return Err(err.into());
        }
        Ok(CachedImage {
            bytes,
            content_type,
        })
    }

    /// [`download`](Self::download), then reflect the outcome into the negative cache:
    /// a definitive [`Unavailable`](ImageError::Unavailable) is recorded (so the miss is
    /// served from memory for the next [`NEGATIVE_TTL`]), a success clears any prior
    /// failure, and a transient [`Http`](ImageError::Http) error is left un-cached so it
    /// retries next time. The **caller holds** the `fetch_limit` permit around this.
    async fn fetch_tracked(&self, neg_key: &str, source_url: &str) -> Result<Vec<u8>, ImageError> {
        match self.download(source_url).await {
            Ok(bytes) => {
                self.clear_negative(neg_key);
                Ok(bytes)
            }
            Err(err) => {
                if let ImageError::Unavailable(status) = &err {
                    self.record_negative(neg_key.to_string(), *status);
                }
                Err(err)
            }
        }
    }

    /// Fetch an image's raw bytes from an **allow-listed** provider CDN without ever
    /// touching the on-disk cache — used by the fingerprint build to hash-and-discard
    /// each card image, so it never persists the (hundreds-of-MB) catalogue to disk.
    /// Shares the live-view politeness budget (the `fetch_limit` semaphore) and the
    /// same host allow-list as the image proxy, so a bad stored URL can't turn the
    /// build into an SSRF. Errors mirror [`download`]: a definitive 4xx is
    /// [`Unavailable`](ImageError::Unavailable), a transient failure is
    /// [`Http`](ImageError::Http). A non-allow-listed host is refused as `Unavailable`.
    pub async fn fetch_bytes(&self, source_url: &str) -> Result<Vec<u8>, ImageError> {
        if !is_allowed_image_url(source_url) {
            return Err(ImageError::Unavailable(StatusCode::FORBIDDEN));
        }
        let _permit = self.fetch_limit.acquire().await.ok();
        self.download(source_url).await
    }

    /// Raw upstream GET of `source_url`, returning the body bytes. Shared by the
    /// on-disk cache-miss path and CDN mode (which never persists the result).
    /// The **caller holds** the `fetch_limit` permit around this (each call site
    /// acquires it once), so this must not re-acquire it — doing so would let the
    /// disk path hold its outer permit while blocking on an inner one and deadlock
    /// the semaphore. `source_url` is resolved from trusted stored data, never
    /// from user input.
    ///
    /// A `4xx` that means the asset simply isn't there — anything client-side except a
    /// rate-limit (`429`) or timeout (`408`), which are transient and worth retrying —
    /// maps to [`ImageError::Unavailable`] so the caller can 404 it and cache the miss.
    /// Everything else non-2xx (`5xx`, `429`, `408`) stays an [`ImageError::Http`] and is
    /// retried on the next request.
    async fn download(&self, source_url: &str) -> Result<Vec<u8>, ImageError> {
        let response = self.client.get(source_url).send().await?;
        let status = response.status();
        if status.is_client_error()
            && status != StatusCode::TOO_MANY_REQUESTS
            && status != StatusCode::REQUEST_TIMEOUT
        {
            return Err(ImageError::Unavailable(status));
        }
        Ok(response.error_for_status()?.bytes().await?.to_vec())
    }
}

/// Whether the image fetchers may retrieve a URL: HTTPS on a known provider CDN
/// (Scryfall for card art / set icons, the TCGplayer CDN for sealed-product images).
/// Stored/derived image URLs all come from those providers; this guards every outbound
/// image fetch — the proxy handler ([`crate::handlers::catalog`] re-exports it) and the
/// fingerprint build's [`ImageCache::fetch_bytes`] — against a bad value ever turning a
/// fetch into an SSRF.
pub(crate) fn is_allowed_image_url(url: &str) -> bool {
    match reqwest::Url::parse(url) {
        Ok(parsed) => {
            parsed.scheme() == "https"
                && parsed.host_str().is_some_and(|host| {
                    host == "scryfall.io"
                        || host.ends_with(".scryfall.io")
                        || host == "tcgplayer-cdn.tcgplayer.com"
                })
        }
        Err(_) => false,
    }
}

/// Reduce a path segment to a safe `[a-z0-9_-]` form, preventing traversal.
fn sanitize(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use reqwest::{Client, StatusCode};

    use super::{ImageCache, ImageError, sanitize};

    /// A fresh, not-yet-created temp directory unique per call (so concurrent
    /// tests never share a cache root).
    fn unique_tmp_dir(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("tcglense-img-{tag}-{}-{n}", std::process::id()))
    }

    /// Spawn a throwaway HTTP server on a random loopback port that answers every
    /// path with `body`. Returns its address; the task is cancelled when the test's
    /// runtime is dropped.
    async fn spawn_image_server(body: Vec<u8>) -> SocketAddr {
        let app = axum::Router::new().fallback(move || {
            let body = body.clone();
            async move { body }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test image server");
        let addr = listener.local_addr().expect("test server addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        addr
    }

    /// Spawn a throwaway HTTP server that answers the first `fail_count` requests with
    /// `fail_status` (empty body) and every request after that with `200` + `body`.
    /// Returns its address and the shared hit counter, so a test can both drive the
    /// failure→success transition and assert how many times upstream was actually
    /// reached — proving the negative cache did (or didn't) short-circuit the fetch.
    async fn spawn_counting_server(
        fail_status: u16,
        fail_count: u64,
        body: Vec<u8>,
    ) -> (SocketAddr, Arc<AtomicU64>) {
        let hits = Arc::new(AtomicU64::new(0));
        let counter = hits.clone();
        let app = axum::Router::new().fallback(move || {
            let body = body.clone();
            let counter = counter.clone();
            async move {
                let n = counter.fetch_add(1, Ordering::SeqCst);
                if n < fail_count {
                    let status =
                        axum::http::StatusCode::from_u16(fail_status).expect("valid status");
                    (status, Vec::new())
                } else {
                    (axum::http::StatusCode::OK, body.clone())
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test image server");
        let addr = listener.local_addr().expect("test server addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (addr, hits)
    }

    #[tokio::test]
    async fn unavailable_upstream_is_negatively_cached() {
        // A product with no CDN art 403s every time. The first request records the miss;
        // the second is served from the negative cache without touching upstream again.
        let (addr, hits) = spawn_counting_server(403, u64::MAX, Vec::new()).await;
        let dir = unique_tmp_dir("neg-403");
        let cache = ImageCache::new(dir.clone(), Client::new(), false);
        let url = format!("http://{addr}/missing.jpg");

        let first = cache.get("products", "normal", "404id", &url).await;
        assert!(matches!(first, Err(ImageError::Unavailable(s)) if s == StatusCode::FORBIDDEN));

        let second = cache.get("products", "normal", "404id", &url).await;
        assert!(matches!(second, Err(ImageError::Unavailable(s)) if s == StatusCode::FORBIDDEN));

        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the second request must be served from the negative cache, not re-fetched"
        );
        assert!(
            !dir.exists(),
            "a failed fetch must not create the cache dir"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn cdn_mode_negatively_caches_unavailable() {
        // The negative cache spares the origin in CDN mode too — a fronting CDN won't
        // cache a 403, so without it every miss would re-hit upstream.
        let (addr, hits) = spawn_counting_server(403, u64::MAX, Vec::new()).await;
        let dir = unique_tmp_dir("neg-cdn");
        let cache = ImageCache::new(dir.clone(), Client::new(), true);
        let url = format!("http://{addr}/missing.jpg");

        for _ in 0..2 {
            let out = cache.get("products", "normal", "404id", &url).await;
            assert!(matches!(out, Err(ImageError::Unavailable(_))));
        }
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "cdn mode must serve the repeat miss from memory"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn server_error_is_not_negatively_cached() {
        // A 5xx is transient, not "no image": it must NOT be cached, so a retry that
        // finds the image now available (200) succeeds and persists it.
        let (addr, hits) = spawn_counting_server(500, 1, b"NOW-OK".to_vec()).await;
        let dir = unique_tmp_dir("neg-500");
        let cache = ImageCache::new(dir.clone(), Client::new(), false);
        let url = format!("http://{addr}/flaky.jpg");

        let first = cache.get("mtg", "normal", "flaky", &url).await;
        assert!(
            matches!(first, Err(ImageError::Http(_))),
            "a 5xx is a transient Http error, never Unavailable"
        );

        let second = cache
            .get("mtg", "normal", "flaky", &url)
            .await
            .expect("a retry after a 5xx must reach upstream again");
        assert_eq!(second.bytes, b"NOW-OK");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            2,
            "a 5xx must be retried, not cached as unavailable"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn rate_limit_is_not_negatively_cached() {
        // A 429 means "back off", not "no image" — it stays a retryable Http error and is
        // never negatively cached (else a rate-limited asset would 404 for the whole TTL).
        let (addr, hits) = spawn_counting_server(429, 1, b"NOW-OK".to_vec()).await;
        let dir = unique_tmp_dir("neg-429");
        let cache = ImageCache::new(dir.clone(), Client::new(), false);
        let url = format!("http://{addr}/limited.jpg");

        let first = cache.get("mtg", "normal", "limited", &url).await;
        assert!(
            matches!(first, Err(ImageError::Http(_))),
            "a 429 is transient, not Unavailable"
        );

        let second = cache
            .get("mtg", "normal", "limited", &url)
            .await
            .expect("a 429 must be retried, not cached as unavailable");
        assert_eq!(second.bytes, b"NOW-OK");
        assert_eq!(hits.load(Ordering::SeqCst), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn request_timeout_is_not_negatively_cached() {
        // A 408 is the other transient client error excluded from the negative cache (like
        // 429): it stays a retryable Http error, so a timing-out asset isn't 404'd for 6h.
        let (addr, hits) = spawn_counting_server(408, 1, b"NOW-OK".to_vec()).await;
        let dir = unique_tmp_dir("neg-408");
        let cache = ImageCache::new(dir.clone(), Client::new(), false);
        let url = format!("http://{addr}/slow.jpg");

        let first = cache.get("mtg", "normal", "slow", &url).await;
        assert!(
            matches!(first, Err(ImageError::Http(_))),
            "a 408 is transient, not Unavailable"
        );

        let second = cache
            .get("mtg", "normal", "slow", &url)
            .await
            .expect("a 408 must be retried, not cached as unavailable");
        assert_eq!(second.bytes, b"NOW-OK");
        assert_eq!(hits.load(Ordering::SeqCst), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn stale_negative_entry_is_reprobed_after_ttl() {
        // The "minimum time to try again" (issue #214): a failure is honoured only for the
        // TTL window, after which the dead URL is probed once more — and if the image has
        // since appeared, it succeeds and is cached normally. Driven with a tiny injected
        // TTL so the test doesn't wait the production six hours.
        let (addr, hits) = spawn_counting_server(403, 1, b"NOW-OK".to_vec()).await;
        let dir = unique_tmp_dir("neg-ttl");
        let cache = ImageCache::with_negative_ttl(
            dir.clone(),
            Client::new(),
            false,
            Duration::from_millis(30),
        );
        let url = format!("http://{addr}/appears-later.jpg");

        let first = cache.get("products", "normal", "later", &url).await;
        assert!(matches!(first, Err(ImageError::Unavailable(_))));
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        // Past the window, the proxy tries again; the image now exists, so the re-probe
        // succeeds (hits==2 proves the stale entry was NOT served from memory) and persists.
        tokio::time::sleep(Duration::from_millis(120)).await;
        let served = cache
            .get("products", "normal", "later", &url)
            .await
            .expect("a stale negative entry must be re-probed after the TTL");
        assert_eq!(served.bytes, b"NOW-OK");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            2,
            "re-probe must reach upstream again"
        );

        // The now-successful fetch was persisted like any other (served from disk, upstream
        // gone), confirming the re-probe path joins the normal cache flow.
        let disk = cache
            .get("products", "normal", "later", "http://127.0.0.1:1/gone")
            .await
            .expect("re-probed image should now serve from disk");
        assert_eq!(disk.bytes, b"NOW-OK");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn cdn_mode_serves_upstream_without_touching_disk() {
        let addr = spawn_image_server(b"CDN-IMAGE-BYTES".to_vec()).await;
        let dir = unique_tmp_dir("cdn-skip");
        let cache = ImageCache::new(dir.clone(), Client::new(), true);

        let url = format!("http://{addr}/img.jpg");
        let image = cache
            .get("mtg", "normal", "abc", &url)
            .await
            .expect("cdn-mode fetch");

        assert_eq!(image.bytes, b"CDN-IMAGE-BYTES");
        assert_eq!(image.content_type, "image/jpeg");
        // The whole point of CDN mode: nothing is persisted — the cache root is
        // never even created.
        assert!(!dir.exists(), "cdn mode must not write to the image dir");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn normal_mode_persists_and_then_serves_from_disk() {
        let addr = spawn_image_server(b"DISK-IMAGE-BYTES".to_vec()).await;
        let dir = unique_tmp_dir("disk-write");
        let cache = ImageCache::new(dir.clone(), Client::new(), false);

        let url = format!("http://{addr}/img.jpg");
        let image = cache
            .get("mtg", "normal", "abc", &url)
            .await
            .expect("first fetch");
        assert_eq!(image.bytes, b"DISK-IMAGE-BYTES");

        // The download was written to disk at the sanitised path.
        let path = dir.join("mtg").join("normal").join("abc.jpg");
        assert!(path.exists(), "normal mode must persist the download");

        // A second read is served from disk — proven by pointing at an
        // unreachable upstream and still getting the cached bytes back.
        let served = cache
            .get("mtg", "normal", "abc", "http://127.0.0.1:1/gone")
            .await
            .expect("disk cache hit");
        assert_eq!(served.bytes, b"DISK-IMAGE-BYTES");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_blocks_traversal_and_lowercases() {
        assert_eq!(sanitize("../../etc/passwd"), "______etc_passwd");
        assert_eq!(sanitize("AbC-123_x"), "abc-123_x");
        assert_eq!(sanitize("a/b"), "a_b");
    }

    #[test]
    fn sanitize_neutralizes_more_traversal_vectors() {
        // Windows separators, drive letters, leading slashes and NUL bytes all
        // collapse to '_', so a crafted game/size/key can never escape the cache
        // directory regardless of host OS path semantics.
        assert_eq!(sanitize("/etc/shadow"), "_etc_shadow");
        assert_eq!(sanitize("..\\..\\windows"), "______windows");
        assert_eq!(sanitize("C:\\Win"), "c__win");
        assert_eq!(sanitize("a\0b"), "a_b");
        // The output alphabet is strictly [a-z0-9_-]; no separator survives.
        let cleaned = sanitize("../foo/./bar%2e%2e");
        assert!(
            cleaned
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'),
            "unexpected char in {cleaned}"
        );
    }
}

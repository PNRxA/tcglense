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

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use reqwest::Client;
use tokio::sync::Semaphore;

/// Monotonic counter giving each in-flight download a unique temp filename.
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
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
}

impl ImageCache {
    pub fn new(base_dir: PathBuf, client: Client, cdn_mode: bool) -> Self {
        Self {
            base_dir,
            client,
            fetch_limit: Semaphore::new(8),
            cdn_mode,
        }
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
        // CDN mode: never touch `base_dir`. A fronting CDN caches the immutable
        // response, so fetch straight from upstream and stream the bytes through.
        if self.cdn_mode {
            let _permit = self.fetch_limit.acquire().await.ok();
            let bytes = self.download(source_url).await?;
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

        // Bound concurrent upstream fetches. A closed semaphore (never expected)
        // degrades to proceeding without the limit rather than failing.
        let _permit = self.fetch_limit.acquire().await.ok();

        // Re-check: another task may have populated the cache while we waited.
        if let Ok(bytes) = tokio::fs::read(&path).await {
            return Ok(CachedImage {
                bytes,
                content_type,
            });
        }

        let bytes = self.download(source_url).await?;

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

    /// Raw upstream GET of `source_url`, returning the body bytes. Shared by the
    /// on-disk cache-miss path and CDN mode (which never persists the result).
    /// The **caller holds** the `fetch_limit` permit around this (each call site
    /// acquires it once), so this must not re-acquire it — doing so would let the
    /// disk path hold its outer permit while blocking on an inner one and deadlock
    /// the semaphore. `source_url` is resolved from trusted stored data, never
    /// from user input.
    async fn download(&self, source_url: &str) -> Result<Vec<u8>, ImageError> {
        Ok(self
            .client
            .get(source_url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec())
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
    use std::sync::atomic::{AtomicU64, Ordering};

    use reqwest::Client;

    use super::{ImageCache, sanitize};

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

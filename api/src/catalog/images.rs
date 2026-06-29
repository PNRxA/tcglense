//! Lazy, on-disk cache for card images.
//!
//! Card art is fetched from the upstream provider (Scryfall) the first time it
//! is requested, written to a directory, and served from disk thereafter — so
//! we never bulk-download the provider's entire image catalogue, yet every image
//! shown is persisted locally. Generic over `game`, so any TCG reuses it.

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
}

impl ImageCache {
    pub fn new(base_dir: PathBuf, client: Client) -> Self {
        Self {
            base_dir,
            client,
            fetch_limit: Semaphore::new(8),
        }
    }

    /// Return the cached image for `(game, size, key)`, downloading it from
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
        // Sanitise every path segment defensively against traversal.
        let dir = self.base_dir.join(sanitize(game)).join(sanitize(size));
        let path = dir.join(format!("{}.{ext}", sanitize(key)));

        if let Ok(bytes) = tokio::fs::read(&path).await {
            return Ok(CachedImage { bytes, content_type });
        }

        // Bound concurrent upstream fetches. A closed semaphore (never expected)
        // degrades to proceeding without the limit rather than failing.
        let _permit = self.fetch_limit.acquire().await.ok();

        // Re-check: another task may have populated the cache while we waited.
        if let Ok(bytes) = tokio::fs::read(&path).await {
            return Ok(CachedImage { bytes, content_type });
        }

        let bytes = self
            .client
            .get(source_url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec();

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
        Ok(CachedImage { bytes, content_type })
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
    use super::sanitize;

    #[test]
    fn sanitize_blocks_traversal_and_lowercases() {
        assert_eq!(sanitize("../../etc/passwd"), "______etc_passwd");
        assert_eq!(sanitize("AbC-123_x"), "abc-123_x");
        assert_eq!(sanitize("a/b"), "a_b");
    }
}

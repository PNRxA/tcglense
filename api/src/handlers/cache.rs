//! `Cache-Control` policy for the HTTP layer, applied as response middleware.
//!
//! The API serves two very different kinds of response, and a shared cache (a CDN)
//! must treat them differently:
//!
//! * **Public catalog reads** (`/api/games/...`) are game data that changes at most
//!   once a day (the Scryfall sync). They are the same for every visitor, so they
//!   are safe to cache in the browser *and* at a CDN — that is the bulk of the
//!   traffic and where CDN offload matters. [`public_cache_layer`] tags successful
//!   ones with [`PUBLIC_CATALOG_CACHE`].
//! * **Per-user / live / error responses** must never be stored by a shared cache:
//!   auth responses carry access tokens and `Set-Cookie`, the import-status route is
//!   a live progress signal the SPA polls, and a cached `404`/`5xx` would pin a
//!   transient failure. [`no_store_layer`] marks these `no-store`.
//!
//! The image proxy already sets its own long-lived `immutable` header
//! (`IMAGE_CACHE_CONTROL` in [`super::catalog`]); [`public_cache_layer`] leaves any
//! response that already carries a `Cache-Control` untouched so that stays intact.

use axum::{
    http::{HeaderValue, StatusCode, header::CACHE_CONTROL},
    response::Response,
};

/// `Cache-Control` for public, CDN-cacheable catalog reads.
///
/// * `public` — a shared cache (CDN) may store it, not just the browser.
/// * `max-age=300` — a browser reuses it for 5 minutes before revalidating.
/// * `s-maxage=3600` — a shared cache keeps it fresh for an hour (the data turns
///   over at most daily, so the origin is hit ~once an hour per object).
/// * `stale-while-revalidate=86400` — for a day past freshness the CDN may serve
///   the stale copy immediately while it refreshes in the background, so a cache
///   miss never blocks a visitor on the origin.
pub const PUBLIC_CATALOG_CACHE: &str =
    "public, max-age=300, s-maxage=3600, stale-while-revalidate=86400";

/// `Cache-Control` for responses that must never be stored by any cache
/// (per-user auth, live import status, and every error response).
pub const NO_STORE: &str = "no-store";

/// Decide the `Cache-Control` value for a *public catalog* response, or `None` to
/// leave the response's existing header in place.
///
/// * An already-set header (the image proxy's `immutable`) wins — return `None`.
/// * A successful read is shared-cacheable — [`PUBLIC_CATALOG_CACHE`].
/// * Anything else (a `404` for an unknown card, a `422` bad query, a `5xx`) is
///   [`NO_STORE`] so a CDN can't pin a transient or negative result.
///
/// Kept as a pure function so the policy is unit-testable without spinning up the
/// router.
pub fn public_cache_value(status: StatusCode, has_cache_control: bool) -> Option<&'static str> {
    if has_cache_control {
        None
    } else if status.is_success() {
        Some(PUBLIC_CATALOG_CACHE)
    } else {
        Some(NO_STORE)
    }
}

/// Response middleware for the public catalog routes: apply [`public_cache_value`].
pub async fn public_cache_layer(mut response: Response) -> Response {
    let has_cache_control = response.headers().contains_key(CACHE_CONTROL);
    if let Some(value) = public_cache_value(response.status(), has_cache_control) {
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static(value));
    }
    response
}

/// Response middleware for private / live routes: force `Cache-Control: no-store`
/// on every response (success or error) so credentials, cookies, and live status
/// are never stored by a browser or a shared cache.
pub async fn no_store_layer(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_success_is_shared_cacheable() {
        assert_eq!(
            public_cache_value(StatusCode::OK, false),
            Some(PUBLIC_CATALOG_CACHE)
        );
        assert_eq!(
            public_cache_value(StatusCode::NO_CONTENT, false),
            Some(PUBLIC_CATALOG_CACHE)
        );
    }

    #[test]
    fn existing_cache_control_is_left_untouched() {
        // The image proxy sets its own `immutable` header; we must not clobber it,
        // even on a successful response.
        assert_eq!(public_cache_value(StatusCode::OK, true), None);
        assert_eq!(public_cache_value(StatusCode::NOT_FOUND, true), None);
    }

    #[test]
    fn errors_are_never_shared_cached() {
        for status in [
            StatusCode::NOT_FOUND,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::UNAUTHORIZED,
            StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            assert_eq!(public_cache_value(status, false), Some(NO_STORE));
        }
    }
}

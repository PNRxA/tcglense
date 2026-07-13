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
//!
//! On top of the freshness policy, [`conditional_request_layer`] adds **validators**
//! for conditional requests: it hashes a cacheable success into a weak `ETag` and
//! turns a matching `If-None-Match` into a bodyless `304 Not Modified`, so once a
//! CDN / browser entry goes stale the revalidation transfers headers instead of the
//! whole body. It deliberately skips `immutable` (images — never revalidated) and
//! `no-store` (errors / per-user) responses.

use axum::{
    body::{Body, HttpBody, to_bytes},
    extract::Request,
    http::{
        HeaderValue, Method, StatusCode,
        header::{CACHE_CONTROL, ETAG, IF_NONE_MATCH},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};

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

/// `Cache-Control` for public, handle-keyed **holdings** reads (`/api/u/{handle}/...`).
///
/// Shorter-lived than the catalog: a public collection changes whenever its owner edits
/// it, not just on the daily sync, and there is **no active purge on edit** — so a shared
/// cache may serve a stale copy for up to `s-maxage` after the owner toggles a collection
/// private or changes it. That is accepted for v1: the owner's *own* view is served from
/// the authed, `no-store` routes, so they always see edits immediately; only the anonymous
/// public copy lags (≤ 5 min = `s-maxage`), and a self-host without a CDN only ever applies
/// the 60-second browser `max-age`.
///
/// Deliberately **no `stale-while-revalidate`**: because this content is privacy-sensitive
/// (a made-private collection must stop being served), the worst-case public-exposure
/// window must equal `s-maxage`, not `s-maxage + stale-while-revalidate`. The catalog can
/// afford SWR; a de-listed collection cannot.
///
/// Unlike the per-user authed collection routes (which are `no-store`), this is `public`
/// because the URL — the handle plus the game — fully identifies the content, exactly as a
/// card id does for the catalog. `max-age=60` (browser), `s-maxage=300` (shared cache).
pub const PUBLIC_HOLDINGS_CACHE: &str = "public, max-age=60, s-maxage=300";

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
    cache_value(status, has_cache_control, PUBLIC_CATALOG_CACHE)
}

/// Decide the `Cache-Control` for a public, handle-keyed **holdings** response
/// (`/api/u/{handle}/...`): a success is shared-cacheable under [`PUBLIC_HOLDINGS_CACHE`],
/// an already-set header wins (`None`), and every error is [`NO_STORE`] so a CDN never
/// pins the `404` a private/unknown handle returns.
pub fn public_holdings_cache_value(
    status: StatusCode,
    has_cache_control: bool,
) -> Option<&'static str> {
    cache_value(status, has_cache_control, PUBLIC_HOLDINGS_CACHE)
}

/// Shared "errors → `no-store`, success → `fresh`" policy, leaving any pre-set header
/// (e.g. the image proxy's `immutable`) intact. `fresh` is the shared-cache policy to tag
/// a success with — [`PUBLIC_CATALOG_CACHE`] for the catalog, [`PUBLIC_HOLDINGS_CACHE`]
/// for the public holdings reads.
fn cache_value(
    status: StatusCode,
    has_cache_control: bool,
    fresh: &'static str,
) -> Option<&'static str> {
    if has_cache_control {
        None
    } else if status.is_success() {
        Some(fresh)
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

/// Response middleware for the public, handle-keyed holdings routes (`/api/u/...`):
/// apply [`public_holdings_cache_value`].
pub async fn public_holdings_cache_layer(mut response: Response) -> Response {
    let has_cache_control = response.headers().contains_key(CACHE_CONTROL);
    if let Some(value) = public_holdings_cache_value(response.status(), has_cache_control) {
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

// ---------- Conditional requests (`ETag` / `304 Not Modified`) ----------

/// Upper bound on a response body we'll buffer to hash into an `ETag`. Every
/// catalog / sitemap body is a fully-materialised, bounded `Bytes` far under this
/// (the largest — a 50 000-URL card-sitemap chunk — is a few MB), so this is only a
/// belt-and-braces guard against buffering something unexpectedly large; a body that
/// somehow exceeds it (or has no known size) is served un-`ETag`ged rather than
/// buffered without bound.
const MAX_ETAG_BODY_BYTES: usize = 32 * 1024 * 1024;

/// Whether a response is a candidate for an `ETag` validator.
///
/// Only a **revalidatable, shared-cacheable success** gets one:
/// * a non-2xx is [`NO_STORE`] (errors are never cached), so there's nothing to
///   revalidate;
/// * an `immutable` response (the image / icon proxy) is never revalidated within
///   its `max-age`, so hashing its — potentially large, binary — body would buffer
///   megabytes for no benefit;
/// * a `no-store` response must never be stored, so a validator is meaningless.
///
/// Everything else on the public router (the catalog reads and the sitemaps, whose
/// `Cache-Control` is `public, max-age=…` without `immutable`) is worth an `ETag`
/// so a stale-cache revalidation can come back as a cheap `304`. Kept pure so the
/// policy is unit-testable without the router.
pub fn is_etaggable(status: StatusCode, cache_control: Option<&str>) -> bool {
    match cache_control {
        Some(cc) => status.is_success() && !cc.contains("no-store") && !cc.contains("immutable"),
        None => false,
    }
}

/// Derive a **weak** `ETag` from a response body: `W/"<hex>"` over a 128-bit prefix
/// of the body's SHA-256.
///
/// It's *weak* (`W/`) because the validator identifies the payload we serialised,
/// not a specific byte-for-byte transfer encoding — a downstream CDN is free to
/// gzip it in transit without invalidating the tag. `If-None-Match` revalidation
/// uses the weak-comparison function regardless, so this still yields a `304` on a
/// match. A 128-bit hash makes an accidental collision (two different bodies, same
/// tag → a wrongly-suppressed update) negligible.
pub fn weak_etag(body: &[u8]) -> String {
    let digest = Sha256::digest(body);
    let hex = hex::encode(&digest[..16]);
    format!("W/\"{hex}\"")
}

/// Whether a request's `If-None-Match` header value satisfies our current `etag`,
/// i.e. the client already holds this exact representation and we may answer `304`.
///
/// Implements RFC 9110 §13.1.2 with the weak-comparison function: `*` matches any
/// current representation, and each comma-separated candidate matches if its opaque
/// tag equals ours ignoring a `W/` weakness prefix on either side (so a client that
/// echoes our tag as either `W/"x"` or `"x"` still matches).
pub fn if_none_match_matches(header: &str, etag: &str) -> bool {
    let ours = etag.strip_prefix("W/").unwrap_or(etag);
    header.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == ours
    })
}

/// Build a bodyless `304 Not Modified` carrying the validators a `200` would have
/// (`ETag` plus the response's `Cache-Control`), as RFC 9110 §15.4.5 requires so the
/// cache can refresh freshness without the body.
fn not_modified_response(cache_control: Option<HeaderValue>, etag: HeaderValue) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NOT_MODIFIED;
    let headers = response.headers_mut();
    headers.insert(ETAG, etag);
    if let Some(cc) = cache_control {
        headers.insert(CACHE_CONTROL, cc);
    }
    response
}

/// Response middleware for the public catalog routes: attach an `ETag` to cacheable
/// successes and turn a matching `If-None-Match` into a `304 Not Modified`, so a
/// CDN / browser revalidating a stale entry gets headers instead of the full body.
///
/// Runs *outside* [`public_cache_layer`] so it can read the `Cache-Control` that
/// layer set to decide what's worth an `ETag` (see [`is_etaggable`]). Restricted to
/// `GET`: axum serves `HEAD` off the same handler but strips the body, so hashing a
/// `HEAD` response could yield a tag that disagrees with the `GET` — `HEAD` simply
/// carries no validator (conditional revalidation uses `GET`).
pub async fn conditional_request_layer(request: Request, next: Next) -> Response {
    let is_get = request.method() == Method::GET;
    let if_none_match = request
        .headers()
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let response = next.run(request).await;

    let cache_control = response
        .headers()
        .get(CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if !is_get || !is_etaggable(response.status(), cache_control.as_deref()) {
        return response;
    }

    // Buffer the (in-memory, bounded) body so we can hash it into the validator. A
    // body of unknown or over-cap size is passed through untouched rather than
    // buffered — defensive only; the catalog/sitemap bodies are always well under.
    let (mut parts, body) = response.into_parts();
    if body.size_hint().upper().is_none_or(|u| u > MAX_ETAG_BODY_BYTES as u64) {
        return Response::from_parts(parts, body);
    }
    let bytes = match to_bytes(body, MAX_ETAG_BODY_BYTES).await {
        Ok(bytes) => bytes,
        // Unreachable given the size-hint guard, but the body is consumed on error
        // so we can't rebuild the original — fail loud rather than serve a truncated
        // response.
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let etag = weak_etag(&bytes);
    // The tag is `W/"<hex>"` — always valid ASCII — so this never fails in practice.
    let Ok(etag_value) = HeaderValue::from_str(&etag) else {
        return Response::from_parts(parts, Body::from(bytes));
    };

    // A matching `If-None-Match` means the client already holds this exact body:
    // answer `304` (headers only) and skip the re-transfer.
    if let Some(inm) = &if_none_match
        && if_none_match_matches(inm, &etag)
    {
        return not_modified_response(parts.headers.get(CACHE_CONTROL).cloned(), etag_value);
    }

    parts.headers.insert(ETAG, etag_value);
    Response::from_parts(parts, Body::from(bytes))
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

    #[test]
    fn public_holdings_success_shared_cached_errors_no_store() {
        // A handle-keyed public holdings success is shared-cacheable...
        assert_eq!(
            public_holdings_cache_value(StatusCode::OK, false),
            Some(PUBLIC_HOLDINGS_CACHE)
        );
        // ...but a 404 for a private/unknown handle is never CDN-pinned...
        assert_eq!(
            public_holdings_cache_value(StatusCode::NOT_FOUND, false),
            Some(NO_STORE)
        );
        // ...and a pre-set header still wins.
        assert_eq!(public_holdings_cache_value(StatusCode::OK, true), None);
        // It carries neither `no-store` nor `immutable`, so it earns an ETag (304s work).
        assert!(is_etaggable(StatusCode::OK, Some(PUBLIC_HOLDINGS_CACHE)));
    }

    #[test]
    fn only_revalidatable_successes_get_an_etag() {
        // A shared-cacheable catalog read / sitemap: worth a validator.
        assert!(is_etaggable(StatusCode::OK, Some(PUBLIC_CATALOG_CACHE)));
        assert!(is_etaggable(
            StatusCode::OK,
            Some(crate::handlers::sitemap::SITEMAP_CACHE_CONTROL)
        ));
    }

    #[test]
    fn immutable_and_no_store_and_errors_are_not_etagged() {
        // Images/icons are `immutable` — never revalidated, so no point hashing
        // (potentially large, binary) bodies.
        assert!(!is_etaggable(
            StatusCode::OK,
            Some("public, max-age=2592000, immutable")
        ));
        // `no-store` (an error under the public layer) must never carry a validator.
        assert!(!is_etaggable(StatusCode::OK, Some(NO_STORE)));
        // A non-2xx is never etaggable regardless of the (nonsensical) header.
        assert!(!is_etaggable(StatusCode::NOT_FOUND, Some(PUBLIC_CATALOG_CACHE)));
        // No `Cache-Control` at all (shouldn't happen on the public layer) → skip.
        assert!(!is_etaggable(StatusCode::OK, None));
    }

    #[test]
    fn weak_etag_is_deterministic_content_addressed_and_well_formed() {
        let a = weak_etag(b"hello world");
        // Stable for the same bytes, distinct for different bytes.
        assert_eq!(a, weak_etag(b"hello world"));
        assert_ne!(a, weak_etag(b"hello worlds"));
        // Shape: weak prefix + a 32-hex-char (128-bit) quoted opaque tag.
        assert!(a.starts_with("W/\""));
        assert!(a.ends_with('"'));
        let hex = a.strip_prefix("W/\"").unwrap().strip_suffix('"').unwrap();
        assert_eq!(hex.len(), 32);
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn if_none_match_matches_handles_star_lists_and_weakness() {
        let etag = weak_etag(b"body");
        let bare = etag.strip_prefix("W/").unwrap().to_string(); // the strong-looking `"<hex>"`

        // Exact echo of our weak tag matches.
        assert!(if_none_match_matches(&etag, &etag));
        // A client that drops the `W/` (strong-form spelling) still matches (weak cmp).
        assert!(if_none_match_matches(&bare, &etag));
        // `*` matches any current representation.
        assert!(if_none_match_matches("*", &etag));
        // Present within a comma-separated list, with surrounding whitespace.
        let list = format!("\"other\", {etag} , \"another\"");
        assert!(if_none_match_matches(&list, &etag));
        // A different tag does not match.
        assert!(!if_none_match_matches("\"deadbeef\"", &etag));
        assert!(!if_none_match_matches(&weak_etag(b"different"), &etag));
    }
}

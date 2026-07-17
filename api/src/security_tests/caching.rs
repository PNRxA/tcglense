//! Cache-Control policy (CDN / browser caching).
//!
//! A shared cache (CDN) must be able to cache the public catalog reads (they are
//! the same for everyone and change at most daily) while never storing per-user
//! auth responses, the live import-status signal, or error responses. These drive
//! the real router so the route-group wiring in `build_router` is covered, not just
//! the pure policy in `handlers::cache`.

use super::harness::*;
use axum::http::header::{ETAG, IF_NONE_MATCH};

/// The `ETag` header value as an owned string, or `None` if absent.
fn etag(headers: &HeaderMap) -> Option<String> {
    headers
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// A `GET` carrying an `If-None-Match`, to drive a conditional (revalidation) request.
fn get_if_none_match(uri: &str, inm: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header(IF_NONE_MATCH, inm)
        .body(Body::empty())
        .unwrap()
}

/// A bare `HEAD` request (axum serves it off the same `get` handler).
fn head(uri: &str) -> Request<Body> {
    Request::builder()
        .method("HEAD")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn public_catalog_reads_are_shared_cacheable() {
    let app = test_app_with_catalog().await;

    // The games list is always present (a static registry), so this is a clean 200.
    let (status, headers, _) = send(&app, get("/api/games")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "public catalog reads must be browser + CDN cacheable"
    );

    // A seeded set listing is likewise shared-cacheable.
    let (status, headers, _) = send(&app, get("/api/games/mtg/sets")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE)
    );
}

#[tokio::test]
async fn auth_responses_are_never_cached() {
    let app = test_app().await;

    // An unauthenticated /me is a 401; either way it must be no-store so a shared
    // cache can never retain a response tied to credentials.
    let (status, headers, _) = send(&app, get("/api/auth/me")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // Registration is a generic 200 (email-first): still no-store, since even the
    // generic body carries per-request state a shared cache must never retain.
    let email = "cache-nostore@example.com";
    let (status, headers, _) = send(
        &app,
        json_post("/api/auth/register", json!({ "email": email })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // Completing the registration is a 200 that mints a session (access token +
    // refresh cookie) — the very kind of per-user response a shared cache must
    // never store.
    let token = latest_email_token(&app, email).await;
    let (status, headers, _) = send(
        &app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": token, "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // And a successful login (now that the account exists) is likewise no-store.
    let (status, headers, _) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": email, "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn live_import_status_is_never_cached() {
    // The SPA polls import status for live progress; a CDN caching it would freeze
    // the progress UI, so it must be no-store even though it's a public GET.
    let app = test_app().await;
    let (status, headers, _) = send(&app, get("/api/games/mtg/status")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

#[tokio::test]
async fn catalog_errors_are_not_shared_cached() {
    // A 404 on a public route must not be pinned by a CDN (an unknown id/set is
    // often transient — the sync may not have imported it yet).
    let app = test_app().await;
    let (status, headers, _) = send(&app, get("/api/games/mtg/sets/does-not-exist")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));
}

// ---------- Conditional requests (`ETag` / `304`) — issue #76 ----------

#[tokio::test]
async fn cacheable_read_carries_a_weak_etag_and_revalidates_to_304() {
    let app = test_app().await;

    // The games list is a clean 200; a cacheable success now carries a weak ETag.
    let (status, headers, body) = send(&app, get("/api/games")).await;
    assert_eq!(status, StatusCode::OK);
    let tag = etag(&headers).expect("a cacheable success must carry an ETag");
    assert!(
        tag.starts_with("W/\""),
        "the validator should be a weak ETag: {tag}"
    );
    assert!(
        !body.is_null(),
        "the first (unconditional) read carries the full body"
    );

    // Revalidating with that exact tag is a bodyless 304 that still carries the
    // validator and the freshness policy, so the cache can extend its entry.
    let (status, headers, body) = send(&app, get_if_none_match("/api/games", &tag)).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(body.is_null(), "a 304 must not carry a body");
    assert_eq!(etag(&headers).as_deref(), Some(tag.as_str()));
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "a 304 still advertises the cache policy"
    );
}

#[tokio::test]
async fn if_none_match_star_revalidates_to_304() {
    // `*` matches any current representation of a cacheable resource.
    let app = test_app().await;
    let (status, _headers, body) = send(&app, get_if_none_match("/api/games", "*")).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(body.is_null());
}

#[tokio::test]
async fn a_stale_if_none_match_gets_the_full_body() {
    // A validator the client no longer holds (or never did) must not suppress the
    // body: the response is a normal 200 carrying the current ETag.
    let app = test_app().await;
    let stale = "W/\"00000000000000000000000000000000\"";
    let (status, headers, body) = send(&app, get_if_none_match("/api/games", stale)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.is_null());
    assert!(etag(&headers).is_some_and(|t| t != stale));
}

#[tokio::test]
async fn etag_is_content_addressed_so_a_different_page_is_not_suppressed() {
    // The validator is a hash of the body, so it varies with the query. Page 1's tag
    // must revalidate page 1 (304) but never suppress the distinct page 2 (200).
    let app = test_app_with_catalog().await;

    let page1 = "/api/games/mtg/cards?page_size=1&page=1";
    let page2 = "/api/games/mtg/cards?page_size=1&page=2";

    let (status, headers, _) = send(&app, get(page1)).await;
    assert_eq!(status, StatusCode::OK);
    let tag1 = etag(&headers).expect("page 1 carries an ETag");

    let (status, _, _) = send(&app, get_if_none_match(page1, &tag1)).await;
    assert_eq!(
        status,
        StatusCode::NOT_MODIFIED,
        "same body revalidates to 304"
    );

    let (status, headers, body) = send(&app, get_if_none_match(page2, &tag1)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a different body must not 304 on page 1's tag"
    );
    assert!(!body.is_null());
    assert_ne!(etag(&headers).as_deref(), Some(tag1.as_str()));
}

#[tokio::test]
async fn sitemaps_also_revalidate_to_304() {
    // Sitemaps are shared-cacheable (no `immutable`), so crawlers revalidating them
    // get a cheap 304 too.
    let app = test_app_with_catalog().await;

    let (status, headers, body) = send_text(&app, get("/api/sitemap.xml")).await;
    assert_eq!(status, StatusCode::OK);
    let tag = etag(&headers).expect("the sitemap carries an ETag");
    assert!(!body.is_empty());

    let (status, headers, body) =
        send_text(&app, get_if_none_match("/api/sitemap.xml", &tag)).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(body.is_empty(), "a 304 sitemap carries no body");
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::sitemap::SITEMAP_CACHE_CONTROL)
    );
}

#[tokio::test]
async fn no_store_and_error_responses_never_carry_an_etag() {
    let app = test_app().await;

    // Per-user auth is on the private (no-store) router — never even reaches the
    // ETag layer.
    let (_, headers, _) = send(&app, get("/api/auth/me")).await;
    assert_eq!(
        etag(&headers),
        None,
        "auth responses must not be validated/cached"
    );

    // The live import-status route is no-store.
    let (_, headers, _) = send(&app, get("/api/games/mtg/status")).await;
    assert_eq!(etag(&headers), None);

    // A public 404 is no-store, so the ETag layer skips it (no validator to pin a
    // transient negative result).
    let (status, headers, _) = send(&app, get("/api/games/mtg/sets/does-not-exist")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(etag(&headers), None);
}

#[tokio::test]
async fn head_requests_carry_the_cache_policy_but_no_etag() {
    // axum serves HEAD off the same GET handler but strips the body, so the ETag
    // layer (which would have to hash that body) intentionally skips non-GET: a HEAD
    // still carries the freshness policy, just no validator.
    let app = test_app().await;
    let (status, headers, body) = send(&app, head("/api/games")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_null(), "a HEAD response has no body");
    assert_eq!(
        cache_control(&headers),
        Some(crate::handlers::cache::PUBLIC_CATALOG_CACHE),
        "HEAD still advertises the cache policy"
    );
    assert_eq!(
        etag(&headers),
        None,
        "HEAD carries no ETag (the body is stripped)"
    );
}

#[tokio::test]
async fn a_conditional_request_on_a_404_is_not_a_spurious_304() {
    // An `If-None-Match` on a route that errors must still 404 — the client's stale
    // validator can't be turned into a 304 for a body that no longer exists.
    let app = test_app().await;
    let (status, headers, _) = send(
        &app,
        get_if_none_match("/api/games/mtg/sets/does-not-exist", "*"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(etag(&headers), None);
}

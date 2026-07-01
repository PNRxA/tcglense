//! Cache-Control policy (CDN / browser caching).
//!
//! A shared cache (CDN) must be able to cache the public catalog reads (they are
//! the same for everyone and change at most daily) while never storing per-user
//! auth responses, the live import-status signal, or error responses. These drive
//! the real router so the route-group wiring in `build_router` is covered, not just
//! the pure policy in `handlers::cache`.

use super::harness::*;

/// The `Cache-Control` header value as a string, or `None` if absent.
fn cache_control(headers: &HeaderMap) -> Option<&str> {
    headers.get(CACHE_CONTROL).and_then(|v| v.to_str().ok())
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

    // A successful login carries an access token + Set-Cookie: also no-store.
    let email = "cache-nostore@example.com";
    let (status, headers, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": email, "password": "correct horse battery" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
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

//! Pagination-parameter hardening — hostile `?page`/`?page_size` values must
//! yield an ordinary (empty) page, never a panicked handler task.

use super::harness::*;

/// Regression: an unclamped `?page` used to ride into SeaORM's
/// `offset = page * page_size` u64 multiply — an overflow panic in a debug
/// build (the connection just drops, no response) and a wrapped, bogus offset
/// in release. Clamped, it behaves like any other past-the-end page.
#[tokio::test]
async fn huge_page_param_is_an_empty_page_not_a_dropped_connection() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // The public catalog lists (both paginate in SQL via fetch_page)…
    for uri in [
        format!("/api/games/{game}/cards?page={}&page_size=200", u64::MAX),
        format!("/api/games/{game}/sets/dmb/cards?page={}", u64::MAX),
    ] {
        let (status, _, body) = send(&app, get(&uri)).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body:?}");
        assert!(
            body["data"].as_array().is_some_and(Vec::is_empty),
            "{uri} should be past the end: {body:?}"
        );
        assert_eq!(body["has_more"].as_bool(), Some(false), "{uri}: {body:?}");
    }

    // …and the authenticated collection + wish-list lists (cards + sealed products), which
    // share resolve_page.
    let (token, _) = register(&app, "pager@example.com", "password123").await;
    for uri in [
        format!("/api/collection/{game}?page={}", u64::MAX),
        format!("/api/wishlist/{game}?page={}", u64::MAX),
        format!("/api/wishlist/{game}/products?page={}", u64::MAX),
    ] {
        let (status, _, body) = send(&app, get_with_bearer(&uri, &token)).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body:?}");
        assert!(
            body["data"].as_array().is_some_and(Vec::is_empty),
            "{body:?}"
        );
        assert_eq!(body["has_more"].as_bool(), Some(false), "{body:?}");
    }
}

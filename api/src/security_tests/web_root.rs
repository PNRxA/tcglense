//! `WEB_ROOT` static-SPA fallback — the seam the single-process "combined" Docker
//! image relies on. When `WEB_ROOT` is set the API serves the built SPA from disk
//! for any request the `/api` routes don't match, with `index.html` as the fallback
//! for client-side routes. These drive the real router to pin the contract: `/api`
//! still answers JSON; real static files are served; a deep-linked SPA route serves
//! `index.html` with a **200** (not the 404 `not_found_service` would force); and an
//! unset `WEB_ROOT` leaves the API `/api`-only (a normal 404 for other paths), so
//! existing API-only deployments are unaffected.

use std::fs;
use std::path::PathBuf;

use super::harness::*;
use crate::{build_router, config::Config, state::AppState};

const INDEX_HTML: &str = "<!doctype html><html><head><title>TCGLense SPA</title></head>\
    <body><div id=\"app\"></div></body></html>";

/// Create a throwaway web-root dir (an index.html + a hashed asset) under the temp
/// dir. Each test uses a distinct name; a stale dir from a prior run is cleared.
fn make_web_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tcglense-webroot-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("assets")).expect("create web root");
    fs::write(dir.join("index.html"), INDEX_HTML).expect("write index.html");
    fs::write(dir.join("assets/app-abc123.js"), "console.log('spa')").expect("write asset");
    dir
}

/// Build the real router over a fresh migrated state whose config has `web_root` set.
async fn app_with_web_root(web_root: Option<PathBuf>) -> Router {
    let db = crate::test_support::migrated_memory_db().await;
    let config = Config {
        web_root,
        ..crate::test_support::test_config()
    };
    let http = reqwest::Client::builder().build().expect("http client");
    let image_http = reqwest::Client::builder().build().expect("image client");
    let state = AppState::new(config, db, http, image_http, None).expect("assemble app state");
    build_router(state)
}

#[tokio::test]
async fn api_routes_still_answer_when_web_root_is_set() {
    let app = app_with_web_root(Some(make_web_root("api-still-works"))).await;
    let (status, _headers, body) = send(&app, get("/api/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok" }));
}

#[tokio::test]
async fn root_serves_the_spa_index_html() {
    let app = app_with_web_root(Some(make_web_root("root-index"))).await;
    let (status, headers, body) = send_text(&app, get("/")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers.get("cache-control").and_then(|v| v.to_str().ok()),
        Some("public, no-cache")
    );
    assert!(
        body.contains("id=\"app\""),
        "expected the SPA index.html, got: {body}"
    );
}

#[tokio::test]
async fn a_static_asset_is_served_from_disk() {
    let app = app_with_web_root(Some(make_web_root("asset"))).await;
    let (status, headers, body) = send_text(&app, get("/assets/app-abc123.js")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers.get("cache-control").and_then(|v| v.to_str().ok()),
        Some("public, max-age=31536000, immutable")
    );
    assert!(body.contains("console.log('spa')"));
}

#[tokio::test]
async fn a_deep_spa_route_falls_back_to_index_with_200() {
    // The crux of this feature: a client-side route the API doesn't know must serve
    // index.html with a real 200 (not a 404) so deep links are valid, crawlable pages.
    let app = app_with_web_root(Some(make_web_root("spa-deep-link"))).await;
    let (status, headers, body) = send_text(&app, get("/collection/mtg/sets/abc")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an SPA deep link must be 200, not 404"
    );
    assert_eq!(
        headers.get("cache-control").and_then(|v| v.to_str().ok()),
        Some("public, no-cache")
    );
    assert!(
        body.contains("id=\"app\""),
        "expected the index.html fallback, got: {body}"
    );
}

#[tokio::test]
async fn unknown_api_paths_stay_a_json_404_not_the_spa() {
    // The SPA fallback must not swallow unknown /api routes: an API path with no
    // handler stays a real JSON 404 (via the lowest-priority /api catch-all), matching
    // the split deployment — not a 200 of the SPA's index.html.
    let app = app_with_web_root(Some(make_web_root("api-404"))).await;
    let (status, _headers, body) = send(&app, get("/api/definitely-not-a-route")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // `send` parses the body as JSON; the SPA's HTML would deserialize to `Null`, so a
    // present `error` field proves this is the API's JSON error, not the SPA fallback.
    assert!(
        body.get("error").is_some(),
        "expected a JSON error body, got: {body}"
    );
}

#[tokio::test]
async fn without_web_root_unmatched_routes_stay_404() {
    // Default (WEB_ROOT unset): the API serves only /api and 404s everything else,
    // so no fallback service is wired and existing deployments are unaffected.
    let app = app_with_web_root(None).await;
    let (status, _headers, _body) = send_text(&app, get("/collection/mtg/sets/abc")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

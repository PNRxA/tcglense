//! Dataset mirror (`/api/mirror/*`) — the endpoints that re-serve the raw provider
//! datasets so other TCGLense instances can pull them from here (see
//! [`crate::handlers::mirror`]). These pin the two contracts a unit test can't see on
//! the wire: the routes are **absent unless `MIRROR_ENABLED` is set** (an ordinary
//! self-host isn't an open proxy), and, when enabled, the input-validation gates reject
//! a malformed request **before** any upstream fetch (so these assertions need no
//! network — the happy path, which would hit Scryfall/MTGJSON/TCGCSV live, is exercised
//! by the provider unit tests + the `datasets` URL-resolution tests instead).

use std::path::PathBuf;

use super::harness::*;
use crate::{build_router, config::Config, state::AppState};

/// Build the real router over a fresh migrated state with `mirror_enabled` (and,
/// optionally, `web_root`) set. `web_root` need not exist on disk — `ServeDir` is
/// constructed lazily, so this exercises router *assembly* (where a route conflict
/// between the mirror's `/api/mirror/tcgcsv/{*path}` and the web-root `/api/{*rest}`
/// catch-all would panic) without any filesystem setup.
async fn app_with_mirror_and_web_root(enabled: bool, web_root: Option<PathBuf>) -> Router {
    let db = crate::test_support::migrated_memory_db().await;
    let config = Config {
        mirror_enabled: enabled,
        web_root,
        ..crate::test_support::test_config()
    };
    let http = reqwest::Client::builder().build().expect("http client");
    let image_http = reqwest::Client::builder().build().expect("image client");
    let state = AppState::new(config, db, http, image_http, None).expect("assemble app state");
    build_router(state)
}

/// Build the real router over a fresh migrated state with `mirror_enabled` set as given.
async fn app_with_mirror(enabled: bool) -> Router {
    app_with_mirror_and_web_root(enabled, None).await
}

#[tokio::test]
async fn mirror_routes_are_absent_by_default() {
    // MIRROR_ENABLED defaults off, so none of the mirror routes are registered — a
    // self-host doesn't become an open proxy to the upstream services. Each is a plain
    // 404 (no fallback service is wired). No network is touched.
    let app = app_with_mirror(false).await;
    for path in [
        "/api/mirror/scryfall/bulk-data",
        "/api/mirror/scryfall/sets",
        "/api/mirror/scryfall/file/default_cards",
        "/api/mirror/mtgjson/AllPrintings.json.gz",
        "/api/mirror/tcgcsv/last-updated.txt",
    ] {
        let (status, _headers, _body) = send(&app, get(path)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path} should be absent");
    }
}

#[tokio::test]
async fn enabled_mirror_rejects_a_bad_scryfall_dataset_kind_without_fetching() {
    // The `{kind}` slug is validated before the catalog is fetched, so an illegal kind
    // (uppercase / traversal chars) is a 404 with no upstream call.
    let app = app_with_mirror(true).await;
    for kind in ["BAD", "..", "all-cards"] {
        let (status, _headers, body) =
            send(&app, get(&format!("/api/mirror/scryfall/file/{kind}"))).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "kind {kind:?} should be refused"
        );
        assert!(
            body.get("error").is_some(),
            "expected a JSON error for {kind:?}"
        );
    }
}

#[tokio::test]
async fn enabled_mirror_rejects_tcgcsv_path_traversal_without_fetching() {
    // The `{*path}` capture is sanitised before the upstream fetch, so a traversal /
    // host-escape attempt is a 404 and never becomes an outbound request.
    let app = app_with_mirror(true).await;
    for path in [
        "/api/mirror/tcgcsv/../secret",
        "/api/mirror/tcgcsv/tcgplayer/../../etc/passwd",
    ] {
        let (status, _headers, body) = send(&app, get(path)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path} should be refused");
        assert!(
            body.get("error").is_some(),
            "expected a JSON error for {path}"
        );
    }
}

#[tokio::test]
async fn mirror_routes_coexist_with_the_web_root_catch_all() {
    // The combined "combined image" posture: MIRROR_ENABLED + WEB_ROOT both set. The
    // mirror's `/api/mirror/tcgcsv/{*path}` catch-all and the web-root's `/api/{*rest}`
    // catch-all must coexist without a router-build panic, and the specific mirror
    // routes must still win over the SPA fallback (a bad kind is the mirror's own JSON
    // 404, not the SPA's index.html). `build_router` succeeding is itself the assertion
    // that the two catch-alls don't conflict.
    let app =
        app_with_mirror_and_web_root(true, Some(PathBuf::from("/nonexistent/web/root"))).await;
    let (status, _headers, body) = send(&app, get("/api/mirror/scryfall/file/BAD")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body.get("error").is_some(),
        "a bad mirror kind must stay the mirror's JSON 404, not the SPA fallback"
    );
}

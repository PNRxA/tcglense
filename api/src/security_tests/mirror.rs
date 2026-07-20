//! Dataset mirror (`/api/mirror/*`) — the endpoints that re-serve the raw provider
//! datasets so other TCGLense instances can pull them from here (see
//! [`crate::handlers::mirror`]). These pin the two contracts a unit test can't see on
//! the wire: the routes are **absent unless `MIRROR_ENABLED` is set** (an ordinary
//! self-host isn't an open proxy), and, when enabled, the input-validation gates reject
//! a malformed request **before** any upstream fetch (so these assertions need no
//! network — the happy path, which would hit Scryfall/MTGJSON/TCGCSV live, is exercised
//! by the provider unit tests + the `datasets` URL-resolution tests instead).

use std::path::PathBuf;

use axum::http::header::{ETAG, IF_NONE_MATCH};

use super::harness::*;
use crate::{
    build_router,
    catalog::{fingerprint_sync, fingerprints},
    config::Config,
    state::AppState,
};

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
        "/api/mirror/scryfall/sld-drops",
        "/api/mirror/mtgjson/AllPrintings.json.gz",
        "/api/mirror/tcgcsv/last-updated.txt",
        "/api/mirror/fingerprints/mtg",
        "/api/mirror/currency",
    ] {
        let (status, _headers, _body) = send(&app, get(path)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path} should be absent");
    }
}

#[tokio::test]
async fn app_state_points_currency_rates_at_the_mirror_for_a_consumer() {
    // The load-bearing wiring for pulling FX rates from the main server: `AppState::new`
    // must build the rate cache via `CurrencyRates::from_config`, so a default consumer
    // reads rates from `{DATASET_MIRROR_URL}/api/mirror/currency` rather than contacting the
    // upstream provider. A silent revert to `CurrencyRates::default()` would reintroduce a
    // direct provider fetch with no other test failing — this pins it. No network is touched.
    let db = crate::test_support::migrated_memory_db().await;
    let config = Config {
        mirror_enabled: false,
        sync_from_upstream: false,
        dataset_mirror_url: "https://mirror.example".to_string(),
        ..crate::test_support::test_config()
    };
    let http = reqwest::Client::builder().build().expect("http client");
    let image_http = reqwest::Client::builder().build().expect("image client");
    let state = AppState::new(config, db, http, image_http, None).expect("assemble app state");
    assert_eq!(
        state.currency_rates.rates_url(),
        "https://mirror.example/api/mirror/currency",
        "a consumer's AppState must resolve FX rates through the dataset mirror"
    );
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
async fn enabled_mirror_serves_the_sld_drop_snapshot() {
    // Served straight from this origin's in-memory drop store (no upstream) — the committed
    // fallback snapshot, since no scrape has run — as JSON carrying a strong content ETag, so
    // other instances import it instead of each scraping Scryfall's gallery.
    let app = app_with_mirror(true).await;
    let (status, headers, body) = send(&app, get("/api/mirror/scryfall/sld-drops")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("application/json"));
    assert!(
        headers.get(ETAG).is_some(),
        "the snapshot must carry an ETag"
    );
    // The body is a drop snapshot: an object with a non-empty `sets` array (the shipped fallback
    // covers the Secret Lair set).
    let sets = body
        .get("sets")
        .and_then(|v| v.as_array())
        .expect("snapshot has a sets array");
    assert!(!sets.is_empty(), "the fallback snapshot lists the sld set");
}

#[tokio::test]
async fn sld_drop_snapshot_honours_a_conditional_request() {
    // A consumer that already has the current snapshot sends its stored ETag and gets a bodyless
    // 304 — the mechanism that keeps an unchanged snapshot off the wire.
    let app = app_with_mirror(true).await;
    let (status, headers, _) = send(&app, get("/api/mirror/scryfall/sld-drops")).await;
    assert_eq!(status, StatusCode::OK);
    let etag = headers
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .expect("etag")
        .to_string();

    let req = Request::builder()
        .method("GET")
        .uri("/api/mirror/scryfall/sld-drops")
        .header(IF_NONE_MATCH, &etag)
        .body(Body::empty())
        .unwrap();
    let (status, headers, body) = send(&app, req).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(body.is_null(), "a 304 carries no body");
    assert_eq!(
        headers.get(ETAG).and_then(|v| v.to_str().ok()),
        Some(etag.as_str())
    );
}

/// Build the real router with the mirror enabled and one fingerprint seeded into the
/// live match index, so the fingerprint export endpoint has a real payload to serve.
async fn app_with_fingerprint(external_id: &str, hash: &[u8; 32]) -> Router {
    let db = crate::test_support::migrated_memory_db().await;
    let config = Config {
        mirror_enabled: true,
        ..crate::test_support::test_config()
    };
    let http = reqwest::Client::builder().build().expect("http client");
    let image_http = reqwest::Client::builder().build().expect("image client");
    let state = AppState::new(config, db, http, image_http, None).expect("assemble app state");
    fingerprints::upsert(
        &state.db,
        "mtg",
        external_id,
        0,
        1,
        hash,
        "small",
        "src-hash",
    )
    .await
    .expect("seed fingerprint");
    let index = fingerprints::load_index(&state.db, 1)
        .await
        .expect("load index");
    state.set_fingerprint_index(index);
    build_router(state)
}

#[tokio::test]
async fn enabled_mirror_serves_a_parseable_fingerprint_payload() {
    // The export is served straight from the in-memory index (no upstream), as the
    // compact binary the import path parses — so this round-trips through the real parser.
    let hash = [9u8; 32];
    let app = app_with_fingerprint("card-uuid-1", &hash).await;
    let (status, headers, body) = send_bytes(&app, get("/api/mirror/fingerprints/mtg")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("application/octet-stream"));
    assert!(
        headers.get(ETAG).is_some(),
        "the payload must carry an ETag"
    );
    let parsed = fingerprint_sync::parse(&body).expect("payload parses");
    assert_eq!(parsed.algo_version, 1);
    assert_eq!(parsed.rows.len(), 1);
    assert_eq!(parsed.rows[0].external_id, "card-uuid-1");
    assert_eq!(parsed.rows[0].hash, hash);
}

#[tokio::test]
async fn fingerprint_index_honours_a_conditional_request() {
    // A consumer that already has the current index sends its stored ETag and gets a
    // bodyless 304 — the mechanism that keeps an unchanged index off the wire.
    let app = app_with_fingerprint("card-uuid-1", &[9u8; 32]).await;
    let (status, headers, _) = send_bytes(&app, get("/api/mirror/fingerprints/mtg")).await;
    assert_eq!(status, StatusCode::OK);
    let etag = headers
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .expect("etag")
        .to_string();

    let req = Request::builder()
        .method("GET")
        .uri("/api/mirror/fingerprints/mtg")
        .header(IF_NONE_MATCH, &etag)
        .body(Body::empty())
        .unwrap();
    let (status, headers, body) = send_bytes(&app, req).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(body.is_empty(), "a 304 carries no body");
    assert_eq!(
        headers.get(ETAG).and_then(|v| v.to_str().ok()),
        Some(etag.as_str())
    );
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

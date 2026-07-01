//! HTTP-level security tests.
//!
//! These drive the *real* application router (built by [`crate::build_router`],
//! including CORS, the JSON-body extractor, error mapping, and the auth stack)
//! in-process via `tower`'s `oneshot` — no TCP bind, no network. They assert the
//! security-relevant behaviour a unit test of any single module can't see on its
//! own: end-to-end refresh-token rotation + reuse detection, generic login
//! failures (no user enumeration), the hardened refresh cookie on the wire,
//! correct status codes for malformed bodies, the CORS contract, and that secret
//! material (password hashes) never leaks into a response.

use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{
        HeaderMap, Request, StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_ORIGIN,
            ACCESS_CONTROL_REQUEST_METHOD, AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, COOKIE,
            ORIGIN, SET_COOKIE,
        },
    },
};
use sea_orm::{ConnectOptions, Database};
use sea_orm_migration::MigratorTrait;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::{
    build_router, catalog::images::ImageCache, config::Config, migrator::Migrator,
    state::AppState,
};

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

/// Build an [`AppState`] backed by a fresh in-memory SQLite DB (migrated), a real
/// validated config, and a real precomputed timing-equalizer hash. No card sync.
async fn test_state() -> AppState {
    // Pin the pool to a single connection. With `sqlite::memory:` every physical
    // connection is its own separate, empty database, so a multi-connection pool
    // could hand a request an unmigrated DB; one connection keeps the migrated
    // schema + data consistent across every request (and any future concurrent one).
    let mut opts = ConnectOptions::new("sqlite::memory:");
    opts.max_connections(1).min_connections(1);
    let db = Database::connect(opts)
        .await
        .expect("connect in-memory sqlite");
    Migrator::up(&db, None).await.expect("run migrations");

    let config = Config {
        database_url: "sqlite::memory:".to_string(),
        jwt_secret: "integration-test-signing-secret-0123456789".to_string(),
        access_token_expiry_minutes: 15,
        refresh_token_expiry_days: 30,
        cookie_secure: false,
        host: "127.0.0.1".to_string(),
        port: 8080,
        // A distinctive origin so the sitemap tests can assert the <loc>s are built
        // against the configured public site URL.
        public_site_url: "https://sitemap.test".to_string(),
        data_dir: std::env::temp_dir().join("tcglense-security-tests"),
        scryfall_user_agent: "TCGLense/test".to_string(),
        sync_on_startup: false,
        sync_interval_hours: 24,
        seed_dummy_data: false,
    };

    let dummy_password_hash: Arc<str> = crate::auth::password::hash_password("timing-equalizer")
        .expect("hash dummy password")
        .into();
    let image_dir = config.data_dir.join("images");
    let image_http = reqwest::Client::builder()
        .build()
        .expect("build image client");

    AppState {
        db,
        config: Arc::new(config),
        dummy_password_hash,
        images: Arc::new(ImageCache::new(image_dir, image_http)),
    }
}

/// A router over a fresh, empty (no catalog) state.
async fn test_app() -> Router {
    build_router(test_state().await)
}

/// A router whose DB has the deterministic offline dummy catalog seeded, so the
/// public catalog/search routes have data to exercise.
async fn test_app_with_catalog() -> Router {
    let state = test_state().await;
    crate::catalog::seed_all(&state.db).await;
    build_router(state)
}

/// Drive one request through the router (clones it, since `oneshot` consumes the
/// service) and return `(status, headers, json_body)`. A non-JSON / empty body
/// comes back as `Value::Null`.
async fn send(app: &Router, req: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let res = app
        .clone()
        .oneshot(req)
        .await
        .expect("router is infallible");
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, headers, json)
}

/// Like [`send`] but returns the raw body as a UTF-8 string, for the XML sitemap
/// routes whose bodies aren't JSON.
async fn send_text(app: &Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let res = app
        .clone()
        .oneshot(req)
        .await
        .expect("router is infallible");
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("read response body");
    (status, headers, String::from_utf8_lossy(&bytes).into_owned())
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn json_post(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn post_with_cookie(uri: &str, refresh_token: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(COOKIE, format!("tcglense_refresh={refresh_token}"))
        .body(Body::empty())
        .unwrap()
}

fn get_with_bearer(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

/// The plaintext of the freshly-set refresh cookie, if any (ignores a cleared one).
fn refresh_token_from(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|s| {
            let rest = s.strip_prefix("tcglense_refresh=")?;
            let value = rest.split(';').next().unwrap_or("");
            (!value.is_empty()).then(|| value.to_string())
        })
}

/// Whether the response clears the refresh cookie (empty-valued `Set-Cookie`).
fn refresh_cookie_cleared(headers: &HeaderMap) -> bool {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|s| {
            s.strip_prefix("tcglense_refresh=")
                .map(|rest| rest.is_empty() || rest.starts_with(';'))
                .unwrap_or(false)
        })
}

/// Register a user and return its access token and refresh-cookie plaintext.
async fn register(app: &Router, email: &str, password: &str) -> (String, String) {
    let (status, headers, body) = send(
        app,
        json_post(
            "/api/auth/register",
            json!({ "email": email, "password": password }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "register failed: {body:?}");
    let access = body["access_token"].as_str().expect("access_token").to_string();
    let refresh = refresh_token_from(&headers).expect("refresh cookie");
    (access, refresh)
}

// ---------------------------------------------------------------------------
// Registration / response hygiene
// ---------------------------------------------------------------------------

#[tokio::test]
async fn register_hardens_cookie_and_never_leaks_password_hash() {
    let app = test_app().await;
    let (status, headers, body) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "User@Example.COM", "password": "password123", "display_name": "Tester" }),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(body["access_token"].as_str().is_some());
    // Email is canonicalised (trimmed + lowercased).
    assert_eq!(body["user"]["email"], "user@example.com");
    assert_eq!(body["user"]["display_name"], "Tester");

    // The public user shape must never carry secret material.
    let raw = body.to_string();
    assert!(!raw.contains("password_hash"), "leaked field name: {raw}");
    assert!(!raw.contains("$argon2"), "leaked a hash: {raw}");

    // The long-lived refresh token must ride ONLY in Set-Cookie (httpOnly), never
    // echoed into the JSON body where JS could read it.
    let refresh = refresh_token_from(&headers).expect("refresh cookie set");
    assert!(
        !raw.contains(&refresh),
        "the refresh token must not appear in the response body"
    );

    // The refresh cookie rides only in Set-Cookie, hardened.
    let set_cookie = headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|s| s.starts_with("tcglense_refresh="))
        .expect("refresh Set-Cookie");
    let lower = set_cookie.to_ascii_lowercase();
    assert!(lower.contains("httponly"), "{set_cookie}");
    assert!(lower.contains("samesite=lax"), "{set_cookie}");
    assert!(set_cookie.contains("Path=/api/auth"), "{set_cookie}");
}

#[tokio::test]
async fn register_rejects_invalid_email_and_weak_password() {
    let app = test_app().await;

    let (s1, _, b1) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "no-at-sign", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(s1, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(b1["error"].as_str().is_some());

    let (s2, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "ok@example.com", "password": "short" }),
        ),
    )
    .await;
    assert_eq!(s2, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn duplicate_email_is_conflict_case_insensitively() {
    let app = test_app().await;
    register(&app, "Dup@Example.com", "password123").await;

    // Same address, different casing — the case-insensitive account must collide.
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "dup@EXAMPLE.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn case_insensitive_uniqueness_is_enforced_at_the_database() {
    use crate::entities::user;
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, Set};

    // Defense-in-depth beyond the handler's lowercasing: insert two rows that
    // differ only in email case *directly* via the entity (bypassing the handler),
    // and require the COLLATE NOCASE unique index to reject the second.
    let state = test_state().await;
    let now = Utc::now();
    let row = |email: &str| user::ActiveModel {
        email: Set(email.to_string()),
        password_hash: Set("irrelevant".to_string()),
        display_name: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    row("collate@example.com")
        .insert(&state.db)
        .await
        .expect("first insert succeeds");
    let second = row("Collate@Example.com").insert(&state.db).await;
    assert!(
        second.is_err(),
        "a case-variant email must violate the unique index"
    );
}

#[tokio::test]
async fn oversized_credentials_are_rejected_before_hashing() {
    let app = test_app().await;

    // Register: a password past the 1024-char cap is a cheap-to-send /
    // expensive-to-hash Argon2 DoS -> 422.
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": "big@example.com", "password": "a".repeat(1025) }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // Register: an email past the 254-char cap -> 422.
    let long_email = format!("{}@example.com", "a".repeat(250));
    assert!(long_email.len() > 254);
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/register",
            json!({ "email": long_email, "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);

    // Login: an oversized password must short-circuit to 422 *before* Argon2 runs,
    // rather than being hashed against the (dummy or real) verifier.
    register(&app, "victim@example.com", "password123").await;
    let (s, _, _) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "victim@example.com", "password": "a".repeat(5000) }),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
}

// ---------------------------------------------------------------------------
// Login — generic failures, no user enumeration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn login_succeeds_and_failures_are_generic() {
    let app = test_app().await;
    register(&app, "login@example.com", "password123").await;

    let (ok_status, ok_headers, ok_body) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "login@example.com", "password": "password123" }),
        ),
    )
    .await;
    assert_eq!(ok_status, StatusCode::OK);
    assert!(ok_body["access_token"].as_str().is_some());
    assert!(refresh_token_from(&ok_headers).is_some());

    let (wrong_pw_status, _, wrong_pw_body) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "login@example.com", "password": "wrong-password" }),
        ),
    )
    .await;
    let (no_user_status, _, no_user_body) = send(
        &app,
        json_post(
            "/api/auth/login",
            json!({ "email": "ghost@example.com", "password": "password123" }),
        ),
    )
    .await;

    // Both 401, and the message is identical — no oracle for "does this user exist".
    assert_eq!(wrong_pw_status, StatusCode::UNAUTHORIZED);
    assert_eq!(no_user_status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_pw_body["error"], "invalid email or password");
    assert_eq!(wrong_pw_body["error"], no_user_body["error"]);
}

// ---------------------------------------------------------------------------
// Bearer-protected route
// ---------------------------------------------------------------------------

#[tokio::test]
async fn me_requires_a_valid_bearer_token() {
    let app = test_app().await;
    let (access, _) = register(&app, "me@example.com", "password123").await;

    let (ok_status, _, ok_body) = send(&app, get_with_bearer("/api/auth/me", &access)).await;
    assert_eq!(ok_status, StatusCode::OK);
    assert_eq!(ok_body["user"]["email"], "me@example.com");

    // Missing header.
    let (missing, _, _) = send(&app, get("/api/auth/me")).await;
    assert_eq!(missing, StatusCode::UNAUTHORIZED);

    // Malformed scheme.
    let (malformed, _, _) = send(
        &app,
        Request::builder()
            .uri("/api/auth/me")
            .header(AUTHORIZATION, "Token abc")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(malformed, StatusCode::UNAUTHORIZED);

    // Garbage / forged token.
    let (garbage, _, _) = send(&app, get_with_bearer("/api/auth/me", "not.a.jwt")).await;
    assert_eq!(garbage, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// Refresh rotation + reuse detection (the full HTTP lifecycle)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refresh_rotates_single_use_and_detects_token_theft() {
    let app = test_app().await;
    let (_, t1) = register(&app, "rotate@example.com", "password123").await;

    // t1 -> t2: success, new access token, rotated cookie.
    let (s, h2, b2) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(b2["access_token"].as_str().is_some());
    let t2 = refresh_token_from(&h2).expect("rotated cookie t2");
    assert_ne!(t1, t2, "rotation must mint a new token");

    // Replaying t1 now (its successor t2 is still active) is a benign double-submit:
    // rejected and the cookie cleared, but the family is NOT burned.
    let (s, h, _) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(refresh_cookie_cleared(&h), "failed refresh must clear the cookie");

    // t2 still works -> t3 (proves the family survived the benign replay).
    let (s, h3, _) = send(&app, post_with_cookie("/api/auth/refresh", &t2)).await;
    assert_eq!(s, StatusCode::OK);
    let t3 = refresh_token_from(&h3).expect("rotated cookie t3");

    // Now replay t1 again: its successor t2 has itself been revoked, so this is
    // unambiguous theft — the whole family is burned.
    let (s, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // The live t3 is now dead too.
    let (s, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &t3)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn refresh_without_a_cookie_is_unauthorized_and_mints_nothing() {
    let app = test_app().await;
    let (status, headers, _) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/refresh")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // With no cookie there is nothing to clear, but a failed refresh must never
    // hand back a usable refresh token.
    assert!(refresh_token_from(&headers).is_none());
}

#[tokio::test]
async fn logout_revokes_the_refresh_token_and_is_idempotent() {
    let app = test_app().await;
    let (_, t1) = register(&app, "logout@example.com", "password123").await;

    let (status, headers, _) = send(&app, post_with_cookie("/api/auth/logout", &t1)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(refresh_cookie_cleared(&headers));

    // The revoked token can no longer be exchanged.
    let (status, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &t1)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Logout with no cookie is still a clean 204.
    let (status, _, _) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/logout")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

// ---------------------------------------------------------------------------
// Request-body handling — correct status + JSON error shape
// ---------------------------------------------------------------------------

#[tokio::test]
async fn malformed_bodies_map_to_correct_status_with_json_errors() {
    let app = test_app().await;

    // Syntactically invalid JSON -> 400.
    let (bad_json, _, bad_json_body) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from("{ not valid json"))
            .unwrap(),
    )
    .await;
    assert_eq!(bad_json, StatusCode::BAD_REQUEST);
    assert!(bad_json_body["error"].as_str().is_some());

    // Missing Content-Type -> 415.
    let (no_ct, _, _) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(no_ct, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // Wrong Content-Type -> 415.
    let (wrong_ct, _, _) = send(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/auth/login")
            .header(CONTENT_TYPE, "text/plain")
            .body(Body::from("hello"))
            .unwrap(),
    )
    .await;
    assert_eq!(wrong_ct, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // Valid JSON, wrong schema (missing password) -> 422.
    let (schema, _, schema_body) = send(
        &app,
        json_post("/api/auth/login", json!({ "email": "a@b.com" })),
    )
    .await;
    assert_eq!(schema, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(schema_body["error"].as_str().is_some());
}

// ---------------------------------------------------------------------------
// CORS contract
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cors_preflight_allows_dev_origin_with_credentials() {
    let app = test_app().await;
    let (status, headers, _) = send(
        &app,
        Request::builder()
            .method("OPTIONS")
            .uri("/api/auth/login")
            .header(ORIGIN, "http://localhost:5173")
            .header(ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert!(status.is_success(), "preflight status was {status}");
    let allow_origin = headers
        .get(ACCESS_CONTROL_ALLOW_ORIGIN)
        .and_then(|v| v.to_str().ok());
    // Echoes the explicit origin (never the wildcard, which is illegal with creds).
    assert_eq!(allow_origin, Some("http://localhost:5173"));
    assert_ne!(allow_origin, Some("*"));
    assert_eq!(
        headers
            .get(ACCESS_CONTROL_ALLOW_CREDENTIALS)
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );
}

#[tokio::test]
async fn cors_does_not_authorize_foreign_or_near_miss_origins() {
    let app = test_app().await;
    // A near-miss (right host, wrong port) and a look-alike host prove the
    // allow-list is an exact match, not a prefix/substring one.
    for origin in [
        "https://evil.example.com",
        "http://localhost:5174",
        "http://localhost.evil.com",
    ] {
        let (_, headers, _) = send(
            &app,
            Request::builder()
                .method("GET")
                .uri("/api/health")
                .header(ORIGIN, origin)
                .body(Body::empty())
                .unwrap(),
        )
        .await;

        // The allow-list is a single pinned origin, so `Access-Control-Allow-Origin`
        // is ALWAYS that exact value — never the requesting (foreign) origin and
        // never `*`. The browser compares its origin to this header and blocks the
        // cross-origin response on the mismatch. Asserting it stays pinned also
        // catches a regression to `mirror_request` or a widened allow-list (either
        // would echo the foreign origin here).
        let allow_origin = headers
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|v| v.to_str().ok());
        assert_eq!(
            allow_origin,
            Some("http://localhost:5173"),
            "ACAO must stay pinned to the one allowed origin for {origin}"
        );
        assert_ne!(allow_origin, Some(origin), "must never echo the foreign origin");
        assert_ne!(allow_origin, Some("*"));
    }
}

#[tokio::test]
async fn health_is_public() {
    let app = test_app().await;
    let (status, _, body) = send(&app, get("/api/health")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

// ---------------------------------------------------------------------------
// Public search route — injection-safe, malformed -> 422 (not 500)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn search_is_injection_safe_and_maps_bad_queries_to_422() {
    let game = crate::scryfall::GAME;
    let app = test_app_with_catalog().await;

    // A baseline listing has data (the seed populated the catalog).
    let (base_status, _, base_body) =
        send(&app, get(&format!("/api/games/{game}/cards?page=1&page_size=5"))).await;
    assert_eq!(base_status, StatusCode::OK);
    let seeded_total = base_body["total"].as_u64().expect("total");
    assert!(seeded_total > 0, "dummy catalog should have seeded cards");

    // An unknown filter is a client error (422), never a 500.
    let (bad_status, _, bad_body) =
        send(&app, get(&format!("/api/games/{game}/cards?q=boguskey:1"))).await;
    assert_eq!(bad_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(bad_body["error"].as_str().is_some());

    // A SQL-injection payload is treated as a harmless literal name search: it
    // returns 200 and, crucially, the cards table is still intact afterwards.
    let injection = "'; DROP TABLE cards;--";
    let encoded: String = url_encode(injection);
    let (inj_status, _, _) =
        send(&app, get(&format!("/api/games/{game}/cards?q={encoded}"))).await;
    assert_eq!(inj_status, StatusCode::OK);

    let (after_status, _, after_body) =
        send(&app, get(&format!("/api/games/{game}/cards?page=1&page_size=5"))).await;
    assert_eq!(after_status, StatusCode::OK);
    assert_eq!(
        after_body["total"].as_u64(),
        Some(seeded_total),
        "the cards table must be untouched by the injection attempt"
    );
}

// ---------------------------------------------------------------------------
// Cache-Control policy (CDN / browser caching)
// ---------------------------------------------------------------------------
//
// A shared cache (CDN) must be able to cache the public catalog reads (they are
// the same for everyone and change at most daily) while never storing per-user
// auth responses, the live import-status signal, or error responses. These drive
// the real router so the route-group wiring in `build_router` is covered, not just
// the pure policy in `handlers::cache`.

/// The `Cache-Control` header value as a string, or `None` if absent.
fn cache_control(headers: &HeaderMap) -> Option<&str> {
    headers.get(CACHE_CONTROL).and_then(|v| v.to_str().ok())
}

/// The `Content-Type` header value as a string, or `None` if absent.
fn content_type(headers: &HeaderMap) -> Option<&str> {
    headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok())
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

// ---------------------------------------------------------------------------
// DB-backed sitemaps (issue #75)
// ---------------------------------------------------------------------------
//
// The sitemap index + child sitemaps advertise the public catalog to crawlers.
// These drive the real router so the route wiring, the XML shape, the configured
// public-site-URL `<loc>`s, and the cache policy are all covered end to end.

use crate::handlers::sitemap::SITEMAP_CACHE_CONTROL;

#[tokio::test]
async fn sitemap_index_lists_child_sitemaps() {
    let app = test_app_with_catalog().await;
    let (status, headers, body) = send_text(&app, get("/api/sitemap.xml")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(content_type(&headers).unwrap().starts_with("application/xml"));
    // A sitemap is expensive to build and changes at most daily, so it gets the
    // longer, shared-cacheable sitemap policy (not the catalog default).
    assert_eq!(cache_control(&headers), Some(SITEMAP_CACHE_CONTROL));

    assert!(body.contains("<sitemapindex"), "not an index doc: {body}");
    // Children are referenced against the configured public site origin.
    assert!(body.contains("<loc>https://sitemap.test/api/sitemaps/pages.xml</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/api/sitemaps/sets.xml</loc>"));
    // The seeded catalog has cards, so there is at least one card chunk.
    assert!(body.contains("<loc>https://sitemap.test/api/sitemaps/cards-1.xml</loc>"));
}

#[tokio::test]
async fn sitemap_pages_covers_static_and_game_routes() {
    let app = test_app_with_catalog().await;
    let (status, headers, body) = send_text(&app, get("/api/sitemaps/pages.xml")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(content_type(&headers).unwrap().starts_with("application/xml"));
    assert!(body.contains("<urlset"));
    assert!(body.contains("<loc>https://sitemap.test/</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/cards</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/cards/mtg</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/cards/mtg/cards</loc>"));
}

#[tokio::test]
async fn sitemap_sets_lists_seeded_sets() {
    let app = test_app_with_catalog().await;

    // Discover a real seeded set code from the catalog, then assert the sitemap
    // advertises its SPA detail page.
    let (_s, _h, sets) = send(&app, get("/api/games/mtg/sets")).await;
    let code = sets["data"][0]["code"].as_str().expect("a seeded set code");

    let (status, _h, body) = send_text(&app, get("/api/sitemaps/sets.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(&format!("<loc>https://sitemap.test/cards/mtg/sets/{code}</loc>")),
        "set {code} missing from sitemap: {body}"
    );
}

#[tokio::test]
async fn sitemap_cards_chunk_lists_cards_and_out_of_range_is_404() {
    let app = test_app_with_catalog().await;

    let (status, headers, body) = send_text(&app, get("/api/sitemaps/cards-1.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(SITEMAP_CACHE_CONTROL));
    assert!(
        body.contains("<loc>https://sitemap.test/cards/mtg/cards/"),
        "no card URLs in chunk: {body}"
    );

    // A chunk past the end is a 404, and — like every error — is never shared-cached.
    let (status, headers, _b) = send_text(&app, get("/api/sitemaps/cards-9999.xml")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // An unknown child name is likewise a 404.
    let (status, _h, _b) = send_text(&app, get("/api/sitemaps/bogus.xml")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Percent-encode a query value (only what these tests need: the injection chars).
fn url_encode(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

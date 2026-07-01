//! Shared HTTP harness for the security tests: builds the real router over a fresh,
//! migrated in-memory state and drives requests through it via `tower`'s `oneshot`
//! (no TCP bind, no network). The request builders and response helpers are
//! `pub(super)` so every concern module (`super::registration`, `super::login`, …)
//! can reuse them; the HTTP/JSON types are re-exported for the same reason, so a
//! concern file only needs `use super::harness::*`.

use std::sync::Arc;

use tower::ServiceExt;

use crate::{build_router, catalog::images::ImageCache, config::Config, state::AppState};

// Re-exported so the concern modules get both the helpers below and the HTTP/JSON
// types they build requests and assert with from a single `use super::harness::*`.
pub(super) use axum::{
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
pub(super) use serde_json::{Value, json};

/// Build an [`AppState`] backed by a fresh in-memory SQLite DB (migrated), a real
/// validated config, and a real precomputed timing-equalizer hash. No card sync.
pub(super) async fn test_state() -> AppState {
    let db = crate::test_support::migrated_memory_db().await;

    let config = Config {
        data_dir: std::env::temp_dir().join("tcglense-security-tests"),
        // A distinctive origin so the sitemap tests can assert the <loc>s are built
        // against the configured public site URL.
        public_site_url: "https://sitemap.test".to_string(),
        ..crate::test_support::test_config()
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
pub(super) async fn test_app() -> Router {
    build_router(test_state().await)
}

/// A router whose DB has the deterministic offline dummy catalog seeded, so the
/// public catalog/search routes have data to exercise.
pub(super) async fn test_app_with_catalog() -> Router {
    let state = test_state().await;
    crate::catalog::seed_all(&state.db).await;
    build_router(state)
}

/// Drive one request through the router (clones it, since `oneshot` consumes the
/// service) and return `(status, headers, json_body)`. A non-JSON / empty body
/// comes back as `Value::Null`.
pub(super) async fn send(app: &Router, req: Request<Body>) -> (StatusCode, HeaderMap, Value) {
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
pub(super) async fn send_text(app: &Router, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
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

/// The `Cache-Control` header value as a string, or `None` if absent.
pub(super) fn cache_control(headers: &HeaderMap) -> Option<&str> {
    headers.get(CACHE_CONTROL).and_then(|v| v.to_str().ok())
}

/// The `Content-Type` header value as a string, or `None` if absent.
pub(super) fn content_type(headers: &HeaderMap) -> Option<&str> {
    headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok())
}

pub(super) fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

pub(super) fn json_post(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub(super) fn post_with_cookie(uri: &str, refresh_token: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(COOKIE, format!("tcglense_refresh={refresh_token}"))
        .body(Body::empty())
        .unwrap()
}

pub(super) fn get_with_bearer(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

/// The plaintext of the freshly-set refresh cookie, if any (ignores a cleared one).
pub(super) fn refresh_token_from(headers: &HeaderMap) -> Option<String> {
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
pub(super) fn refresh_cookie_cleared(headers: &HeaderMap) -> bool {
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
pub(super) async fn register(app: &Router, email: &str, password: &str) -> (String, String) {
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

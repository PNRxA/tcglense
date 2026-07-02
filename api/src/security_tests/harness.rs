//! Shared HTTP harness for the security tests: builds the real router over a fresh,
//! migrated in-memory state and drives requests through it via `tower`'s `oneshot`
//! (no TCP bind, no network). The request builders and response helpers are
//! `pub(super)` so every concern module (`super::registration`, `super::login`, …)
//! can reuse them; the HTTP/JSON types are re-exported for the same reason, so a
//! concern file only needs `use super::harness::*`.

use std::sync::Arc;

use tower::ServiceExt;

use crate::{
    build_router,
    captcha::Captcha,
    config::Config,
    email::{Emailer, Mailbox, OutgoingEmail},
    state::AppState,
};

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

    // Plain clients: the import routes that use them aren't exercised over the
    // network in these in-process tests.
    let http = reqwest::Client::builder().build().expect("build http client");
    let image_http = reqwest::Client::builder().build().expect("build image client");

    AppState::new(config, db, http, image_http).expect("assemble test app state")
}

/// The router plus the state and captured outbox behind it. Tests that only
/// drive HTTP use it exactly like the `Router` it derefs to (`send(&app, …)`);
/// the extra fields exist because email verification made two things reachable
/// only from outside the HTTP surface: the emailed token (the DB stores just
/// its hash, so only the captured message carries the plaintext) and direct DB
/// fixtures.
pub(super) struct TestApp {
    pub router: Router,
    pub state: AppState,
    pub mailbox: Mailbox,
}

impl std::ops::Deref for TestApp {
    type Target = Router;
    fn deref(&self) -> &Router {
        &self.router
    }
}

/// Wrap a state in the real router, swapping the emailer for a capturing sink
/// so tests can read what would have been sent.
fn test_app_over(mut state: AppState) -> TestApp {
    let mailbox = Mailbox::default();
    state.email = Arc::new(Emailer::Capture(mailbox.clone()));
    TestApp {
        router: build_router(state.clone()),
        state,
        mailbox,
    }
}

/// An app over a fresh, empty (no catalog) state.
pub(super) async fn test_app() -> TestApp {
    test_app_over(test_state().await)
}

/// An app whose DB has the deterministic offline dummy catalog seeded, so the
/// public catalog/search routes have data to exercise.
pub(super) async fn test_app_with_catalog() -> TestApp {
    let state = test_state().await;
    crate::catalog::seed_all(&state.db).await;
    test_app_over(state)
}

/// An app that trusts `X-Forwarded-For`, so a test can drive the per-IP rate
/// limiter (the in-process harness has no socket peer) by setting that header.
pub(super) async fn test_app_trusting_proxy() -> TestApp {
    let db = crate::test_support::migrated_memory_db().await;
    let config = Config {
        data_dir: std::env::temp_dir().join("tcglense-security-tests"),
        public_site_url: "https://sitemap.test".to_string(),
        trust_proxy_headers: true,
        ..crate::test_support::test_config()
    };
    let http = reqwest::Client::builder().build().expect("build http client");
    let image_http = reqwest::Client::builder().build().expect("build image client");
    let state = AppState::new(config, db, http, image_http).expect("assemble test app state");
    test_app_over(state)
}

/// An app with **no email provider** (`Emailer::Disabled`) — the dev posture in
/// which the emailed completion link can't be delivered: register returns the
/// completion token in the response instead, and login doesn't gate on
/// verification.
pub(super) async fn test_app_email_disabled() -> TestApp {
    let mut state = test_state().await;
    state.email = Arc::new(Emailer::Disabled);
    TestApp {
        router: build_router(state.clone()),
        state,
        // Nothing is ever sent in this mode; an empty mailbox suffices.
        mailbox: Mailbox::default(),
    }
}

/// An app whose CAPTCHA verifier is enabled and expects the fixed token
/// `"good-token"`, so a test can exercise the token-required path with no network.
pub(super) async fn test_app_requiring_captcha() -> TestApp {
    let mut state = test_state().await;
    let mailbox = Mailbox::default();
    state.email = Arc::new(Emailer::Capture(mailbox.clone()));
    state.captcha = Arc::new(Captcha::ExpectToken("good-token"));
    TestApp {
        router: build_router(state.clone()),
        state,
        mailbox,
    }
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

/// A JSON POST carrying an `X-Forwarded-For`, for driving the per-IP rate limiter
/// (only honoured by an app built via [`test_app_trusting_proxy`]).
pub(super) fn json_post_from(uri: &str, forwarded_for: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .header("x-forwarded-for", forwarded_for)
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

/// A request with a JSON body and a bearer access token, for authenticated writes
/// (e.g. `PUT`/`POST` on the collection routes).
pub(super) fn json_with_bearer(
    method: &str,
    uri: &str,
    token: &str,
    body: Value,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
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

/// Register a usable account and return its access token and refresh-cookie
/// plaintext. Email-first registration takes only the address and answers
/// generically, so this walks the real user journey: submit the email, pull the
/// completion token from the captured email, then complete the registration
/// (choose the password), which verifies the account and signs it in. Tests
/// that care about the pending in-between state drive the endpoints directly.
pub(super) async fn register(app: &TestApp, email: &str, password: &str) -> (String, String) {
    let (status, _, body) = send(
        app,
        json_post("/api/auth/register", json!({ "email": email })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "register failed: {body:?}");

    // The handler canonicalises the address before mailing it.
    let token = latest_email_token(app, &email.trim().to_lowercase()).await;
    let (status, headers, body) = send(
        app,
        json_post(
            "/api/auth/complete-registration",
            json!({ "token": token, "password": password }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "complete-registration failed: {body:?}"
    );
    let access = body["access_token"].as_str().expect("access_token").to_string();
    let refresh = refresh_token_from(&headers).expect("refresh cookie");
    (access, refresh)
}

/// Log in and return the access token and refresh-cookie plaintext.
pub(super) async fn login(app: &TestApp, email: &str, password: &str) -> (String, String) {
    let (status, headers, body) = send(
        app,
        json_post(
            "/api/auth/login",
            json!({ "email": email, "password": password }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "login failed: {body:?}");
    let access = body["access_token"].as_str().expect("access_token").to_string();
    let refresh = refresh_token_from(&headers).expect("refresh cookie");
    (access, refresh)
}

/// Everything "sent" so far, after letting any fire-and-forget send tasks run
/// (the resend/forgot endpoints spawn their sends off the request path; on the
/// test runtime those complete as soon as the test yields — the capture sink
/// does no real I/O).
pub(super) async fn delivered_emails(app: &TestApp) -> Vec<OutgoingEmail> {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    app.mailbox.emails()
}

/// The token carried by the most recent email delivered to `to`. The DB stores
/// only the token's hash, so the captured message is the sole source of the
/// plaintext — exactly like a real inbox.
pub(super) async fn latest_email_token(app: &TestApp, to: &str) -> String {
    let emails = delivered_emails(app).await;
    let email = emails
        .iter()
        .rev()
        .find(|e| e.to == to)
        .unwrap_or_else(|| panic!("no email delivered to {to}"));
    let after = email
        .text
        .split_once("token=")
        .expect("email text carries a token link")
        .1;
    let token: String = after.chars().take_while(char::is_ascii_hexdigit).collect();
    assert!(!token.is_empty(), "token link is empty: {}", email.text);
    token
}

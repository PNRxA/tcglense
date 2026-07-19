//! CLI browser (loopback) sign-in flow (`/api/auth/cli/*`).
//!
//! Drives the real router end to end: a signed-in session mints a one-time code
//! (`/authorize`), and that code + its PKCE verifier are exchanged for a fresh
//! session (`/token`). Asserts the security-relevant contract: authorize is
//! session-only (an API key is refused), the token exchange is PKCE-bound and
//! single-use, a mismatched/absent verifier is a generic 401, and the exchange
//! yields a working, refreshable session for the right account.

use super::harness::*;
use sha2::{Digest, Sha256};

/// A verifier plus the challenge (its SHA-256 hex) the CLI would present. Computes
/// the digest directly (the server's `auth::secret` helper is private) — the CLI
/// derives the challenge the same way.
fn pkce() -> (String, String) {
    let verifier = "cli-pkce-verifier-0123456789abcdef0123456789abcdef".to_string();
    let challenge = hex::encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// Mint a `tcgl_` API key for `access` (a session token) and return the plaintext.
async fn mint_api_key(app: &TestApp, access: &str) -> String {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "POST",
            "/api/auth/api-keys",
            access,
            json!({ "name": "cli-test", "scope": "read_write" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "mint api key: {body:?}");
    body["key"].as_str().expect("key").to_string()
}

#[tokio::test]
async fn full_flow_exchanges_a_code_for_a_working_session() {
    let app = test_app().await;
    let (access, _) = register(&app, "cli-flow@example.com", "password123").await;
    let (verifier, challenge) = pkce();

    // The browser (holding the session) mints a one-time code.
    let (status, _, body) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/auth/cli/authorize",
            &access,
            json!({ "code_challenge": challenge, "client_name": "my laptop" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "authorize: {body:?}");
    let code = body["code"].as_str().expect("code").to_string();
    assert!(body["expires_in"].as_i64().unwrap_or(0) > 0);

    // The CLI exchanges the code + verifier for a session (no auth header).
    let (status, headers, body) = send(
        &app,
        json_post(
            "/api/auth/cli/token",
            json!({ "code": code, "code_verifier": verifier }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "token: {body:?}");
    let cli_access = body["access_token"].as_str().expect("access_token");
    assert_eq!(body["user"]["email"], "cli-flow@example.com");
    let cli_refresh = refresh_token_from(&headers).expect("refresh cookie set");

    // The minted access token authenticates a real request...
    let (status, _, me) = send(&app, get_with_bearer("/api/auth/me", cli_access)).await;
    assert_eq!(status, StatusCode::OK, "me: {me:?}");
    assert_eq!(me["user"]["email"], "cli-flow@example.com");

    // ...and the refresh cookie can be rotated (it's a full session).
    let (status, _, _) = send(&app, post_with_cookie("/api/auth/refresh", &cli_refresh)).await;
    assert_eq!(status, StatusCode::OK, "refresh should succeed");
}

#[tokio::test]
async fn authorize_requires_a_session_and_rejects_an_api_key() {
    let app = test_app().await;
    let (access, _) = register(&app, "cli-session@example.com", "password123").await;
    let (_, challenge) = pkce();

    // No credential at all -> 401.
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/cli/authorize",
            json!({ "code_challenge": challenge }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A valid API key is authenticated but forbidden from authorizing a device.
    let key = mint_api_key(&app, &access).await;
    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/auth/cli/authorize",
            &key,
            json!({ "code_challenge": challenge }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn authorize_rejects_a_malformed_challenge() {
    let app = test_app().await;
    let (access, _) = register(&app, "cli-bad-challenge@example.com", "password123").await;

    let (status, _, _) = send(
        &app,
        json_with_bearer(
            "POST",
            "/api/auth/cli/authorize",
            &access,
            json!({ "code_challenge": "too-short" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn token_rejects_a_wrong_verifier_without_burning_the_code() {
    let app = test_app().await;
    let (access, _) = register(&app, "cli-wrong-verifier@example.com", "password123").await;
    let (verifier, challenge) = pkce();
    let code = authorize(&app, &access, &challenge).await;

    // A wrong verifier is a generic 401...
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/cli/token",
            json!({ "code": code, "code_verifier": "not-the-verifier" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // ...and does not spend the code — the real verifier still redeems it.
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/cli/token",
            json!({ "code": code, "code_verifier": verifier }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_code_is_single_use() {
    let app = test_app().await;
    let (access, _) = register(&app, "cli-replay@example.com", "password123").await;
    let (verifier, challenge) = pkce();
    let code = authorize(&app, &access, &challenge).await;

    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/cli/token",
            json!({ "code": &code, "code_verifier": &verifier }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Replaying the same code is refused.
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/cli/token",
            json!({ "code": code, "code_verifier": verifier }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_unknown_code_is_unauthorized() {
    let app = test_app().await;
    let (verifier, _) = pkce();
    let (status, _, _) = send(
        &app,
        json_post(
            "/api/auth/cli/token",
            json!({ "code": "not-a-real-code", "code_verifier": verifier }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// Mint a one-time code for the given session access token, returning the code.
async fn authorize(app: &TestApp, access: &str, challenge: &str) -> String {
    let (status, _, body) = send(
        app,
        json_with_bearer(
            "POST",
            "/api/auth/cli/authorize",
            access,
            json!({ "code_challenge": challenge }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "authorize: {body:?}");
    body["code"].as_str().expect("code").to_string()
}

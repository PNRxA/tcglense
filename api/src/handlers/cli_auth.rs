//! CLI browser (loopback) sign-in endpoints (`/api/auth/cli/*`).
//!
//! The `tcglense` CLI signs in without ever taking a password in the terminal: it
//! opens the browser to the SPA's `/cli-login` page, the user authenticates there
//! and approves the device, and the browser relays a one-time code to a loopback
//! listener the CLI is holding. This is the OAuth 2.0 native-app loopback flow
//! (RFC 8252) with PKCE. The token store is [`crate::auth::cli_auth`].
//!
//! * `POST /api/auth/cli/authorize` — the *browser*, holding the user's session,
//!   asks the server to mint a one-time code bound to the CLI's PKCE challenge.
//!   [`SessionUser`] (a JWT), never an API key: authorizing a new full-access
//!   device is a session-only action, exactly like API-key management.
//! * `POST /api/auth/cli/token` — the *CLI* exchanges that code (plus the PKCE
//!   verifier) for a normal session — an access token + the refresh cookie —
//!   identical to what [`crate::handlers::auth::login`] returns, so the CLI gets a
//!   refreshable session it can silently keep alive.

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};

use crate::{
    auth::{
        cli_auth::{consume_code, issue_code},
        cookie::build_refresh_cookie,
        extractor::SessionUser,
        jwt::encode_token,
        refresh::issue_refresh_token,
    },
    entities::prelude::User,
    error::AppError,
    extract::JsonBody,
    handlers::auth::AuthResponse,
    state::AppState,
};
use sea_orm::EntityTrait;

/// Length of a PKCE challenge: the SHA-256 hex digest of the verifier (64 chars).
const CODE_CHALLENGE_LEN: usize = 64;
/// Upper bound on the human device label the SPA shows on the consent screen.
const MAX_CLIENT_NAME_LEN: usize = 100;
/// Upper bound on the presented PKCE verifier. The CLI sends 64 hex chars; cap it
/// so an oversized value can't be used to waste work before the generic rejection.
const MAX_CODE_VERIFIER_LEN: usize = 256;

// ---------- DTOs ----------

/// Body of `POST /api/auth/cli/authorize`, sent by the SPA on the user's behalf.
/// Not ts-exported: the SPA hand-writes its request payload (the codebase's
/// convention for request bodies); only the response DTO is generated.
#[derive(Debug, Deserialize)]
pub struct CliAuthorizeRequest {
    /// The SHA-256 hex (64 hex chars) of the CLI's PKCE verifier.
    pub code_challenge: String,
    /// Optional human label for the device/CLI, shown for the user's awareness.
    #[serde(default)]
    pub client_name: Option<String>,
}

/// The one-time code the SPA relays to the CLI via the loopback redirect. Never a
/// session on its own — it must be exchanged (with the verifier) at
/// `/api/auth/cli/token` within [`crate::auth::cli_auth::CODE_TTL`].
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CliAuthorizeResponse {
    pub code: String,
    /// Seconds until the code expires.
    pub expires_in: i64,
}

/// Body of `POST /api/auth/cli/token`, sent by the CLI. Not ts-exported: the CLI
/// hand-maintains its own request type in its own repository.
#[derive(Debug, Deserialize)]
pub struct CliTokenRequest {
    pub code: String,
    pub code_verifier: String,
}

// ---------- Handlers ----------

/// `POST /api/auth/cli/authorize`
///
/// Mint a one-time authorization code for the signed-in user, bound to the CLI's
/// PKCE challenge. `SessionUser`, so an API key can't authorize a new device (a
/// leaked key must not be able to bootstrap a full session — the same rule as
/// API-key management). Returns `201` with the code and its lifetime.
pub async fn cli_authorize(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    JsonBody(payload): JsonBody<CliAuthorizeRequest>,
) -> Result<(StatusCode, Json<CliAuthorizeResponse>), AppError> {
    let challenge = payload.code_challenge.trim().to_ascii_lowercase();
    if challenge.len() != CODE_CHALLENGE_LEN || !challenge.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AppError::Validation(
            "code_challenge must be the SHA-256 hex (64 hex characters) of the verifier"
                .to_string(),
        ));
    }
    let client_name = payload
        .client_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(MAX_CLIENT_NAME_LEN).collect::<String>());

    let code = issue_code(
        &state.db,
        user.id,
        user.session_version,
        &challenge,
        client_name.as_deref(),
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(CliAuthorizeResponse {
            code,
            expires_in: crate::auth::cli_auth::CODE_TTL.num_seconds(),
        }),
    ))
}

/// `POST /api/auth/cli/token`
///
/// Exchange a one-time code + its PKCE verifier for a session. Unauthenticated —
/// the code and verifier ARE the credential. On success mints a session exactly
/// like [`crate::handlers::auth::login`]: an access token plus the `Set-Cookie`
/// refresh token, so the CLI holds a refreshable session. A bad / expired /
/// already-spent code, or a verifier that doesn't match the challenge, is a
/// generic `401`.
pub async fn cli_token(
    State(state): State<AppState>,
    jar: CookieJar,
    JsonBody(payload): JsonBody<CliTokenRequest>,
) -> Result<impl IntoResponse, AppError> {
    let code = payload.code.trim();
    if code.is_empty()
        || payload.code_verifier.is_empty()
        || payload.code_verifier.len() > MAX_CODE_VERIFIER_LEN
    {
        return Err(AppError::Unauthorized(
            "invalid or expired code".to_string(),
        ));
    }

    let claim = consume_code(&state.db, code, &payload.code_verifier).await?;

    // Mint a session exactly like `login`. Loading the user and re-checking the
    // generation (plus `issue_refresh_token`'s own atomic generation check) means a
    // password reset that landed after the code was minted — but before this
    // exchange — still invalidates it.
    let user = User::find_by_id(claim.user_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid or expired code".to_string()))?;
    if user.session_version != claim.session_version {
        return Err(AppError::Unauthorized(
            "invalid or expired code".to_string(),
        ));
    }

    let access_token = encode_token(&user, &state.config)?;
    let refresh_plaintext = issue_refresh_token(
        &state.db,
        user.id,
        user.session_version,
        state.config.refresh_token_expiry_days,
    )
    .await?;
    let jar = jar.add(build_refresh_cookie(refresh_plaintext, &state.config));

    Ok((
        StatusCode::OK,
        jar,
        Json(AuthResponse {
            access_token,
            user: user.into(),
        }),
    ))
}

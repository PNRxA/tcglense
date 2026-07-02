use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set, SqlErr, prelude::DateTimeUtc,
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{
        cookie::{REFRESH_COOKIE_NAME, build_refresh_cookie, removal_cookie},
        email_token::{EmailTokenPurpose, consume, issue, issue_with_cooldown},
        extractor::AuthUser,
        jwt::encode_token,
        password::{hash_password, verify_password},
        refresh::{issue_refresh_token, revoke_all_for_user, revoke_one, rotate},
    },
    client_ip::ClientIp,
    email::{OutgoingEmail, password_reset_email, verification_email},
    entities::{prelude::User, user},
    error::AppError,
    extract::JsonBody,
    state::AppState,
};

// ---------- Request / response DTOs ----------

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub display_name: Option<String>,
    /// CAPTCHA token from the browser widget (required only when a verifier is
    /// configured; absent/ignored in dev/test where CAPTCHA is disabled).
    #[serde(default)]
    pub captcha_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub captcha_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VerifyEmailRequest {
    pub token: String,
    #[serde(default)]
    pub captcha_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResendVerificationRequest {
    pub email: String,
    #[serde(default)]
    pub captcha_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ForgotPasswordRequest {
    pub email: String,
    #[serde(default)]
    pub captcha_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub password: String,
    #[serde(default)]
    pub captcha_token: Option<String>,
}

/// Public-facing user shape (never includes the password hash).
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "User"))]
pub struct UserResponse {
    pub id: i32,
    pub email: String,
    pub display_name: Option<String>,
    pub created_at: DateTimeUtc,
}

impl From<user::Model> for UserResponse {
    fn from(m: user::Model) -> Self {
        UserResponse {
            id: m.id,
            email: m.email,
            display_name: m.display_name,
            created_at: m.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct AuthResponse {
    pub access_token: String,
    pub user: UserResponse,
}

/// Body returned by `/api/auth/register`: the created account. Normally there is
/// **no session** — signing in requires the emailed verification link first — so
/// `access_token` is `null`. But when email verification is bypassed (no email
/// provider configured — dev), the account is created already-verified and signed
/// in, and `access_token` carries the session (with the refresh cookie set).
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct RegisterResponse {
    pub user: UserResponse,
    pub access_token: Option<String>,
}

/// Body returned by `/api/auth/refresh` (the rotated refresh token rides in the
/// `Set-Cookie` header, never in the JSON body).
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "RefreshResponse"))]
pub struct AccessTokenResponse {
    pub access_token: String,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub user: UserResponse,
}

// ---------- Validation helpers ----------

/// Upper bound on a stored email (RFC 5321 caps an address at 254 octets).
const MAX_EMAIL_LEN: usize = 254;
/// Upper bound on a password. Argon2 does not truncate, so an unbounded password
/// is a cheap-to-send, expensive-to-hash DoS vector; cap it generously enough to
/// still allow long passphrases.
const MAX_PASSWORD_LEN: usize = 1024;

fn validate_email(email: &str) -> Result<(), AppError> {
    if email.is_empty() || !email.contains('@') {
        return Err(AppError::Validation(
            "email must be non-empty and contain '@'".to_string(),
        ));
    }
    if email.len() > MAX_EMAIL_LEN {
        return Err(AppError::Validation(format!(
            "email must be at most {MAX_EMAIL_LEN} characters"
        )));
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), AppError> {
    if password.len() < 8 {
        return Err(AppError::Validation(
            "password must be at least 8 characters".to_string(),
        ));
    }
    if password.len() > MAX_PASSWORD_LEN {
        return Err(AppError::Validation(format!(
            "password must be at most {MAX_PASSWORD_LEN} characters"
        )));
    }
    Ok(())
}

// ---------- Handlers ----------

/// Absolute SPA link carrying an emailed token, built against the configured
/// public site origin (`public_site_url` is trailing-slash-trimmed; the token is
/// hex, so no URL-encoding is needed).
fn spa_link(state: &AppState, path: &str, token: &str) -> String {
    format!("{}/{path}?token={token}", state.config.public_site_url)
}

/// Fire-and-forget email send for the anti-enumeration endpoints: the send runs
/// off the request path so response timing can't reveal whether an account
/// exists, and failures are logged, never surfaced (a 502 would only ever fire
/// for existing accounts — leaking exactly what those endpoints must not).
fn spawn_send(state: &AppState, email: OutgoingEmail) {
    let emailer = state.email.clone();
    tokio::spawn(async move {
        if let Err(err) = emailer.send(email).await {
            tracing::warn!(error = %err, "failed to send email");
        }
    });
}

/// `POST /api/auth/register`
pub async fn register(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    jar: CookieJar,
    JsonBody(payload): JsonBody<RegisterRequest>,
) -> Result<impl IntoResponse, AppError> {
    state
        .captcha
        .verify(payload.captcha_token.as_deref(), client_ip)
        .await?;
    // Canonicalise the email so look-alike casings map to a single account.
    let email = payload.email.trim().to_lowercase();
    validate_email(&email)?;
    validate_password(&payload.password)?;

    let existing = User::find()
        .filter(user::Column::Email.eq(&email))
        .one(&state.db)
        .await?;
    if existing.is_some() {
        return Err(AppError::Conflict("email already registered".to_string()));
    }

    let password_hash = hash_password(&payload.password)?;
    let display_name = payload
        .display_name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let now = Utc::now();

    let new_user = user::ActiveModel {
        email: Set(email),
        password_hash: Set(password_hash),
        display_name: Set(display_name),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    // The pre-check above handles the common case, but the unique index is the
    // real source of truth: a concurrent registration can race past it, so map
    // a unique-constraint violation to 409 rather than letting it become a 500.
    let model = match new_user.insert(&state.db).await {
        Ok(model) => model,
        Err(err) => {
            if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
                return Err(AppError::Conflict("email already registered".to_string()));
            }
            return Err(err.into());
        }
    };
    // The response's user shape is verification-agnostic, so build it up front
    // (the DB row may be stamped verified just below).
    let user = UserResponse::from(model.clone());

    let (access_token, jar) = if state.email.is_enabled() {
        // Verify-first: registration does NOT start a session — the account must
        // prove it owns the address (the emailed link) before login accepts it.
        let token = issue(&state.db, model.id, EmailTokenPurpose::VerifyEmail).await?;
        let link = spa_link(&state, "verify-email", &token);
        // Await the send so the mail is normally on its way before we answer, but
        // don't fail the registration over it: the account exists either way, and
        // the sign-in screen offers a fresh link (resend-verification) at any time.
        if let Err(err) = state
            .email
            .send(verification_email(&model.email, &link))
            .await
        {
            tracing::error!(error = %err, "failed to send the verification email");
        }
        (None, jar)
    } else {
        // No email provider (dev): there is no way to deliver a verification link,
        // so bypass verification entirely — mark the account verified and sign it
        // in immediately, so a dev can register and use it in one step.
        user::ActiveModel {
            id: Set(model.id),
            email_verified_at: Set(Some(now)),
            updated_at: Set(now),
            ..Default::default()
        }
        .update(&state.db)
        .await?;
        let access = encode_token(&model, &state.config)?;
        let refresh =
            issue_refresh_token(&state.db, model.id, state.config.refresh_token_expiry_days).await?;
        (Some(access), jar.add(build_refresh_cookie(refresh, &state.config)))
    };

    Ok((
        StatusCode::CREATED,
        jar,
        Json(RegisterResponse { user, access_token }),
    ))
}

/// `POST /api/auth/login`
pub async fn login(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    jar: CookieJar,
    JsonBody(payload): JsonBody<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    state
        .captcha
        .verify(payload.captcha_token.as_deref(), client_ip)
        .await?;
    let email = payload.email.trim().to_lowercase();

    // Cap the password length here too (register enforces it, but login must as
    // well): an unbounded password is fed straight to Argon2 verify below, so an
    // oversized one is a cheap-to-send, expensive-to-hash DoS. This is a pure
    // length check, so the generic 401 is preserved for any plausible password —
    // no value over the cap could match a stored account anyway.
    if payload.password.len() > MAX_PASSWORD_LEN {
        return Err(AppError::Validation(format!(
            "password must be at most {MAX_PASSWORD_LEN} characters"
        )));
    }

    let user = User::find()
        .filter(user::Column::Email.eq(&email))
        .one(&state.db)
        .await?;

    let user = match user {
        Some(u) => u,
        None => {
            // Keep timing comparable to the wrong-password path (a real Argon2
            // verify against the precomputed dummy hash), then fail generically.
            let _ = verify_password(&state.dummy_password_hash, &payload.password);
            return Err(AppError::InvalidCredentials);
        }
    };

    if !verify_password(&user.password_hash, &payload.password) {
        return Err(AppError::InvalidCredentials);
    }

    // Checked only AFTER the password verified, so the distinct error is never
    // an account-enumeration oracle (the generic-401 timing paths above are
    // untouched); 403 (not 401) so the SPA's auto-refresh never fires on it.
    // Only enforced when an email provider is configured: with no provider (dev)
    // there's no way to verify, so verification is bypassed (see `register`).
    if user.email_verified_at.is_none() && state.email.is_enabled() {
        return Err(AppError::Forbidden("email not verified".to_string()));
    }

    let access_token = encode_token(&user, &state.config)?;
    let refresh_plaintext =
        issue_refresh_token(&state.db, user.id, state.config.refresh_token_expiry_days).await?;
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

/// `POST /api/auth/refresh`
///
/// Reads the `tcglense_refresh` cookie, rotates it, and returns a new access
/// token. Any failure clears the cookie and returns 401.
pub async fn refresh(State(state): State<AppState>, jar: CookieJar) -> Response {
    let Some(cookie) = jar.get(REFRESH_COOKIE_NAME) else {
        return (
            jar.remove(removal_cookie()),
            AppError::Unauthorized("missing refresh token".to_string()),
        )
            .into_response();
    };
    let presented = cookie.value().to_string();

    match issue_rotated_access_token(&state, &presented).await {
        Ok((access_token, new_refresh)) => {
            let jar = jar.add(build_refresh_cookie(new_refresh, &state.config));
            (jar, Json(AccessTokenResponse { access_token })).into_response()
        }
        Err(err) => (jar.remove(removal_cookie()), err).into_response(),
    }
}

/// Rotate the presented refresh token and mint a fresh access token for the
/// owning user. Returns `(access_token, new_refresh_plaintext)`.
async fn issue_rotated_access_token(
    state: &AppState,
    presented: &str,
) -> Result<(String, String), AppError> {
    let rotated = rotate(&state.db, presented, state.config.refresh_token_expiry_days).await?;

    let user = User::find_by_id(rotated.user_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;

    let access_token = encode_token(&user, &state.config)?;
    Ok((access_token, rotated.plaintext))
}

/// `POST /api/auth/logout`
///
/// Revokes the presented refresh token (best-effort) and clears the cookie.
/// Always 204, even when no/invalid cookie is present.
pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if let Some(cookie) = jar.get(REFRESH_COOKIE_NAME) {
        let presented = cookie.value().to_string();
        // Best-effort: logout must remain idempotent and always succeed.
        if let Err(err) = revoke_one(&state.db, &presented).await {
            tracing::warn!(error = %err, "failed to revoke refresh token on logout");
        }
    }

    (StatusCode::NO_CONTENT, jar.remove(removal_cookie()))
}

/// `GET /api/auth/me`
pub async fn me(AuthUser(user): AuthUser) -> Result<impl IntoResponse, AppError> {
    Ok((StatusCode::OK, Json(MeResponse { user: user.into() })))
}

/// `POST /api/auth/verify-email`
///
/// Consumes an emailed verification token (single-use, 24h expiry) and stamps
/// the account verified. Mints no session — the user signs in normally after.
pub async fn verify_email(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    JsonBody(payload): JsonBody<VerifyEmailRequest>,
) -> Result<impl IntoResponse, AppError> {
    state
        .captcha
        .verify(payload.captcha_token.as_deref(), client_ip)
        .await?;
    let row = consume(&state.db, &payload.token, EmailTokenPurpose::VerifyEmail).await?;

    let user = User::find_by_id(row.user_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid or expired token".to_string()))?;

    if user.email_verified_at.is_none() {
        let now = Utc::now();
        let mut active: user::ActiveModel = user.into();
        active.email_verified_at = Set(Some(now));
        active.updated_at = Set(now);
        active.update(&state.db).await?;
    }

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/auth/resend-verification`
///
/// Unauthenticated (an unverified account cannot sign in to ask). Deliberately
/// generic: an unknown address, an already-verified account, and the issue
/// cooldown all return the same 204, and the send itself runs off the request
/// path — the endpoint reveals nothing about which accounts exist.
pub async fn resend_verification(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    JsonBody(payload): JsonBody<ResendVerificationRequest>,
) -> Result<impl IntoResponse, AppError> {
    state
        .captcha
        .verify(payload.captcha_token.as_deref(), client_ip)
        .await?;
    let email = payload.email.trim().to_lowercase();
    validate_email(&email)?;

    let user = User::find()
        .filter(user::Column::Email.eq(&email))
        .one(&state.db)
        .await?;

    if let Some(user) = user
        && user.email_verified_at.is_none()
        && let Some(token) =
            issue_with_cooldown(&state.db, user.id, EmailTokenPurpose::VerifyEmail).await?
    {
        let link = spa_link(&state, "verify-email", &token);
        spawn_send(&state, verification_email(&user.email, &link));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/auth/forgot-password`
///
/// Deliberately generic like resend-verification: always 204, send off the
/// request path. The reset link is issued even for an unverified account —
/// completing the reset proves mailbox ownership (and verifies it, see
/// [`reset_password`]), so losing a password never strands an account.
pub async fn forgot_password(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    JsonBody(payload): JsonBody<ForgotPasswordRequest>,
) -> Result<impl IntoResponse, AppError> {
    state
        .captcha
        .verify(payload.captcha_token.as_deref(), client_ip)
        .await?;
    let email = payload.email.trim().to_lowercase();
    validate_email(&email)?;

    let user = User::find()
        .filter(user::Column::Email.eq(&email))
        .one(&state.db)
        .await?;

    if let Some(user) = user
        && let Some(token) =
            issue_with_cooldown(&state.db, user.id, EmailTokenPurpose::ResetPassword).await?
    {
        let link = spa_link(&state, "reset-password", &token);
        spawn_send(&state, password_reset_email(&user.email, &link));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/auth/reset-password`
///
/// Consumes an emailed reset token (single-use, 1h expiry), re-hashes the
/// password, and revokes every refresh token — a changed password ends all
/// existing sessions. Completing a reset also proves mailbox ownership, so it
/// stamps a still-unverified account verified.
pub async fn reset_password(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    JsonBody(payload): JsonBody<ResetPasswordRequest>,
) -> Result<impl IntoResponse, AppError> {
    state
        .captcha
        .verify(payload.captcha_token.as_deref(), client_ip)
        .await?;
    validate_password(&payload.password)?;

    let row = consume(&state.db, &payload.token, EmailTokenPurpose::ResetPassword).await?;

    let user = User::find_by_id(row.user_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid or expired token".to_string()))?;

    let user_id = user.id;
    let now = Utc::now();
    let was_verified = user.email_verified_at.is_some();
    let mut active: user::ActiveModel = user.into();
    active.password_hash = Set(hash_password(&payload.password)?);
    active.updated_at = Set(now);
    if !was_verified {
        active.email_verified_at = Set(Some(now));
    }
    active.update(&state.db).await?;

    revoke_all_for_user(&state.db, user_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

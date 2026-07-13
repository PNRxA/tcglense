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
        email_token::{EmailTokenPurpose, consume, issue_with_cooldown},
        extractor::{AuthUser, WritableUser},
        jwt::encode_token,
        password::{hash_password, verify_password},
        refresh::{RotateOutcome, issue_refresh_token, revoke_all_for_user, revoke_one, rotate},
    },
    client_ip::ClientIp,
    email::{OutgoingEmail, password_reset_email, registration_email, verification_email},
    entities::{prelude::User, user},
    error::AppError,
    extract::{JsonBody, Query},
    state::AppState,
};

// ---------- Request / response DTOs ----------

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    /// CAPTCHA token from the browser widget (required only when a verifier is
    /// configured; absent/ignored in dev/test where CAPTCHA is disabled).
    #[serde(default)]
    pub captcha_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteRegistrationRequest {
    pub token: String,
    pub password: String,
    /// Optionally claim a username at signup (issue #362); a `#XXXX` discriminator is
    /// auto-assigned. Left unset, the account has no handle until it's chosen later (from
    /// a collection page, when first going public).
    #[serde(default)]
    pub username: Option<String>,
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

/// Public-facing user shape (never includes the password hash). Carries the opt-in
/// public handle (issue #362): `username`/`discriminator` are set together the first
/// time the user makes a collection public, and `handle` is the formatted
/// `username-0001` (or null until then) the SPA uses for `/u/{handle}/{game}` links.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "User"))]
pub struct UserResponse {
    pub id: i32,
    pub email: String,
    pub created_at: DateTimeUtc,
    pub username: Option<String>,
    pub discriminator: Option<i32>,
    pub handle: Option<String>,
}

impl From<user::Model> for UserResponse {
    fn from(m: user::Model) -> Self {
        // Compute the handle before the field moves below consume `m`.
        let handle = crate::auth::username::handle_of(&m);
        UserResponse {
            id: m.id,
            email: m.email,
            created_at: m.created_at,
            username: m.username,
            discriminator: m.discriminator,
            handle,
        }
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct AuthResponse {
    pub access_token: String,
    pub user: UserResponse,
}

/// Body returned by `/api/auth/register`. Deliberately account-agnostic: the
/// same generic body comes back whether the address was new, mid-registration,
/// or already registered — the endpoint is no enumeration oracle, and the next
/// step (the completion link) always arrives by email. `completion_token` is
/// **always `null` when a real email provider is configured**; only when email
/// sending is disabled (no provider — dev/e2e) does it carry the
/// registration-completion token, so the SPA can drive straight to the
/// set-password step the undeliverable email would have linked to.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct RegisterResponse {
    pub completion_token: Option<String>,
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
///
/// Email-first (issue #176): the visitor submits ONLY their address. A new
/// address gets a pending (password-less) account row and a completion link by
/// email; a pending one gets the link re-sent (60s cooldown); an
/// already-registered one gets nothing. Whichever case, the response is the
/// same generic 200 and the send runs off the request path — registering
/// reveals nothing about which accounts exist (the pre-#176 duplicate `409`
/// was an enumeration oracle).
pub async fn register(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    JsonBody(payload): JsonBody<RegisterRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Signup switch (SIGNUPS_ENABLED=false): refuse to start a new registration
    // while existing users keep signing in. Checked before the CAPTCHA so a
    // disabled instance needn't mint a token just to be told no. 403 (not 401)
    // keeps the SPA's access-token auto-refresh from firing on it.
    if !state.config.signups_enabled {
        return Err(AppError::Forbidden(state.config.signups_disabled_notice()));
    }
    state
        .captcha
        .verify(payload.captcha_token.as_deref(), client_ip)
        .await?;
    // Canonicalise the email so look-alike casings map to a single account.
    let email = payload.email.trim().to_lowercase();
    validate_email(&email)?;

    let existing = User::find()
        .filter(user::Column::Email.eq(&email))
        .one(&state.db)
        .await?;

    let user = match existing {
        Some(user) => user,
        None => {
            let now = Utc::now();
            let new_user = user::ActiveModel {
                email: Set(email.clone()),
                password_hash: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            };
            match new_user.insert(&state.db).await {
                Ok(model) => model,
                // A concurrent registration can race past the lookup above; the
                // unique index is the source of truth. Fall through to the row
                // that won rather than answering differently (no 409 here — an
                // existing account must be indistinguishable from a new one).
                Err(err) if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => {
                    match User::find()
                        .filter(user::Column::Email.eq(&email))
                        .one(&state.db)
                        .await?
                    {
                        Some(user) => user,
                        None => return Err(err.into()),
                    }
                }
                Err(err) => return Err(err.into()),
            }
        }
    };

    // Only a pending (password-less) account gets a completion link; the
    // cooldown collapses repeat requests, and an already-registered address
    // gets no mail at all — all unobservable from outside, since the send is
    // fire-and-forget off the request path.
    let mut completion_token = None;
    if user.password_hash.is_none()
        && let Some(token) =
            issue_with_cooldown(&state.db, user.id, EmailTokenPurpose::CompleteRegistration).await?
    {
        let link = spa_link(&state, "complete-registration", &token);
        spawn_send(&state, registration_email(&user.email, &link));
        // No-email posture (dev/e2e): the link above was only logged, so hand
        // the token to the SPA directly — it drives straight to the
        // set-password page and the offline registration journey stays
        // completable. With a real provider this stays null: the token only
        // ever travels by email.
        if !state.email.is_enabled() {
            completion_token = Some(token);
        }
    }

    Ok((StatusCode::OK, Json(RegisterResponse { completion_token })))
}

/// `POST /api/auth/complete-registration`
///
/// Consumes an emailed registration-completion token (single-use, 24h expiry),
/// sets the account's first password (+ optional display name), stamps the
/// email verified (using the link proves mailbox ownership), and signs the
/// user in. Only a pending (password-less) account qualifies: once a password
/// exists the token is refused, so a completion link can never double as a
/// password reset.
pub async fn complete_registration(
    State(state): State<AppState>,
    ClientIp(client_ip): ClientIp,
    jar: CookieJar,
    JsonBody(payload): JsonBody<CompleteRegistrationRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Same signup switch as `register`: a completion link issued before signups
    // were turned off must not be able to finalise a brand-new account either.
    if !state.config.signups_enabled {
        return Err(AppError::Forbidden(state.config.signups_disabled_notice()));
    }
    state
        .captcha
        .verify(payload.captcha_token.as_deref(), client_ip)
        .await?;
    // Validate BEFORE consuming the token, so a weak password (or a bad optional
    // username) doesn't burn the single-use link (mirrors reset_password).
    validate_password(&payload.password)?;
    let username_display = payload
        .username
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(crate::auth::username::validate)
        .transpose()?;

    let row = consume(
        &state.db,
        &payload.token,
        EmailTokenPurpose::CompleteRegistration,
    )
    .await?;

    let invalid = || AppError::Unauthorized("invalid or expired token".to_string());
    let user = User::find_by_id(row.user_id)
        .one(&state.db)
        .await?
        .ok_or_else(invalid)?;
    // Already completed (the password arrived via an earlier completion or a
    // reset): refuse generically, like any other dead token.
    if user.password_hash.is_some() {
        return Err(invalid());
    }

    let now = Utc::now();
    let mut active: user::ActiveModel = user.into();
    active.password_hash = Set(Some(hash_password(&payload.password)?));
    active.email_verified_at = Set(Some(now));
    active.updated_at = Set(now);
    let user = active.update(&state.db).await?;

    // Optionally claim the username chosen at signup, with an auto-assigned discriminator.
    // The account is already committed + verified above and the token is spent, so a rare
    // username-claim failure (a DB blip, or the near-impossible discriminator exhaustion) must
    // NOT fail the whole signup — that would strand a created account behind a now-dead token.
    // Complete the session without a handle; the user can set a username later (the opt-in
    // flow from any collection page).
    let user = match username_display {
        Some(display) => match assign_username(&state.db, user.clone(), display).await {
            Ok(updated) => updated,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    user_id = user.id,
                    "could not claim the username chosen at signup; account created without a handle"
                );
                user
            }
        },
        None => user,
    };

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

    // A pending registration (email-first, no password chosen yet) has no
    // credential to check: keep the timing comparable (same dummy verify as an
    // unknown address) and fail with the same generic 401.
    let Some(password_hash) = user.password_hash.as_deref() else {
        let _ = verify_password(&state.dummy_password_hash, &payload.password);
        return Err(AppError::InvalidCredentials);
    };

    if !verify_password(password_hash, &payload.password) {
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

    match rotate(
        &state.db,
        &presented,
        state.config.refresh_token_expiry_days,
    )
    .await
    {
        Ok(RotateOutcome::Rotated(rotated)) => {
            match mint_access_token(&state, rotated.user_id).await {
                Ok(access_token) => {
                    let jar = jar.add(build_refresh_cookie(rotated.plaintext, &state.config));
                    (jar, Json(AccessTokenResponse { access_token })).into_response()
                }
                Err(err) => refresh_error_response(jar, err),
            }
        }
        // Benign concurrent double-submit: a sibling request/tab already rotated
        // this cookie and its successor is still live, so the browser already holds
        // the newer valid cookie. Return 401 but send NO Set-Cookie — clearing here
        // would race the winning request's rotated cookie and, when the clear lands
        // last, wipe the live cookie and log every tab out (the intermittent
        // "randomly signed out" bug). The caller simply refreshes again and picks
        // up the current cookie. See `RotateOutcome::Superseded`.
        Ok(RotateOutcome::Superseded) => {
            AppError::Unauthorized("refresh token superseded".to_string()).into_response()
        }
        Err(err) => refresh_error_response(jar, err),
    }
}

/// Build the response for a refresh that minted no new token.
///
/// The browser's refresh cookie is cleared ONLY for a *definitive* auth failure
/// (a 401 — an unknown/expired/reuse-detected token, or a vanished user): the
/// session is genuinely dead, so removing the cookie stops the SPA re-trying a
/// hopeless refresh. A *transient* failure (a 5xx — e.g. a momentary database
/// error on the refresh path, which prod's cold Postgres makes real) leaves the
/// cookie in place: otherwise a brief infra blip during a refresh would wipe the
/// cookie and turn a recoverable hiccup into a permanent, browser-wide logout.
fn refresh_error_response(jar: CookieJar, err: AppError) -> Response {
    if matches!(err, AppError::Unauthorized(_)) {
        (jar.remove(removal_cookie()), err).into_response()
    } else {
        err.into_response()
    }
}

/// Load the owner of a just-rotated refresh token and mint them a fresh access
/// token.
async fn mint_access_token(state: &AppState, user_id: i32) -> Result<String, AppError> {
    let user = User::find_by_id(user_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;

    encode_token(&user, &state.config)
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

/// Body of `PUT /api/auth/username`.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SetUsernameRequest {
    pub username: String,
}

/// `PUT /api/auth/username` -> set or change the caller's username (issue #362).
/// `WritableUser`, so a read-only API key is 403 — a key must not claim a handle. The
/// username is validated (length/charset/structure/reserved/`rustrict`) before any write;
/// the discriminator is kept across a rename when the new pair is free, else the lowest
/// free one is allocated. The `(lower(username), discriminator)` unique index is the
/// source of truth for the concurrent-allocation race, so a lost race re-allocates and
/// retries. Returns the updated user (its `handle` now populated).
pub async fn set_username(
    State(state): State<AppState>,
    WritableUser(user): WritableUser,
    JsonBody(payload): JsonBody<SetUsernameRequest>,
) -> Result<impl IntoResponse, AppError> {
    let display = crate::auth::username::validate(&payload.username)?;
    let user = assign_username(&state.db, user, display).await?;
    Ok((StatusCode::OK, Json(UserResponse::from(user))))
}

/// Allocate a free discriminator for the (already-validated) display username and persist
/// it on `user`, keeping the current tag across a rename when it's still free. The
/// `(lower(username), discriminator)` unique index is the source of truth for the
/// concurrent-allocation race, so a lost race re-allocates and retries. Shared by
/// [`set_username`] and the optional username chosen at registration completion.
async fn assign_username(
    db: &sea_orm::DatabaseConnection,
    user: user::Model,
    display: String,
) -> Result<user::Model, AppError> {
    let normalized = crate::auth::username::normalize(&display);
    const MAX_ALLOC_RETRIES: usize = 5;
    for _ in 0..MAX_ALLOC_RETRIES {
        let discriminator = crate::auth::username::allocate_discriminator(
            db,
            &normalized,
            user.discriminator, // prefer keeping the current tag across a rename
            user.id,            // exclude the caller's own row from the "taken" set
        )
        .await?;

        let mut active: user::ActiveModel = user.clone().into();
        active.username = Set(Some(display.clone()));
        active.discriminator = Set(Some(discriminator));
        active.updated_at = Set(Utc::now());
        match active.update(db).await {
            Ok(row) => return Ok(row),
            // Lost the (lower(username), discriminator) race — re-allocate and retry.
            Err(e) if matches!(e.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(AppError::Conflict(
        "could not allocate a username; please try again".to_string(),
    ))
}

/// Query params for the username-availability check.
#[derive(Debug, Deserialize)]
pub struct UsernameAvailabilityParams {
    #[serde(default)]
    pub username: String,
}

/// Whether a candidate username passes the rules, with a reason when it doesn't — for
/// the "choose a username" dialog's live feedback.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct UsernameAvailability {
    pub valid: bool,
    pub reason: Option<String>,
}

/// `GET /api/auth/username/available?username=<name>` -> whether `<name>` passes the
/// username rules. Validation-only: it allocates nothing (the discriminator scheme makes
/// a valid name effectively always claimable), so it never reserves a tag or reveals
/// whether a name is "taken". `AuthUser` — the dialog is only reachable when signed in,
/// keeping the profanity checker off the open internet.
pub async fn username_available(
    AuthUser(_user): AuthUser,
    Query(params): Query<UsernameAvailabilityParams>,
) -> Result<Json<UsernameAvailability>, AppError> {
    match crate::auth::username::validate(&params.username) {
        Ok(_) => Ok(Json(UsernameAvailability {
            valid: true,
            reason: None,
        })),
        Err(AppError::Validation(reason)) => Ok(Json(UsernameAvailability {
            valid: false,
            reason: Some(reason),
        })),
        Err(e) => Err(e),
    }
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

    // Only an account that HAS a password gets a verification link: a
    // password-less row is a pending email-first registration, whose link is
    // re-sent by POSTing /register again (same address, same generic 204 here).
    if let Some(user) = user
        && user.password_hash.is_some()
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

    // Signup switch: a reset must not *activate* a pending (password-less) account
    // while signups are disabled — that is the same new-account creation `register`
    // and `complete-registration` refuse (a stale pending row could otherwise be
    // finalised via forgot/reset). A genuine reset for an account that already has
    // a password is untouched, so existing users keep recovering their access.
    if user.password_hash.is_none() && !state.config.signups_enabled {
        return Err(AppError::Forbidden(state.config.signups_disabled_notice()));
    }

    let user_id = user.id;
    let now = Utc::now();
    let was_verified = user.email_verified_at.is_some();
    let mut active: user::ActiveModel = user.into();
    active.password_hash = Set(Some(hash_password(&payload.password)?));
    active.updated_at = Set(now);
    if !was_verified {
        active.email_verified_at = Set(Some(now));
    }
    active.update(&state.db).await?;

    revoke_all_for_user(&state.db, user_id).await?;

    Ok(StatusCode::NO_CONTENT)
}

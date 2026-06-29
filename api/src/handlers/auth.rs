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
        extractor::AuthUser,
        jwt::encode_token,
        password::{hash_password, verify_password},
        refresh::{issue_refresh_token, revoke_one, rotate},
    },
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
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// Public-facing user shape (never includes the password hash).
#[derive(Debug, Serialize)]
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
pub struct AuthResponse {
    pub access_token: String,
    pub user: UserResponse,
}

/// Body returned by `/api/auth/refresh` (the rotated refresh token rides in the
/// `Set-Cookie` header, never in the JSON body).
#[derive(Debug, Serialize)]
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

/// `POST /api/auth/register`
pub async fn register(
    State(state): State<AppState>,
    jar: CookieJar,
    JsonBody(payload): JsonBody<RegisterRequest>,
) -> Result<impl IntoResponse, AppError> {
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
    let access_token = encode_token(&model, &state.config)?;
    let refresh_plaintext =
        issue_refresh_token(&state.db, model.id, state.config.refresh_token_expiry_days).await?;
    let jar = jar.add(build_refresh_cookie(refresh_plaintext, &state.config));

    Ok((
        StatusCode::CREATED,
        jar,
        Json(AuthResponse {
            access_token,
            user: model.into(),
        }),
    ))
}

/// `POST /api/auth/login`
pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    JsonBody(payload): JsonBody<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    let email = payload.email.trim().to_lowercase();

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

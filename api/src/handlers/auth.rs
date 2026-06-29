use std::sync::OnceLock;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set, SqlErr, prelude::DateTimeUtc,
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{
        extractor::AuthUser,
        jwt::encode_token,
        password::{hash_password, verify_password},
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
    pub token: String,
    pub user: UserResponse,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    pub user: UserResponse,
}

// ---------- Validation helpers ----------

fn validate_email(email: &str) -> Result<(), AppError> {
    if email.is_empty() || !email.contains('@') {
        return Err(AppError::Validation(
            "email must be non-empty and contain '@'".to_string(),
        ));
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), AppError> {
    if password.len() < 8 {
        return Err(AppError::Validation(
            "password must be at least 8 characters".to_string(),
        ));
    }
    Ok(())
}

/// Perform a throwaway hash verification to keep the timing of "user not found"
/// roughly equal to "wrong password", mitigating user-enumeration via timing.
fn equalize_timing(password: &str) {
    static DUMMY_HASH: OnceLock<String> = OnceLock::new();
    let dummy = DUMMY_HASH
        .get_or_init(|| hash_password("tcglense-timing-equalizer").unwrap_or_default());
    let _ = verify_password(dummy, password);
}

// ---------- Handlers ----------

/// `POST /api/auth/register`
pub async fn register(
    State(state): State<AppState>,
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
    let token = encode_token(&model, &state.config)?;

    Ok((
        StatusCode::CREATED,
        Json(AuthResponse {
            token,
            user: model.into(),
        }),
    ))
}

/// `POST /api/auth/login`
pub async fn login(
    State(state): State<AppState>,
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
            // Keep timing comparable to the wrong-password path, then fail generically.
            equalize_timing(&payload.password);
            return Err(AppError::InvalidCredentials);
        }
    };

    if !verify_password(&user.password_hash, &payload.password) {
        return Err(AppError::InvalidCredentials);
    }

    let token = encode_token(&user, &state.config)?;

    Ok((
        StatusCode::OK,
        Json(AuthResponse {
            token,
            user: user.into(),
        }),
    ))
}

/// `GET /api/auth/me`
pub async fn me(AuthUser(user): AuthUser) -> Result<impl IntoResponse, AppError> {
    Ok((
        StatusCode::OK,
        Json(MeResponse {
            user: user.into(),
        }),
    ))
}

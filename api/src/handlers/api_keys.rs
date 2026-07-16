//! API-key management endpoints (`/api/auth/api-keys`).
//!
//! A signed-in user mints, lists, and revokes the long-lived keys that authenticate
//! their programmatic access to the public API. Every handler here takes
//! [`SessionUser`] — an API key can *use* the API but can never manage keys, so a
//! leaked key cannot mint more or enumerate/revoke its siblings. The created key's
//! plaintext is returned exactly once; list only ever exposes metadata.

use axum::{Json, extract::State, http::StatusCode};
use chrono::{Duration, Utc};
use sea_orm::prelude::DateTimeUtc;
use serde::{Deserialize, Serialize};

use crate::{
    auth::{
        api_key::{self, ApiKeyScope},
        extractor::SessionUser,
    },
    entities::api_key as api_key_entity,
    error::AppError,
    extract::{JsonBody, Path},
    state::AppState,
};

/// Most active keys one account may hold at once. Bounds abuse (a runaway script
/// minting keys) and keeps the management list manageable; revoke one to free a slot.
const MAX_API_KEYS_PER_USER: u64 = 25;
/// Upper bound on a key's human label.
const MAX_API_KEY_NAME_LEN: usize = 100;
/// Upper bound on a requested key lifetime (~10 years) — a sanity cap, not a policy.
const MAX_API_KEY_EXPIRY_DAYS: u32 = 3650;

// ---------- DTOs ----------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CreateApiKeyRequest {
    /// A human label so the user can tell their keys apart (e.g. "price-bot").
    pub name: String,
    /// `"read"` (GET-only) or `"read_write"`.
    pub scope: String,
    /// Optional lifetime in days; omitted / null means the key never expires.
    #[serde(default)]
    pub expires_in_days: Option<u32>,
}

/// The response to a successful create — the **only** time the plaintext `key`
/// leaves the server. The client must copy it now; it is unrecoverable afterwards.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CreatedApiKey {
    pub id: i32,
    pub name: String,
    pub scope: String,
    /// The full secret, shown exactly once. Present it as `Authorization: Bearer <key>`.
    pub key: String,
    /// The stored, non-secret head of the key (`tcgl_<8 hex>`) for later identification.
    pub key_prefix: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTimeUtc,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub expires_at: Option<DateTimeUtc>,
}

/// Non-secret metadata for one key — what the management list shows. Never carries
/// the plaintext or the hash.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ApiKeyInfo {
    pub id: i32,
    pub name: String,
    pub scope: String,
    pub key_prefix: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTimeUtc,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub last_used_at: Option<DateTimeUtc>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub expires_at: Option<DateTimeUtc>,
}

impl From<api_key_entity::Model> for ApiKeyInfo {
    fn from(m: api_key_entity::Model) -> Self {
        ApiKeyInfo {
            id: m.id,
            name: m.name,
            scope: m.scope,
            key_prefix: m.key_prefix,
            created_at: m.created_at,
            last_used_at: m.last_used_at,
            expires_at: m.expires_at,
        }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct ApiKeyList {
    pub data: Vec<ApiKeyInfo>,
}

// ---------- Handlers ----------

/// Create API key
///
/// `POST /api/auth/api-keys` — mint a new key for the signed-in user.
///
/// Validated synchronously: a blank/oversized name, an unknown scope, or an
/// out-of-range expiry is `422`; exceeding the per-user cap is `409`. Returns `201`
/// with the plaintext key **once**.
#[utoipa::path(
    post,
    path = "/api/auth/api-keys",
    tag = "API keys",
    security(("session" = [])),
    request_body = CreateApiKeyRequest,
    responses(
        (status = 201, description = "The minted key; the plaintext `key` is returned exactly once.", body = CreatedApiKey),
        (status = 401, description = "No or invalid session credential."),
        (status = 403, description = "An API-key credential was used; key management requires an interactive session."),
        (status = 409, description = "The per-user active-key limit is reached."),
        (status = 422, description = "A blank/oversized name, unknown scope, or out-of-range expiry."),
    ),
)]
pub async fn create_api_key(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    JsonBody(payload): JsonBody<CreateApiKeyRequest>,
) -> Result<(StatusCode, Json<CreatedApiKey>), AppError> {
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(AppError::Validation("name must not be empty".to_string()));
    }
    if name.chars().count() > MAX_API_KEY_NAME_LEN {
        return Err(AppError::Validation(format!(
            "name must be at most {MAX_API_KEY_NAME_LEN} characters"
        )));
    }

    let scope = ApiKeyScope::parse_request(&payload.scope)?;

    let expires_at = match payload.expires_in_days {
        None => None,
        Some(0) => {
            return Err(AppError::Validation(
                "expires_in_days must be a positive number of days".to_string(),
            ));
        }
        Some(days) if days > MAX_API_KEY_EXPIRY_DAYS => {
            return Err(AppError::Validation(format!(
                "expires_in_days must be at most {MAX_API_KEY_EXPIRY_DAYS}"
            )));
        }
        Some(days) => Some(Utc::now() + Duration::days(i64::from(days))),
    };

    let over_cap = || {
        AppError::Conflict(format!(
            "you already have the maximum of {MAX_API_KEYS_PER_USER} active api keys; revoke one first"
        ))
    };

    if api_key::count_active_for_user(&state.db, user.id).await? >= MAX_API_KEYS_PER_USER {
        return Err(over_cap());
    }

    let issued = api_key::issue_api_key(&state.db, user.id, name, scope, expires_at).await?;

    // The count check above and the insert aren't atomic, so concurrent creates could
    // each pass the check before any row lands and blow past the cap. Re-count now that
    // the row exists; if we overshot, soft-revoke the key we just made and 409. The
    // plaintext hasn't left the handler yet, so rolling it back is invisible to the
    // caller — and once the persisted count reaches the cap every later create 409s.
    if api_key::count_active_for_user(&state.db, user.id).await? > MAX_API_KEYS_PER_USER {
        api_key::revoke(&state.db, issued.model.id, user.id).await?;
        return Err(over_cap());
    }

    Ok((
        StatusCode::CREATED,
        Json(CreatedApiKey {
            id: issued.model.id,
            name: issued.model.name,
            scope: issued.model.scope,
            key: issued.plaintext,
            key_prefix: issued.model.key_prefix,
            created_at: issued.model.created_at,
            expires_at: issued.model.expires_at,
        }),
    ))
}

/// List API keys
///
/// `GET /api/auth/api-keys` — the signed-in user's active keys, newest first
/// (metadata only; the secret is never returned again).
#[utoipa::path(
    get,
    path = "/api/auth/api-keys",
    tag = "API keys",
    security(("session" = [])),
    responses(
        (status = 200, description = "The signed-in user's active keys, newest first (metadata only).", body = ApiKeyList),
        (status = 401, description = "No or invalid session credential."),
        (status = 403, description = "An API-key credential was used; key management requires an interactive session."),
    ),
)]
pub async fn list_api_keys(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
) -> Result<Json<ApiKeyList>, AppError> {
    let data = api_key::list_active_for_user(&state.db, user.id)
        .await?
        .into_iter()
        .map(ApiKeyInfo::from)
        .collect();
    Ok(Json(ApiKeyList { data }))
}

/// Revoke API key
///
/// `DELETE /api/auth/api-keys/{id}` — revoke one of the user's keys. `204` on
/// success (idempotent for an already-revoked key), `404` if the key doesn't exist
/// or belongs to another user (so key ids don't leak across accounts).
#[utoipa::path(
    delete,
    path = "/api/auth/api-keys/{id}",
    tag = "API keys",
    security(("session" = [])),
    params(("id" = i32, Path, description = "The key id to revoke")),
    responses(
        (status = 204, description = "Revoked (idempotent for an already-revoked key)."),
        (status = 401, description = "No or invalid session credential."),
        (status = 403, description = "An API-key credential was used; key management requires an interactive session."),
        (status = 404, description = "No such key, or it belongs to another user."),
    ),
)]
pub async fn revoke_api_key(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Path(id): Path<i32>,
) -> Result<StatusCode, AppError> {
    if api_key::revoke(&state.db, id, user.id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound("api key not found".to_string()))
    }
}

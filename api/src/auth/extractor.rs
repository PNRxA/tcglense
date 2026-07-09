use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};
use sea_orm::EntityTrait;

use crate::{
    auth::{
        api_key::{self, ApiKeyScope, KEY_PLAINTEXT_PREFIX},
        jwt::decode_token,
    },
    entities::{prelude::User, user},
    error::AppError,
    state::AppState,
};

/// How a request authenticated: an interactive session (a short-lived access JWT)
/// or a long-lived API key (carrying its scope). The extractors below key their
/// authorization decisions off this — e.g. only a session may manage keys, and only
/// a `read_write` key may drive a mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    Session,
    ApiKey(ApiKeyScope),
}

/// An authenticated principal: the resolved user plus how they authenticated.
pub struct Principal {
    pub user: user::Model,
    pub method: AuthMethod,
}

/// Resolve the `Authorization: Bearer <credential>` header to a [`Principal`].
///
/// The credential is either an access JWT or an API key; they share the header, so
/// we branch on the API key's `tcgl_` label (a JWT — three dot-separated base64url
/// segments — can never start with it, so the two can't collide). This is the one
/// seam the [`AuthUser`], [`WritableUser`], and [`SessionUser`] extractors share, so
/// every authenticated route resolves credentials identically.
async fn resolve_principal(parts: &Parts, state: &AppState) -> Result<Principal, AppError> {
    let header = parts
        .headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("missing authorization header".to_string()))?;

    let token = header
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| AppError::Unauthorized("malformed authorization header".to_string()))?;

    if token.starts_with(KEY_PLAINTEXT_PREFIX) {
        let resolved = api_key::resolve(&state.db, token).await?;
        return Ok(Principal {
            user: resolved.user,
            method: AuthMethod::ApiKey(resolved.scope),
        });
    }

    let claims = decode_token(token, &state.config)?;
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| AppError::Unauthorized("invalid token subject".to_string()))?;

    let user = User::find_by_id(user_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;

    Ok(Principal {
        user,
        method: AuthMethod::Session,
    })
}

/// Authenticates a **read** request via `Authorization: Bearer <token>`, accepting
/// either an interactive session JWT or **any** valid API key (read or read_write).
/// The wrapped `user::Model` is the request's owner, scoping every query. This is
/// the extractor for read endpoints (and the batch-count POSTs, which are reads).
pub struct AuthUser(pub user::Model);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(AuthUser(resolve_principal(parts, state).await?.user))
    }
}

/// Authenticates a **mutating** request. Accepts a session JWT or a `read_write`
/// API key; a valid but **read-only** key is authenticated yet not permitted, so it
/// is rejected with `403 Forbidden` (not 401 — the credential is real, it just lacks
/// the scope). Every collection/wish-list write handler uses this instead of
/// [`AuthUser`], so a read-only key can never drive a mutation.
pub struct WritableUser(pub user::Model);

impl FromRequestParts<AppState> for WritableUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let principal = resolve_principal(parts, state).await?;
        if let AuthMethod::ApiKey(scope) = principal.method
            && !scope.permits_write()
        {
            return Err(AppError::Forbidden(
                "this api key is read-only; a read_write key is required to modify data".to_string(),
            ));
        }
        Ok(WritableUser(principal.user))
    }
}

/// Authenticates a request that must come from a real interactive **session** (a
/// JWT), never an API key. An otherwise-valid API key is rejected with `403
/// Forbidden`. Used for API-key management (create/list/revoke) so a compromised key
/// can neither mint more keys nor enumerate/revoke its siblings.
pub struct SessionUser(pub user::Model);

impl FromRequestParts<AppState> for SessionUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let principal = resolve_principal(parts, state).await?;
        if principal.method != AuthMethod::Session {
            return Err(AppError::Forbidden(
                "api keys cannot manage api keys; sign in to manage keys".to_string(),
            ));
        }
        Ok(SessionUser(principal.user))
    }
}

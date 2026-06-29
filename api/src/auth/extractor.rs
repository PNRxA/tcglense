use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};
use sea_orm::EntityTrait;

use crate::{
    auth::jwt::decode_token,
    entities::{prelude::User, user},
    error::AppError,
    state::AppState,
};

/// Axum extractor that authenticates a request via the
/// `Authorization: Bearer <token>` header and loads the corresponding user.
pub struct AuthUser(pub user::Model);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
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

        let claims = decode_token(token, &state.config)?;

        let user_id: i32 = claims
            .sub
            .parse()
            .map_err(|_| AppError::Unauthorized("invalid token subject".to_string()))?;

        let user = User::find_by_id(user_id)
            .one(&state.db)
            .await?
            .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;

        Ok(AuthUser(user))
    }
}

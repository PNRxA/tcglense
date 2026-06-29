use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use crate::{config::Config, entities::user, error::AppError};

/// JWT claims. `sub` is the user id as a string; `iat`/`exp` are unix seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    pub iat: usize,
    pub exp: usize,
}

/// Encode a signed HS256 JWT for the given user.
pub fn encode_token(user: &user::Model, config: &Config) -> Result<String, AppError> {
    let now = Utc::now();
    let iat = now.timestamp().max(0) as usize;
    let exp = (now + Duration::minutes(config.access_token_expiry_minutes))
        .timestamp()
        .max(0) as usize;

    let claims = Claims {
        sub: user.id.to_string(),
        email: user.email.clone(),
        iat,
        exp,
    };

    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("failed to encode token: {e}")))
}

/// Decode and validate an HS256 JWT, returning its claims.
///
/// The validation requires an `exp` claim and restricts the accepted algorithm
/// to HS256, defending against `alg:none` / algorithm-confusion attacks.
pub fn decode_token(token: &str, config: &Config) -> Result<Claims, AppError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.algorithms = vec![Algorithm::HS256];
    validation.validate_exp = true;
    validation.set_required_spec_claims(&["exp"]);
    // We do not use an audience claim; disable aud validation explicitly.
    validation.validate_aud = false;

    decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|_| AppError::Unauthorized("invalid or expired token".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn test_config() -> Config {
        Config {
            database_url: "sqlite::memory:".to_string(),
            jwt_secret: "test-secret-key-for-unit-tests".to_string(),
            access_token_expiry_minutes: 15,
            refresh_token_expiry_days: 30,
            cookie_secure: false,
            host: "127.0.0.1".to_string(),
            port: 8080,
            data_dir: std::path::PathBuf::from("./data"),
            scryfall_user_agent: "TCGLense/test".to_string(),
            sync_on_startup: false,
            sync_interval_hours: 24,
            seed_dummy_data: false,
        }
    }

    fn test_user() -> user::Model {
        let now = Utc::now();
        user::Model {
            id: 42,
            email: "tester@example.com".to_string(),
            password_hash: "irrelevant".to_string(),
            display_name: Some("Tester".to_string()),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn encode_then_decode_roundtrip() {
        let config = test_config();
        let user = test_user();

        let token = encode_token(&user, &config).expect("encoding should succeed");
        let claims = decode_token(&token, &config).expect("decoding should succeed");

        assert_eq!(claims.sub, "42");
        assert_eq!(claims.email, "tester@example.com");
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn decode_with_wrong_secret_fails() {
        let config = test_config();
        let user = test_user();
        let token = encode_token(&user, &config).expect("encoding should succeed");

        let mut other = test_config();
        other.jwt_secret = "a-completely-different-secret".to_string();

        assert!(decode_token(&token, &other).is_err());
    }

    #[test]
    fn decode_rejects_garbage_token() {
        let config = test_config();
        assert!(decode_token("not.a.jwt", &config).is_err());
    }
}

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

    /// Minimal URL-safe base64 (no padding) — just enough to hand-craft the
    /// header/payload of an `alg:none` token the `jsonwebtoken` encoder refuses
    /// to emit.
    fn b64url(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = *chunk.get(1).unwrap_or(&0) as u32;
            let b2 = *chunk.get(2).unwrap_or(&0) as u32;
            let n = (b0 << 16) | (b1 << 8) | b2;
            let sextets = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
            // 1 input byte -> 2 output chars, 2 -> 3, 3 -> 4.
            for &s in sextets.iter().take(chunk.len() + 1) {
                out.push(ALPHABET[s as usize] as char);
            }
        }
        out
    }

    #[test]
    fn decode_rejects_alg_none_token() {
        // The classic algorithm-confusion / `alg:none` forgery: a token with a
        // future expiry but no signature. Validation pins the algorithm to HS256,
        // so it must be rejected on the algorithm before exp is even considered.
        let config = test_config();
        let far_future = (Utc::now().timestamp() + 3600) as usize;
        let header = b64url(br#"{"alg":"none","typ":"JWT"}"#);
        let payload = b64url(
            format!(r#"{{"sub":"42","email":"tester@example.com","iat":0,"exp":{far_future}}}"#)
                .as_bytes(),
        );
        let forged = format!("{header}.{payload}.");
        assert!(decode_token(&forged, &config).is_err());
    }

    #[test]
    fn decode_rejects_expired_token() {
        let config = test_config();
        // exp an hour in the past — well outside jsonwebtoken's default leeway.
        let past = (Utc::now() - Duration::hours(1)).timestamp() as usize;
        let claims = Claims {
            sub: "42".to_string(),
            email: "tester@example.com".to_string(),
            iat: past - 60,
            exp: past,
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
        )
        .expect("encode expired token");
        assert!(decode_token(&token, &config).is_err());
    }

    #[test]
    fn decode_rejects_token_missing_exp() {
        // `exp` is a required claim; a token without one (no expiry) is rejected.
        let config = test_config();
        let claims = serde_json::json!({
            "sub": "42",
            "email": "tester@example.com",
            "iat": 0,
        });
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
        )
        .expect("encode exp-less token");
        assert!(decode_token(&token, &config).is_err());
    }

    #[test]
    fn decode_rejects_tampered_signature() {
        // A valid token whose signature has been altered must not verify — proves
        // the payload is cryptographically bound to the signature.
        let config = test_config();
        let token = encode_token(&test_user(), &config).expect("encode");
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        // Deterministically corrupt the first signature character (the high bits of
        // the HMAC's first byte) so the signature is guaranteed different and the
        // test can never coincidentally produce the valid signature.
        let sig = parts[2];
        let replacement = if &sig[0..1] == "A" { "B" } else { "A" };
        let tampered_sig = format!("{replacement}{}", &sig[1..]);
        let tampered = format!("{}.{}.{}", parts[0], parts[1], tampered_sig);
        assert_ne!(tampered, token);
        assert!(decode_token(&tampered, &config).is_err());
    }
}

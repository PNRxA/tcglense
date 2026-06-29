use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};

use crate::error::AppError;

/// Argon2id hasher with explicitly pinned parameters (OWASP-recommended floor:
/// 19 MiB memory, 2 iterations, 1 lane). Pinning the algorithm, version, and cost
/// here means a future change to the crate's defaults can't silently weaken
/// hashing. Verification reads its cost from the stored PHC string, so old hashes
/// keep verifying even if these values are raised later.
fn hasher() -> Argon2<'static> {
    let params = Params::new(19_456, 2, 1, None).expect("static argon2 params are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Hash a plaintext password into a PHC string using Argon2id with a fresh random salt.
pub fn hash_password(plain: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    hasher()
        .hash_password(plain.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AppError::Internal(format!("failed to hash password: {e}")))
}

/// Verify a plaintext password against a stored PHC hash string.
/// Returns `false` for any mismatch or malformed hash (never panics).
pub fn verify_password(hash: &str, plain: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => hasher().verify_password(plain.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_roundtrip() {
        let password = "correct horse battery staple";
        let hash = hash_password(password).expect("hashing should succeed");

        // The produced value is a PHC string, not the plaintext.
        assert!(hash.starts_with("$argon2"));
        assert_ne!(hash, password);

        // Correct password verifies.
        assert!(verify_password(&hash, password));

        // Wrong password fails.
        assert!(!verify_password(&hash, "wrong password"));

        // Malformed hash never panics and returns false.
        assert!(!verify_password("not-a-valid-phc-string", password));
    }

    #[test]
    fn distinct_salts_produce_distinct_hashes() {
        let password = "another-password-123";
        let a = hash_password(password).expect("hash a");
        let b = hash_password(password).expect("hash b");
        assert_ne!(a, b, "random salt should yield different hashes");
        assert!(verify_password(&a, password));
        assert!(verify_password(&b, password));
    }
}

//! Shared opaque-secret primitives for the auth token stores.
//!
//! The three auth credential stores (refresh tokens, single-use email tokens,
//! and API keys) all mint a high-entropy opaque secret and persist only its
//! SHA-256 hex digest. Both operations are extracted here so the three modules
//! share one implementation.
//!
//! Because the generated secret is already 32 uniformly-random bytes, a fast
//! cryptographic hash (SHA-256) is the correct choice for hashing it — argon2 is
//! for low-entropy passwords — and a plain hex digest lets the auth path resolve
//! a presented secret with a single indexed lookup on the stored hash.

use rand::Rng;
use sha2::{Digest, Sha256};

/// Generate a new opaque secret: 32 CSPRNG bytes, hex-encoded (64 hex chars).
pub(crate) fn generate_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// SHA-256 hex digest of the opaque secret (what the stores persist / look up by).
pub(crate) fn sha256_hex(input: &str) -> String {
    hex::encode(Sha256::digest(input.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_secrets_are_distinct_64_hex_chars() {
        let a = generate_secret();
        let b = generate_secret();
        // 32 bytes -> 64 hex chars.
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }

    #[test]
    fn sha256_hex_is_deterministic() {
        assert_eq!(sha256_hex("hello"), sha256_hex("hello"));
        assert_ne!(sha256_hex("hello"), sha256_hex("world"));
    }
}

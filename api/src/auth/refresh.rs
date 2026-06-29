//! Opaque refresh-token service.
//!
//! Refresh tokens are high-entropy random values (32 bytes, hex-encoded). Only
//! their SHA-256 hex digest is persisted; the plaintext is returned to the
//! caller exactly once (to be set as an httpOnly cookie) and never logged.
//!
//! Because the token is already uniformly random, a fast cryptographic hash
//! (SHA-256) is the correct choice here — argon2 is for low-entropy passwords.

use chrono::{Duration, Utc};
use rand::Rng;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
    sea_query::Expr,
};
use sha2::{Digest, Sha256};

use crate::{
    entities::{prelude::RefreshToken, refresh_token},
    error::AppError,
};

/// Outcome of a successful rotation: the new plaintext token (for the cookie)
/// and the owning user id (to mint a fresh access token).
#[derive(Debug)]
pub struct RotatedToken {
    pub plaintext: String,
    pub user_id: i32,
}

/// Generate a new opaque refresh token: 32 CSPRNG bytes, hex-encoded.
fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// SHA-256 hex digest of the opaque token (what we store / look up by).
fn hash_token(plaintext: &str) -> String {
    hex::encode(Sha256::digest(plaintext.as_bytes()))
}

/// Issue a brand-new refresh token for `user_id`, persisting only its hash.
/// Returns the PLAINTEXT token (the only time it ever leaves this module).
pub async fn issue_refresh_token(
    db: &DatabaseConnection,
    user_id: i32,
    expiry_days: i64,
) -> Result<String, AppError> {
    let plaintext = generate_token();
    let now = Utc::now();

    let row = refresh_token::ActiveModel {
        user_id: Set(user_id),
        token_hash: Set(hash_token(&plaintext)),
        expires_at: Set(now + Duration::days(expiry_days)),
        revoked_at: Set(None),
        created_at: Set(now),
        ..Default::default()
    };
    row.insert(db).await?;

    Ok(plaintext)
}

/// Rotate the presented refresh token.
///
/// * not found            -> `Unauthorized`
/// * found but revoked     -> REUSE/theft: revoke ALL of the user's tokens, then `Unauthorized`
/// * found but expired     -> `Unauthorized`
/// * valid                 -> mark revoked, issue a replacement, return it
pub async fn rotate(
    db: &DatabaseConnection,
    presented_plaintext: &str,
    expiry_days: i64,
) -> Result<RotatedToken, AppError> {
    let token_hash = hash_token(presented_plaintext);
    let now = Utc::now();

    // Atomically claim the token by flipping revoked_at NULL -> now in a single
    // conditional UPDATE. Only one concurrent caller can match the
    // `RevokedAt IS NULL` predicate, so exactly one rotation wins per presented
    // token — this is the single-use invariant that makes reuse detection sound
    // (a read-then-update across awaits would let two requests both "win").
    let claimed = RefreshToken::update_many()
        .col_expr(refresh_token::Column::RevokedAt, Expr::value(now))
        .filter(refresh_token::Column::TokenHash.eq(&token_hash))
        .filter(refresh_token::Column::RevokedAt.is_null())
        .exec(db)
        .await?;

    if claimed.rows_affected == 0 {
        // We did not claim it: the token is either unknown or was already
        // revoked. Re-read to distinguish — a replayed (already-revoked) token
        // is reuse/theft, so burn the whole family for that user.
        if let Some(row) = RefreshToken::find()
            .filter(refresh_token::Column::TokenHash.eq(&token_hash))
            .one(db)
            .await?
        {
            revoke_all_for_user(db, row.user_id).await?;
        }
        return Err(AppError::Unauthorized("invalid refresh token".to_string()));
    }

    // We hold the claim; load the (now-revoked) row for its owner + expiry.
    let row = RefreshToken::find()
        .filter(refresh_token::Column::TokenHash.eq(&token_hash))
        .one(db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid refresh token".to_string()))?;

    // The token was unrevoked when we claimed it, but it may have been past its
    // expiry; we have already revoked it, so simply reject without replacing it.
    if row.expires_at <= now {
        return Err(AppError::Unauthorized("invalid refresh token".to_string()));
    }

    let user_id = row.user_id;
    let plaintext = issue_refresh_token(db, user_id, expiry_days).await?;

    Ok(RotatedToken { plaintext, user_id })
}

/// Revoke the single refresh token matching `presented_plaintext` (logout).
/// Idempotent: a missing / already-revoked / unknown token is a no-op success.
pub async fn revoke_one(
    db: &DatabaseConnection,
    presented_plaintext: &str,
) -> Result<(), AppError> {
    let token_hash = hash_token(presented_plaintext);

    let row = RefreshToken::find()
        .filter(refresh_token::Column::TokenHash.eq(&token_hash))
        .one(db)
        .await?;

    if let Some(row) = row {
        if row.revoked_at.is_none() {
            let mut active: refresh_token::ActiveModel = row.into();
            active.revoked_at = Set(Some(Utc::now()));
            active.update(db).await?;
        }
    }

    Ok(())
}

/// Revoke every still-active refresh token belonging to `user_id`.
async fn revoke_all_for_user(db: &DatabaseConnection, user_id: i32) -> Result<(), AppError> {
    RefreshToken::update_many()
        .col_expr(refresh_token::Column::RevokedAt, Expr::value(Utc::now()))
        .filter(refresh_token::Column::UserId.eq(user_id))
        .filter(refresh_token::Column::RevokedAt.is_null())
        .exec(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{entities::user, migrator::Migrator};
    use sea_orm::Database;
    use sea_orm_migration::MigratorTrait;

    async fn setup_db() -> DatabaseConnection {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("connect to in-memory sqlite");
        Migrator::up(&db, None).await.expect("run migrations");
        db
    }

    async fn insert_user(db: &DatabaseConnection, email: &str) -> i32 {
        let now = Utc::now();
        let model = user::ActiveModel {
            email: Set(email.to_string()),
            password_hash: Set("irrelevant".to_string()),
            display_name: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert user");
        model.id
    }

    async fn find_by_hash(db: &DatabaseConnection, plaintext: &str) -> refresh_token::Model {
        RefreshToken::find()
            .filter(refresh_token::Column::TokenHash.eq(hash_token(plaintext)))
            .one(db)
            .await
            .expect("query")
            .expect("row exists")
    }

    #[test]
    fn generated_tokens_are_distinct_and_hash_is_deterministic() {
        let a = generate_token();
        let b = generate_token();
        // 32 bytes -> 64 hex chars.
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert_eq!(hash_token(&a), hash_token(&a));
        assert_ne!(hash_token(&a), hash_token(&b));
    }

    #[tokio::test]
    async fn rotate_revokes_old_and_new_token_hash_differs() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "rotate@example.com").await;

        let original = issue_refresh_token(&db, user_id, 30).await.expect("issue");
        let original_hash = hash_token(&original);

        let rotated = rotate(&db, &original, 30).await.expect("rotate");

        assert_eq!(rotated.user_id, user_id);
        // A rotated (revoked) token's replacement has a different hash.
        assert_ne!(rotated.plaintext, original);
        assert_ne!(hash_token(&rotated.plaintext), original_hash);

        // Old row is now revoked, new row is active.
        assert!(find_by_hash(&db, &original).await.revoked_at.is_some());
        assert!(find_by_hash(&db, &rotated.plaintext).await.revoked_at.is_none());
    }

    #[tokio::test]
    async fn rotate_unknown_token_is_unauthorized() {
        let db = setup_db().await;
        let err = rotate(&db, "not-a-real-token", 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn rotate_expired_token_is_unauthorized() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "expired@example.com").await;
        // Negative expiry => already past.
        let expired = issue_refresh_token(&db, user_id, -1).await.expect("issue");
        let err = rotate(&db, &expired, 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn reuse_of_revoked_token_revokes_all_user_tokens() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "reuse@example.com").await;

        let t1 = issue_refresh_token(&db, user_id, 30).await.expect("t1");
        let t2 = issue_refresh_token(&db, user_id, 30).await.expect("t2");

        // First rotation succeeds and revokes t1, issuing t3.
        let rotated = rotate(&db, &t1, 30).await.expect("rotate t1");

        // Presenting the already-revoked t1 again is detected as reuse.
        let err = rotate(&db, &t1, 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));

        // The whole family is now revoked (t2 and the freshly-issued t3).
        assert!(find_by_hash(&db, &t2).await.revoked_at.is_some());
        assert!(find_by_hash(&db, &rotated.plaintext).await.revoked_at.is_some());
    }

    #[tokio::test]
    async fn revoke_one_is_idempotent() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "logout@example.com").await;
        let token = issue_refresh_token(&db, user_id, 30).await.expect("issue");

        revoke_one(&db, &token).await.expect("first revoke");
        revoke_one(&db, &token).await.expect("second revoke is a no-op");
        revoke_one(&db, "unknown-token").await.expect("unknown is a no-op");

        assert!(find_by_hash(&db, &token).await.revoked_at.is_some());
    }
}

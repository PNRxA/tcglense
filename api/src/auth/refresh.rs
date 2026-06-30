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

/// A freshly-inserted refresh token: the plaintext (for the cookie) and the new
/// row's id (so a rotated predecessor can record it as its successor).
struct InsertedToken {
    plaintext: String,
    id: i32,
}

/// Insert a brand-new refresh token for `user_id`, persisting only its hash.
async fn insert_token(
    db: &DatabaseConnection,
    user_id: i32,
    expiry_days: i64,
) -> Result<InsertedToken, AppError> {
    let plaintext = generate_token();
    let now = Utc::now();

    let model = refresh_token::ActiveModel {
        user_id: Set(user_id),
        token_hash: Set(hash_token(&plaintext)),
        expires_at: Set(now + Duration::days(expiry_days)),
        revoked_at: Set(None),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(InsertedToken {
        plaintext,
        id: model.id,
    })
}

/// Issue a brand-new refresh token for `user_id`, persisting only its hash.
/// Returns the PLAINTEXT token (the only time it ever leaves this module).
pub async fn issue_refresh_token(
    db: &DatabaseConnection,
    user_id: i32,
    expiry_days: i64,
) -> Result<String, AppError> {
    Ok(insert_token(db, user_id, expiry_days).await?.plaintext)
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
        // We did not claim it: the token is either unknown or already revoked.
        // Re-read to distinguish reuse/theft from a benign concurrent retry.
        if let Some(row) = RefreshToken::find()
            .filter(refresh_token::Column::TokenHash.eq(&token_hash))
            .one(db)
            .await?
        {
            // A rotated token records its successor. If that successor has ITSELF
            // been revoked (used/rotated/logged out), replaying this superseded
            // token is genuine reuse — burn the whole family. A still-active
            // successor (or none at all, e.g. a logged-out or concurrently-claimed
            // token) is not evidence of theft, so we just reject this request.
            // Either way a revoked token is NEVER exchanged for a new one.
            if let Some(successor_id) = row.replaced_by_id {
                let successor_revoked = RefreshToken::find_by_id(successor_id)
                    .one(db)
                    .await?
                    .is_some_and(|s| s.revoked_at.is_some());
                if successor_revoked {
                    revoke_all_for_user(db, row.user_id).await?;
                }
            }
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
    let successor = insert_token(db, user_id, expiry_days).await?;

    // Record the successor so a later replay of this token can be told apart from
    // a benign concurrent double-submit (whose successor is still active).
    let mut rotated: refresh_token::ActiveModel = row.into();
    rotated.replaced_by_id = Set(Some(successor.id));
    rotated.update(db).await?;

    Ok(RotatedToken {
        plaintext: successor.plaintext,
        user_id,
    })
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

    if let Some(row) = row
        && row.revoked_at.is_none()
    {
        let mut active: refresh_token::ActiveModel = row.into();
        active.revoked_at = Set(Some(Utc::now()));
        active.update(db).await?;
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

/// Delete refresh tokens that are already past their expiry. Expired tokens are
/// rejected on use regardless, so removing them only bounds table growth. Returns
/// the number of rows pruned.
pub async fn prune_expired(db: &DatabaseConnection) -> Result<u64, AppError> {
    let result = RefreshToken::delete_many()
        .filter(refresh_token::Column::ExpiresAt.lte(Utc::now()))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
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
        let old_row = find_by_hash(&db, &original).await;
        let new_row = find_by_hash(&db, &rotated.plaintext).await;
        assert!(old_row.revoked_at.is_some());
        assert!(new_row.revoked_at.is_none());
        // The old row records its successor for reuse detection.
        assert_eq!(old_row.replaced_by_id, Some(new_row.id));
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
    async fn replay_of_superseded_token_revokes_all_user_tokens() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "reuse@example.com").await;

        let t1 = issue_refresh_token(&db, user_id, 30).await.expect("t1");
        let t2 = issue_refresh_token(&db, user_id, 30)
            .await
            .expect("t2 (other device)");

        // Rotate t1 -> t3, then t3 -> t4, so t1's successor (t3) is itself revoked.
        let r3 = rotate(&db, &t1, 30).await.expect("rotate t1 -> t3");
        let r4 = rotate(&db, &r3.plaintext, 30)
            .await
            .expect("rotate t3 -> t4");

        // Replaying the long-superseded t1 is now unambiguous reuse/theft.
        let err = rotate(&db, &t1, 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));

        // The whole family is burned: the unrelated t2 and the live t4 are revoked.
        assert!(find_by_hash(&db, &t2).await.revoked_at.is_some());
        assert!(find_by_hash(&db, &r4.plaintext).await.revoked_at.is_some());
    }

    #[tokio::test]
    async fn concurrent_double_submit_does_not_revoke_family() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "concurrent@example.com").await;

        let t1 = issue_refresh_token(&db, user_id, 30).await.expect("t1");

        // First rotation wins and issues the successor t2.
        let r2 = rotate(&db, &t1, 30).await.expect("rotate t1 -> t2");

        // A near-simultaneous second request carried the same just-rotated t1. Its
        // successor t2 is still active, so this is a benign retry: the request is
        // rejected but the family is NOT burned.
        let err = rotate(&db, &t1, 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));

        // t2 remains usable — the session survives the concurrent refresh.
        assert!(find_by_hash(&db, &r2.plaintext).await.revoked_at.is_none());
        rotate(&db, &r2.plaintext, 30)
            .await
            .expect("t2 still rotatable");
    }

    #[tokio::test]
    async fn prune_expired_removes_only_expired_tokens() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "prune@example.com").await;

        let live = issue_refresh_token(&db, user_id, 30).await.expect("live");
        let expired = issue_refresh_token(&db, user_id, -1)
            .await
            .expect("expired");

        let pruned = prune_expired(&db).await.expect("prune");
        assert_eq!(pruned, 1);

        let live_exists = RefreshToken::find()
            .filter(refresh_token::Column::TokenHash.eq(hash_token(&live)))
            .one(&db)
            .await
            .expect("query")
            .is_some();
        let expired_exists = RefreshToken::find()
            .filter(refresh_token::Column::TokenHash.eq(hash_token(&expired)))
            .one(&db)
            .await
            .expect("query")
            .is_some();
        assert!(live_exists, "live token should survive pruning");
        assert!(!expired_exists, "expired token should be pruned");
    }

    #[tokio::test]
    async fn revoke_one_is_idempotent() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "logout@example.com").await;
        let token = issue_refresh_token(&db, user_id, 30).await.expect("issue");

        revoke_one(&db, &token).await.expect("first revoke");
        revoke_one(&db, &token)
            .await
            .expect("second revoke is a no-op");
        revoke_one(&db, "unknown-token")
            .await
            .expect("unknown is a no-op");

        assert!(find_by_hash(&db, &token).await.revoked_at.is_some());
    }
}

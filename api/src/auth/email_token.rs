//! Single-use email-token service backing the verification and password-reset
//! links.
//!
//! Mirrors the refresh-token storage design (see [`super::refresh`]): tokens are
//! high-entropy random values (32 bytes, hex-encoded), only their SHA-256 hex
//! digest is persisted, and the plaintext is returned to the caller exactly once
//! (to be embedded in an emailed link) and never logged. Unlike refresh tokens
//! there is no rotation or successor lineage — an email token is spent exactly
//! once (an atomic conditional `UPDATE` on `consumed_at`) and never replaced.

use chrono::{Duration, Utc};
use rand::Rng;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Set, Statement, sea_query::Expr,
};
use sha2::{Digest, Sha256};

use crate::{
    entities::{email_token, prelude::EmailToken},
    error::AppError,
};

/// Minimum age of a user's newest token (per purpose) before another one is
/// issued via [`issue_with_cooldown`] — a DB-backed brake on mail-bombing an
/// address through the resend/forgot endpoints.
const ISSUE_COOLDOWN_SECONDS: i64 = 60;

/// What an emailed token authorizes. Stored as a string discriminator on the row
/// and filtered in the consuming `UPDATE`, so a verification token can never be
/// spent as a password reset (or vice versa).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailTokenPurpose {
    VerifyEmail,
    ResetPassword,
}

impl EmailTokenPurpose {
    fn as_str(self) -> &'static str {
        match self {
            Self::VerifyEmail => "verify_email",
            Self::ResetPassword => "reset_password",
        }
    }

    /// Token lifetime: generous for verification (the user may open the mail
    /// much later), tight for password resets (a live credential-changer).
    fn expiry(self) -> Duration {
        match self {
            Self::VerifyEmail => Duration::hours(24),
            Self::ResetPassword => Duration::hours(1),
        }
    }
}

/// Generate a new opaque email token: 32 CSPRNG bytes, hex-encoded.
fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// SHA-256 hex digest of the opaque token (what we store / look up by).
fn hash_token(plaintext: &str) -> String {
    hex::encode(Sha256::digest(plaintext.as_bytes()))
}

/// Insert a token row for `user_id` expiring at `now + expiry`, persisting only
/// its hash. Split from [`issue`] so tests can plant an already-expired row.
async fn insert_token(
    db: &DatabaseConnection,
    user_id: i32,
    purpose: EmailTokenPurpose,
    expiry: Duration,
) -> Result<String, AppError> {
    let plaintext = generate_token();
    let now = Utc::now();

    email_token::ActiveModel {
        user_id: Set(user_id),
        purpose: Set(purpose.as_str().to_string()),
        token_hash: Set(hash_token(&plaintext)),
        expires_at: Set(now + expiry),
        consumed_at: Set(None),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(plaintext)
}

/// Issue a brand-new email token for `user_id`, persisting only its hash.
/// Returns the PLAINTEXT token (the only time it ever leaves this module).
pub async fn issue(
    db: &DatabaseConnection,
    user_id: i32,
    purpose: EmailTokenPurpose,
) -> Result<String, AppError> {
    insert_token(db, user_id, purpose, purpose.expiry()).await
}

/// Like [`issue`], but returns `Ok(None)` — issuing nothing — when the user
/// already has a token of this purpose younger than the cooldown. Callers on the
/// anti-enumeration endpoints (resend-verification, forgot-password) treat
/// `None` exactly like a success so the cooldown is unobservable from outside.
///
/// The check and the insert are a **single atomic statement** (a conditional
/// `INSERT … WHERE NOT EXISTS(recent row)`, gated on `rows_affected` like the
/// rotation/[`consume`] claims): a plain read-then-insert would let a burst of
/// concurrent requests all pass the check before any row committed and each send
/// an email, defeating the very mail-bombing brake this exists to be.
pub async fn issue_with_cooldown(
    db: &DatabaseConnection,
    user_id: i32,
    purpose: EmailTokenPurpose,
) -> Result<Option<String>, AppError> {
    let plaintext = generate_token();
    let now = Utc::now();
    let cutoff = now - Duration::seconds(ISSUE_COOLDOWN_SECONDS);

    // The datetime values bind as chrono `Value`s (same encoding SeaORM uses to
    // store `created_at`), so the `created_at > cutoff` text comparison lines up.
    // All values are bound parameters — nothing is interpolated into the SQL.
    let stmt = Statement::from_sql_and_values(
        db.get_database_backend(),
        "INSERT INTO email_tokens \
             (user_id, purpose, token_hash, expires_at, consumed_at, created_at) \
         SELECT ?, ?, ?, ?, NULL, ? \
         WHERE NOT EXISTS ( \
             SELECT 1 FROM email_tokens \
             WHERE user_id = ? AND purpose = ? AND created_at > ? \
         )",
        [
            user_id.into(),
            purpose.as_str().into(),
            hash_token(&plaintext).into(),
            (now + purpose.expiry()).into(),
            now.into(),
            user_id.into(),
            purpose.as_str().into(),
            cutoff.into(),
        ],
    );

    let result = db.execute(stmt).await?;
    Ok((result.rows_affected() > 0).then_some(plaintext))
}

/// Consume the presented token: spend it if it is a live, unconsumed token of
/// the expected purpose, returning its row (for the owning `user_id`).
///
/// * unknown / wrong purpose / already consumed -> `Unauthorized`
/// * consumed here but past expiry              -> `Unauthorized` (stays spent)
pub async fn consume(
    db: &DatabaseConnection,
    presented_plaintext: &str,
    purpose: EmailTokenPurpose,
) -> Result<email_token::Model, AppError> {
    let token_hash = hash_token(presented_plaintext);
    let now = Utc::now();
    let invalid = || AppError::Unauthorized("invalid or expired token".to_string());

    // Atomically claim the token by flipping consumed_at NULL -> now in a single
    // conditional UPDATE (the same single-use idiom as refresh-token rotation):
    // only one concurrent caller can match the `ConsumedAt IS NULL` predicate.
    let claimed = EmailToken::update_many()
        .col_expr(email_token::Column::ConsumedAt, Expr::value(now))
        .filter(email_token::Column::TokenHash.eq(&token_hash))
        .filter(email_token::Column::Purpose.eq(purpose.as_str()))
        .filter(email_token::Column::ConsumedAt.is_null())
        .exec(db)
        .await?;

    if claimed.rows_affected == 0 {
        return Err(invalid());
    }

    // We hold the claim; load the (now-consumed) row for its owner + expiry.
    let row = EmailToken::find()
        .filter(email_token::Column::TokenHash.eq(&token_hash))
        .one(db)
        .await?
        .ok_or_else(invalid)?;

    // The token was unconsumed when we claimed it, but it may have been past its
    // expiry; we have already spent it, so simply reject (the user requests a
    // fresh link).
    if row.expires_at <= now {
        return Err(invalid());
    }

    Ok(row)
}

/// Delete email tokens that are already past their expiry. Expired tokens are
/// rejected on use regardless, so removing them only bounds table growth.
/// Returns the number of rows pruned.
pub async fn prune_expired(db: &DatabaseConnection) -> Result<u64, AppError> {
    let result = EmailToken::delete_many()
        .filter(email_token::Column::ExpiresAt.lte(Utc::now()))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::insert_user;

    async fn setup_db() -> DatabaseConnection {
        crate::test_support::migrated_memory_db().await
    }

    #[tokio::test]
    async fn issue_then_consume_returns_the_owner_and_spends_the_token() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "verify@example.com").await;

        let token = issue(&db, user_id, EmailTokenPurpose::VerifyEmail)
            .await
            .expect("issue");
        assert_eq!(token.len(), 64); // 32 bytes -> 64 hex chars

        let row = consume(&db, &token, EmailTokenPurpose::VerifyEmail)
            .await
            .expect("consume");
        assert_eq!(row.user_id, user_id);
        assert!(row.consumed_at.is_some());

        // Single-use: a second consumption of the same token is rejected.
        let err = consume(&db, &token, EmailTokenPurpose::VerifyEmail)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn a_token_cannot_be_spent_for_another_purpose() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "purpose@example.com").await;

        let token = issue(&db, user_id, EmailTokenPurpose::VerifyEmail)
            .await
            .expect("issue");

        // A verification token presented as a password reset must fail...
        let err = consume(&db, &token, EmailTokenPurpose::ResetPassword)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));

        // ...without spending it for its real purpose.
        consume(&db, &token, EmailTokenPurpose::VerifyEmail)
            .await
            .expect("still consumable for its own purpose");
    }

    #[tokio::test]
    async fn unknown_and_expired_tokens_are_rejected() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "expired@example.com").await;

        let err = consume(&db, "not-a-real-token", EmailTokenPurpose::VerifyEmail)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));

        // Plant an already-expired token; consuming it must fail.
        let expired = insert_token(
            &db,
            user_id,
            EmailTokenPurpose::VerifyEmail,
            Duration::hours(-1),
        )
        .await
        .expect("insert expired");
        let err = consume(&db, &expired, EmailTokenPurpose::VerifyEmail)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn cooldown_suppresses_back_to_back_issues_per_purpose() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "cooldown@example.com").await;

        let first = issue_with_cooldown(&db, user_id, EmailTokenPurpose::VerifyEmail)
            .await
            .expect("first issue");
        assert!(first.is_some());

        // Immediately asking again is inside the cooldown window -> nothing issued.
        let second = issue_with_cooldown(&db, user_id, EmailTokenPurpose::VerifyEmail)
            .await
            .expect("second issue");
        assert!(second.is_none());

        // A different purpose has its own window.
        let reset = issue_with_cooldown(&db, user_id, EmailTokenPurpose::ResetPassword)
            .await
            .expect("reset issue");
        assert!(reset.is_some());
    }

    #[tokio::test]
    async fn concurrent_cooldown_issues_are_atomic() {
        // A burst of concurrent requests (as an anonymous client could fire at
        // forgot-password/resend) must still issue at most ONE token in the
        // window — the cooldown is an atomic conditional insert, not a racy
        // check-then-insert.
        let db = setup_db().await;
        let user_id = insert_user(&db, "burst@example.com").await;

        let mut handles = Vec::new();
        for _ in 0..20 {
            let db = db.clone();
            handles.push(tokio::spawn(async move {
                issue_with_cooldown(&db, user_id, EmailTokenPurpose::ResetPassword).await
            }));
        }

        let mut issued = 0;
        for handle in handles {
            if handle.await.expect("task").expect("issue").is_some() {
                issued += 1;
            }
        }
        assert_eq!(issued, 1, "exactly one token may be issued within the cooldown");

        // And exactly one row landed.
        let rows = EmailToken::find()
            .filter(email_token::Column::UserId.eq(user_id))
            .all(&db)
            .await
            .expect("query");
        assert_eq!(rows.len(), 1);
    }

    #[tokio::test]
    async fn prune_expired_removes_only_expired_tokens() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "prune@example.com").await;

        let live = issue(&db, user_id, EmailTokenPurpose::VerifyEmail)
            .await
            .expect("live");
        insert_token(
            &db,
            user_id,
            EmailTokenPurpose::VerifyEmail,
            Duration::hours(-1),
        )
        .await
        .expect("expired");

        let pruned = prune_expired(&db).await.expect("prune");
        assert_eq!(pruned, 1);

        // The live token is untouched and still consumable.
        consume(&db, &live, EmailTokenPurpose::VerifyEmail)
            .await
            .expect("live token survives pruning");
    }
}

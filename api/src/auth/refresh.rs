//! Opaque refresh-token service.
//!
//! Refresh tokens are high-entropy random values (32 bytes, hex-encoded). Only
//! their SHA-256 hex digest is persisted; the plaintext is returned to the
//! caller exactly once (to be set as an httpOnly cookie) and never logged.
//!
//! Every token belongs to a family: the lineage started by one login or
//! registration on one browser. Rotation advances the lineage; reuse detection
//! burns only the compromised lineage (RFC 9700 §4.14.2), never other devices.

use chrono::{Duration, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Set, TransactionTrait, sea_query::Expr,
};

use crate::{
    entities::{
        prelude::{RefreshToken, User},
        refresh_token, user,
    },
    error::AppError,
};

/// A successful rotation carries the new cookie plaintext and the owning user.
/// Loading the user inside the rotation transaction prevents a committed claim
/// from being stranded by a later lookup failure.
#[derive(Debug)]
pub struct RotatedToken {
    pub plaintext: String,
    pub user: user::Model,
}

#[derive(Debug)]
pub enum RotateOutcome {
    Rotated(RotatedToken),
    /// A sibling request already rotated the cookie and its successor is live.
    /// The caller returns 401 but must not clear the browser's newer cookie.
    Superseded,
}

struct InsertedToken {
    plaintext: String,
    id: i32,
}

/// Security rejections are represented as data until after commit, so token or
/// family revocations are not rolled back merely because the public result is 401.
enum RotateDecision {
    Outcome(RotateOutcome),
    Rejected,
}

/// Insert a token under the supplied account generation and integer lineage.
/// `None` starts a root; `Some(root_id)` continues an existing family.
async fn insert_token<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    session_version: i64,
    expiry_days: i64,
    family_id: Option<i32>,
) -> Result<InsertedToken, AppError> {
    let plaintext = super::secret::generate_secret();
    let now = Utc::now();
    let model = refresh_token::ActiveModel {
        user_id: Set(user_id),
        token_hash: Set(super::secret::sha256_hex(&plaintext)),
        expires_at: Set(now + Duration::days(expiry_days)),
        revoked_at: Set(None),
        created_at: Set(now),
        family_id: Set(family_id),
        session_version: Set(session_version),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(InsertedToken {
        plaintext,
        id: model.id,
    })
}

/// Acquire the portable per-account session lock. A no-op update takes a row
/// lock on Postgres and SQLite's writer lock without backend-specific SQL.
/// Callers must keep the surrounding transaction open.
pub async fn lock_user_session_state<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
) -> Result<bool, AppError> {
    let locked = User::update_many()
        .col_expr(
            user::Column::SessionVersion,
            Expr::col(user::Column::SessionVersion).into(),
        )
        .filter(user::Column::Id.eq(user_id))
        .exec(db)
        .await?;
    Ok(locked.rows_affected == 1)
}

/// Issue a fresh family root atomically under the same per-user lock as rotation
/// and password reset. A concurrent generation bump cannot leave a newly-minted
/// stale session behind, and the root insert/family stamp cannot split on failure.
pub async fn issue_refresh_token(
    db: &DatabaseConnection,
    user_id: i32,
    session_version: i64,
    expiry_days: i64,
) -> Result<String, AppError> {
    let txn = db.begin().await?;
    if !lock_user_session_state(&txn, user_id).await? {
        txn.rollback().await?;
        return Err(AppError::Unauthorized("user no longer exists".to_string()));
    }
    let Some(user) = User::find_by_id(user_id).one(&txn).await? else {
        txn.rollback().await?;
        return Err(AppError::Unauthorized("user no longer exists".to_string()));
    };
    if user.session_version != session_version {
        txn.rollback().await?;
        return Err(AppError::Unauthorized("session has expired".to_string()));
    }

    let inserted = insert_token(&txn, user_id, session_version, expiry_days, None).await?;
    let stamped = RefreshToken::update_many()
        .col_expr(refresh_token::Column::FamilyId, Expr::value(inserted.id))
        .filter(refresh_token::Column::Id.eq(inserted.id))
        .exec(&txn)
        .await?;
    if stamped.rows_affected != 1 {
        return Err(AppError::Internal(
            "failed to stamp refresh token family".to_string(),
        ));
    }
    txn.commit().await?;
    Ok(inserted.plaintext)
}

async fn find_by_hash<C: ConnectionTrait>(
    db: &C,
    token_hash: &str,
) -> Result<Option<refresh_token::Model>, AppError> {
    Ok(RefreshToken::find()
        .filter(refresh_token::Column::TokenHash.eq(token_hash))
        .one(db)
        .await?)
}

/// Atomically claim, rotate, link, load the user, and handle reuse. Every
/// operation for an account takes the user-row lock first, preventing a
/// descendant rotation from inserting after a family burn or password reset.
pub async fn rotate(
    db: &DatabaseConnection,
    presented_plaintext: &str,
    expiry_days: i64,
) -> Result<RotateOutcome, AppError> {
    let token_hash = super::secret::sha256_hex(presented_plaintext);

    // This is only an owner hint so the transaction's first statement can be the
    // per-user write lock. The token is re-read after that lock.
    let Some(owner_hint) = find_by_hash(db, &token_hash).await? else {
        return Err(AppError::Unauthorized("invalid refresh token".to_string()));
    };

    let txn = db.begin().await?;
    let decision =
        rotate_in_transaction(&txn, &token_hash, owner_hint.user_id, expiry_days).await?;
    txn.commit().await?;
    match decision {
        RotateDecision::Outcome(outcome) => Ok(outcome),
        RotateDecision::Rejected => {
            Err(AppError::Unauthorized("invalid refresh token".to_string()))
        }
    }
}

async fn rotate_in_transaction<C: ConnectionTrait>(
    db: &C,
    token_hash: &str,
    owner_hint: i32,
    expiry_days: i64,
) -> Result<RotateDecision, AppError> {
    if !lock_user_session_state(db, owner_hint).await? {
        return Ok(RotateDecision::Rejected);
    }

    let Some(row) = find_by_hash(db, token_hash).await? else {
        return Ok(RotateDecision::Rejected);
    };
    if row.user_id != owner_hint {
        return Ok(RotateDecision::Rejected);
    }
    if row.revoked_at.is_some() {
        return classify_revoked_token(db, &row).await;
    }

    let now = Utc::now();
    let claimed = RefreshToken::update_many()
        .col_expr(refresh_token::Column::RevokedAt, Expr::value(now))
        .filter(refresh_token::Column::Id.eq(row.id))
        .filter(refresh_token::Column::RevokedAt.is_null())
        .exec(db)
        .await?;
    if claimed.rows_affected != 1 {
        // Logout does not take the family lock and may win this narrow race.
        let Some(current) = RefreshToken::find_by_id(row.id).one(db).await? else {
            return Ok(RotateDecision::Rejected);
        };
        return classify_revoked_token(db, &current).await;
    }

    // Presenting an expired token still commits its claim/revocation.
    if row.expires_at <= now {
        return Ok(RotateDecision::Rejected);
    }

    let Some(user) = User::find_by_id(owner_hint).one(db).await? else {
        return Ok(RotateDecision::Rejected);
    };
    if user.session_version != row.session_version {
        burn_family(db, &row).await?;
        return Ok(RotateDecision::Rejected);
    }

    // A legacy family-less predecessor adopts its own id for its successors.
    let family_id = row.family_id.or(Some(row.id));
    let successor =
        insert_token(db, owner_hint, row.session_version, expiry_days, family_id).await?;
    let linked = RefreshToken::update_many()
        .col_expr(
            refresh_token::Column::ReplacedById,
            Expr::value(successor.id),
        )
        .filter(refresh_token::Column::Id.eq(row.id))
        .filter(refresh_token::Column::ReplacedById.is_null())
        .exec(db)
        .await?;
    if linked.rows_affected != 1 {
        return Err(AppError::Internal(
            "failed to link rotated refresh token".to_string(),
        ));
    }

    Ok(RotateDecision::Outcome(RotateOutcome::Rotated(
        RotatedToken {
            plaintext: successor.plaintext,
            user,
        },
    )))
}

async fn classify_revoked_token<C: ConnectionTrait>(
    db: &C,
    row: &refresh_token::Model,
) -> Result<RotateDecision, AppError> {
    let Some(successor_id) = row.replaced_by_id else {
        // The per-user lock prevents a concurrent rotation from exposing its
        // claim before its successor link commits. No successor therefore means
        // an explicit logout/reset revocation, not a benign double-submit.
        return Ok(RotateDecision::Rejected);
    };
    let expected_family = row.family_id.or(Some(row.id));
    let Some(successor) = RefreshToken::find_by_id(successor_id).one(db).await? else {
        burn_family(db, row).await?;
        return Ok(RotateDecision::Rejected);
    };
    if successor.user_id != row.user_id
        || successor.family_id != expected_family
        || successor.revoked_at.is_some()
        || successor.expires_at <= Utc::now()
    {
        burn_family(db, row).await?;
        return Ok(RotateDecision::Rejected);
    }
    Ok(RotateDecision::Outcome(RotateOutcome::Superseded))
}

/// Revoke a compromised integer family. Pre-migration `NULL` families fall back
/// to revoking every session for that user, which is over-broad but safe.
async fn burn_family<C: ConnectionTrait>(
    db: &C,
    row: &refresh_token::Model,
) -> Result<(), AppError> {
    match row.family_id {
        Some(family_id) => {
            RefreshToken::update_many()
                .col_expr(refresh_token::Column::RevokedAt, Expr::value(Utc::now()))
                .filter(refresh_token::Column::UserId.eq(row.user_id))
                .filter(refresh_token::Column::FamilyId.eq(family_id))
                .filter(refresh_token::Column::RevokedAt.is_null())
                .exec(db)
                .await?;
            Ok(())
        }
        None => revoke_all_for_user(db, row.user_id).await,
    }
}

/// Revoke the presented refresh token's login family during logout. Taking the
/// same per-user lock as rotation ensures a concurrent successor cannot commit
/// after the logout sweep and resurrect the browser session. Idempotent for an
/// unknown token or a family that is already fully revoked.
pub async fn revoke_one(
    db: &DatabaseConnection,
    presented_plaintext: &str,
) -> Result<(), AppError> {
    let token_hash = super::secret::sha256_hex(presented_plaintext);
    let Some(owner_hint) = find_by_hash(db, &token_hash).await? else {
        return Ok(());
    };

    let txn = db.begin().await?;
    if !lock_user_session_state(&txn, owner_hint.user_id).await? {
        txn.rollback().await?;
        return Ok(());
    }
    if let Some(row) = find_by_hash(&txn, &token_hash).await? {
        burn_family(&txn, &row).await?;
    }
    txn.commit().await?;
    Ok(())
}

/// Revoke all active refresh tokens for an account. Generic over a connection so
/// password reset can keep the generation bump and revocation in one transaction.
pub async fn revoke_all_for_user<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<(), AppError> {
    RefreshToken::update_many()
        .col_expr(refresh_token::Column::RevokedAt, Expr::value(Utc::now()))
        .filter(refresh_token::Column::UserId.eq(user_id))
        .filter(refresh_token::Column::RevokedAt.is_null())
        .exec(db)
        .await?;
    Ok(())
}

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
    use crate::auth::secret::{generate_secret as generate_token, sha256_hex as hash_token};
    use crate::test_support::insert_user;

    async fn setup_db() -> DatabaseConnection {
        crate::test_support::migrated_memory_db().await
    }

    async fn find_row(db: &DatabaseConnection, plaintext: &str) -> refresh_token::Model {
        RefreshToken::find()
            .filter(refresh_token::Column::TokenHash.eq(hash_token(plaintext)))
            .one(db)
            .await
            .expect("query")
            .expect("row exists")
    }

    fn expect_rotated(outcome: RotateOutcome) -> RotatedToken {
        match outcome {
            RotateOutcome::Rotated(token) => token,
            RotateOutcome::Superseded => panic!("expected a fresh rotation"),
        }
    }

    #[test]
    fn generated_tokens_are_distinct_and_hash_is_deterministic() {
        let a = generate_token();
        let b = generate_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert_eq!(hash_token(&a), hash_token(&a));
        assert_ne!(hash_token(&a), hash_token(&b));
    }

    #[tokio::test]
    async fn rotate_revokes_old_and_threads_family_and_generation() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "rotate@example.com").await;
        let original = issue_refresh_token(&db, user_id, 0, 30)
            .await
            .expect("issue");
        let original_hash = hash_token(&original);
        let rotated = expect_rotated(rotate(&db, &original, 30).await.expect("rotate"));

        assert_eq!(rotated.user.id, user_id);
        assert_ne!(hash_token(&rotated.plaintext), original_hash);
        let old_row = find_row(&db, &original).await;
        let new_row = find_row(&db, &rotated.plaintext).await;
        assert!(old_row.revoked_at.is_some());
        assert!(new_row.revoked_at.is_none());
        assert_eq!(old_row.replaced_by_id, Some(new_row.id));
        assert_eq!(old_row.family_id, Some(old_row.id));
        assert_eq!(new_row.family_id, Some(old_row.id));
        assert_eq!(old_row.session_version, new_row.session_version);
    }

    #[tokio::test]
    async fn rotate_unknown_token_is_unauthorized() {
        let db = setup_db().await;
        let err = rotate(&db, "not-a-real-token", 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn rotate_expired_token_is_unauthorized_and_stays_revoked() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "expired@example.com").await;
        let expired = issue_refresh_token(&db, user_id, 0, -1)
            .await
            .expect("issue");
        let err = rotate(&db, &expired, 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
        assert!(find_row(&db, &expired).await.revoked_at.is_some());
    }

    #[tokio::test]
    async fn rotate_rejects_an_older_session_generation() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "generation@example.com").await;
        let token = issue_refresh_token(&db, user_id, 0, 30)
            .await
            .expect("issue");
        let user = User::find_by_id(user_id)
            .one(&db)
            .await
            .expect("query")
            .expect("user");
        let mut active: user::ActiveModel = user.into();
        active.session_version = Set(1);
        active.update(&db).await.expect("advance generation");

        let err = rotate(&db, &token, 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
        assert!(find_row(&db, &token).await.revoked_at.is_some());
    }

    #[tokio::test]
    async fn replay_burns_only_its_family_and_not_fresh_sessions() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "reuse@example.com").await;
        let t1 = issue_refresh_token(&db, user_id, 0, 30).await.expect("t1");
        let other_device = issue_refresh_token(&db, user_id, 0, 30)
            .await
            .expect("other device");
        let t2 = expect_rotated(rotate(&db, &t1, 30).await.expect("t1 -> t2"));
        let t3 = expect_rotated(rotate(&db, &t2.plaintext, 30).await.expect("t2 -> t3"));

        let err = rotate(&db, &t1, 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
        assert!(find_row(&db, &t3.plaintext).await.revoked_at.is_some());
        assert!(find_row(&db, &other_device).await.revoked_at.is_none());

        let fresh = issue_refresh_token(&db, user_id, 0, 30)
            .await
            .expect("fresh family");
        assert!(matches!(
            rotate(&db, &t1, 30).await,
            Err(AppError::Unauthorized(_))
        ));
        assert!(find_row(&db, &fresh).await.revoked_at.is_none());
    }

    #[tokio::test]
    async fn replay_of_legacy_family_less_token_burns_all_sessions() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "legacy@example.com").await;
        let t1 = issue_refresh_token(&db, user_id, 0, 30).await.expect("t1");
        let other_device = issue_refresh_token(&db, user_id, 0, 30)
            .await
            .expect("other device");
        let t2 = expect_rotated(rotate(&db, &t1, 30).await.expect("t1 -> t2"));
        expect_rotated(rotate(&db, &t2.plaintext, 30).await.expect("t2 -> t3"));

        RefreshToken::update_many()
            .col_expr(refresh_token::Column::FamilyId, Expr::value(None::<i32>))
            .filter(refresh_token::Column::UserId.eq(user_id))
            .exec(&db)
            .await
            .expect("strip family ids");
        let err = rotate(&db, &t1, 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
        assert!(find_row(&db, &other_device).await.revoked_at.is_some());
    }

    #[tokio::test]
    async fn concurrent_double_submit_does_not_revoke_family() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "concurrent@example.com").await;
        let token = issue_refresh_token(&db, user_id, 0, 30)
            .await
            .expect("issue");
        let (first, second) = tokio::join!(rotate(&db, &token, 30), rotate(&db, &token, 30));
        let successor = match (first.expect("first"), second.expect("second")) {
            (RotateOutcome::Rotated(token), RotateOutcome::Superseded)
            | (RotateOutcome::Superseded, RotateOutcome::Rotated(token)) => token,
            _ => panic!("exactly one request must rotate"),
        };
        assert!(
            find_row(&db, &successor.plaintext)
                .await
                .revoked_at
                .is_none()
        );
        expect_rotated(
            rotate(&db, &successor.plaintext, 30)
                .await
                .expect("successor remains rotatable"),
        );
    }

    #[tokio::test]
    async fn revoked_token_without_successor_is_a_definitive_rejection() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "loggedout@example.com").await;
        let token = issue_refresh_token(&db, user_id, 0, 30)
            .await
            .expect("issue");
        revoke_one(&db, &token).await.expect("revoke");
        assert!(matches!(
            rotate(&db, &token, 30).await,
            Err(AppError::Unauthorized(_))
        ));
        let live = RefreshToken::find()
            .filter(refresh_token::Column::UserId.eq(user_id))
            .filter(refresh_token::Column::RevokedAt.is_null())
            .one(&db)
            .await
            .expect("query");
        assert!(live.is_none());
    }

    #[tokio::test]
    async fn an_expired_successor_is_not_a_benign_double_submit() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "expired-successor@example.com").await;
        let original = issue_refresh_token(&db, user_id, 0, 30)
            .await
            .expect("issue");
        let successor = expect_rotated(rotate(&db, &original, -1).await.expect("rotate"));

        assert!(matches!(
            rotate(&db, &original, 30).await,
            Err(AppError::Unauthorized(_))
        ));
        assert!(
            find_row(&db, &successor.plaintext)
                .await
                .revoked_at
                .is_some()
        );
    }

    #[tokio::test]
    async fn prune_expired_removes_only_expired_tokens() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "prune@example.com").await;
        let live = issue_refresh_token(&db, user_id, 0, 30)
            .await
            .expect("live");
        let expired = issue_refresh_token(&db, user_id, 0, -1)
            .await
            .expect("expired");
        assert_eq!(prune_expired(&db).await.expect("prune"), 1);
        assert!(
            RefreshToken::find()
                .filter(refresh_token::Column::TokenHash.eq(hash_token(&live)))
                .one(&db)
                .await
                .expect("query")
                .is_some()
        );
        assert!(
            RefreshToken::find()
                .filter(refresh_token::Column::TokenHash.eq(hash_token(&expired)))
                .one(&db)
                .await
                .expect("query")
                .is_none()
        );
    }

    #[tokio::test]
    async fn revoke_one_is_idempotent() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "logout@example.com").await;
        let token = issue_refresh_token(&db, user_id, 0, 30)
            .await
            .expect("issue");
        revoke_one(&db, &token).await.expect("first");
        revoke_one(&db, &token).await.expect("second");
        revoke_one(&db, "unknown").await.expect("unknown");
        assert!(find_row(&db, &token).await.revoked_at.is_some());
    }

    #[tokio::test]
    async fn logout_with_a_stale_ancestor_revokes_its_successor_family_only() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "logout-race@example.com").await;
        let ancestor = issue_refresh_token(&db, user_id, 0, 30)
            .await
            .expect("ancestor");
        let other_device = issue_refresh_token(&db, user_id, 0, 30)
            .await
            .expect("other device");
        let successor = expect_rotated(rotate(&db, &ancestor, 30).await.expect("rotate"));

        revoke_one(&db, &ancestor).await.expect("logout family");

        assert!(
            find_row(&db, &successor.plaintext)
                .await
                .revoked_at
                .is_some()
        );
        assert!(find_row(&db, &other_device).await.revoked_at.is_none());
    }
}

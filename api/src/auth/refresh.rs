//! Opaque refresh-token service.
//!
//! Refresh tokens are high-entropy random values (32 bytes, hex-encoded). Only
//! their SHA-256 hex digest is persisted; the plaintext is returned to the
//! caller exactly once (to be set as an httpOnly cookie) and never logged.
//!
//! Because the token is already uniformly random, a fast cryptographic hash
//! (SHA-256) is the correct choice here — argon2 is for low-entropy passwords.
//!
//! Every token belongs to a *family*: the lineage started by one login (or
//! registration) on one browser, threaded by `family_id` (the root's row id).
//! Rotation advances the lineage; reuse detection burns only the compromised
//! lineage (RFC 9700 §4.14.2), never the user's other devices.

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

/// Outcome of a successful rotation: the new plaintext token (for the cookie)
/// and the owning user (to mint a fresh access token — loaded inside the
/// rotation transaction so a committed rotation can never be stranded by a
/// follow-up lookup failing).
#[derive(Debug)]
pub struct RotatedToken {
    pub plaintext: String,
    pub user: user::Model,
}

/// The result of presenting a refresh token to [`rotate`].
///
/// A `Superseded` result is deliberately NOT an error: it is the *benign*
/// concurrent double-submit — the presented token was already rotated by a
/// sibling request (another tab, a browser session-restore, or a
/// `refetchOnReconnect` that fired in every open tab at once) whose successor is
/// still live. The browser therefore already holds a newer, valid refresh
/// cookie, so the caller must leave that cookie untouched. Clearing it here would
/// race the winning request's rotated `Set-Cookie` and, when the clear lands
/// last, wipe the live cookie — logging every tab out (the intermittent
/// "logged out for no reason" bug). Genuine dead-session cases (unknown / expired
/// token, or detected reuse that burned the family) stay as `Err(Unauthorized)`,
/// for which the caller DOES clear the cookie.
#[derive(Debug)]
pub enum RotateOutcome {
    /// The presented token was valid and single-use-claimed: it has been revoked
    /// and replaced. Carries the replacement (for the cookie) and owning user.
    Rotated(RotatedToken),
    /// A benign concurrent double-submit — see the enum docs. The caller returns
    /// 401 but must NOT clear the client's refresh cookie.
    Superseded,
}

/// A freshly-inserted refresh token: the plaintext (for the cookie) and the new
/// row's id (so a rotated predecessor can record it as its successor).
struct InsertedToken {
    plaintext: String,
    id: i32,
}

/// Insert a brand-new refresh token for `user_id`, persisting only its hash.
/// `family_id` threads the lineage: `None` starts a new family (the caller
/// stamps the root afterwards), `Some` continues a rotation chain.
async fn insert_token<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
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
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(InsertedToken {
        plaintext,
        id: model.id,
    })
}

/// Issue a brand-new refresh token for `user_id` (a fresh family root),
/// persisting only its hash. Returns the PLAINTEXT token (the only time it
/// ever leaves this module).
pub async fn issue_refresh_token(
    db: &DatabaseConnection,
    user_id: i32,
    expiry_days: i64,
) -> Result<String, AppError> {
    let inserted = insert_token(db, user_id, expiry_days, None).await?;

    // Stamp the root with its own id: every successor copies it, so a reuse
    // burn can target exactly this lineage. A crash between the insert and the
    // stamp leaves `family_id` NULL, which the burn treats with the (safe,
    // over-broad) revoke-everything fallback — same as pre-migration rows.
    RefreshToken::update_many()
        .col_expr(refresh_token::Column::FamilyId, Expr::value(inserted.id))
        .filter(refresh_token::Column::Id.eq(inserted.id))
        .exec(db)
        .await?;

    Ok(inserted.plaintext)
}

/// Load a token row by the presented plaintext's hash.
async fn find_by_hash<C: ConnectionTrait>(
    db: &C,
    token_hash: &str,
) -> Result<Option<refresh_token::Model>, AppError> {
    Ok(RefreshToken::find()
        .filter(refresh_token::Column::TokenHash.eq(token_hash))
        .one(db)
        .await?)
}

/// Rotate the presented refresh token.
///
/// * not found                 -> `Err(Unauthorized)` (dead session; caller clears the cookie)
/// * found, revoked, reuse      -> REUSE/theft: revoke the token's FAMILY, then `Err(Unauthorized)`
/// * found, revoked, benign     -> `Ok(Superseded)` — a sibling already rotated it; caller KEEPS the cookie
/// * found but expired          -> `Err(Unauthorized)`
/// * valid                      -> mark revoked, issue a replacement, `Ok(Rotated(..))`
///
/// The winning path runs in ONE transaction (claim, successor insert, lineage
/// link, user load), so a dropped request (axum cancels the handler future when
/// the client disconnects) or a DB error mid-rotation rolls the claim back and
/// the presented token stays live and retriable. It used to commit the claim
/// alone, permanently stranding the browser on a revoked cookie (issue #417).
pub async fn rotate(
    db: &DatabaseConnection,
    presented_plaintext: &str,
    expiry_days: i64,
) -> Result<RotateOutcome, AppError> {
    let token_hash = super::secret::sha256_hex(presented_plaintext);
    let now = Utc::now();

    let txn = db.begin().await?;

    // Atomically claim the token by flipping revoked_at NULL -> now in a single
    // conditional UPDATE (the transaction's first statement, so SQLite takes its
    // write lock immediately). Only one concurrent caller can match the
    // `RevokedAt IS NULL` predicate — on Postgres a concurrent claimer blocks on
    // the row lock until this transaction commits, then re-evaluates the
    // predicate and matches 0 rows — so exactly one rotation wins per presented
    // token. This single-use invariant is what makes reuse detection sound.
    let claimed = RefreshToken::update_many()
        .col_expr(refresh_token::Column::RevokedAt, Expr::value(now))
        .filter(refresh_token::Column::TokenHash.eq(&token_hash))
        .filter(refresh_token::Column::RevokedAt.is_null())
        .exec(&txn)
        .await?;

    if claimed.rows_affected == 0 {
        // Nothing was written; release the transaction before diagnosing, so the
        // reuse burn below commits independently of the `Err` we may return.
        txn.rollback().await?;
        return diagnose_unclaimed(db, &token_hash).await;
    }

    // We hold the claim; load the (now-revoked) row for its owner + expiry.
    let row = find_by_hash(&txn, &token_hash)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid refresh token".to_string()))?;

    // The token was unrevoked when we claimed it, but it may have been past its
    // expiry. COMMIT the claim (an expired token stays revoked on presentation,
    // as before the transaction existed) and reject without replacing it.
    if row.expires_at <= now {
        txn.commit().await?;
        return Err(AppError::Unauthorized("invalid refresh token".to_string()));
    }

    let user_id = row.user_id;
    // Successors inherit the family; a pre-migration predecessor (NULL family)
    // adopts its own id as the family key, so the lineage is trackable from the
    // first post-migration rotation onward.
    let family_id = row.family_id.or(Some(row.id));
    let successor = insert_token(&txn, user_id, expiry_days, family_id).await?;

    // Record the successor so a later replay of this token can be told apart from
    // a benign concurrent double-submit (whose successor is still active).
    let mut rotated: refresh_token::ActiveModel = row.into();
    rotated.replaced_by_id = Set(Some(successor.id));
    rotated.update(&txn).await?;

    // Load the owner INSIDE the transaction: if the lookup fails, the whole
    // rotation rolls back instead of committing a claim whose access token can
    // never be minted (which would strand the browser on a dead cookie).
    let user = User::find_by_id(user_id)
        .one(&txn)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user no longer exists".to_string()))?;

    txn.commit().await?;

    Ok(RotateOutcome::Rotated(RotatedToken {
        plaintext: successor.plaintext,
        user,
    }))
}

/// Work out why the conditional claim matched nothing and answer accordingly.
/// Runs OUTSIDE any transaction: the reuse burn must commit even though the
/// function then returns `Err`.
async fn diagnose_unclaimed(
    db: &DatabaseConnection,
    token_hash: &str,
) -> Result<RotateOutcome, AppError> {
    // The token is either unknown or already revoked. Re-read to distinguish.
    let Some(row) = find_by_hash(db, token_hash).await? else {
        // No such token at all: a genuinely invalid/unknown cookie.
        return Err(AppError::Unauthorized("invalid refresh token".to_string()));
    };

    // A rotated token records its successor. If that successor has ITSELF been
    // revoked (used/rotated/logged out), replaying this long-superseded token is
    // genuine reuse — burn the whole family and treat it as a dead session (the
    // caller clears the cookie). A revoked token is NEVER exchanged for a new one.
    if let Some(successor_id) = row.replaced_by_id {
        let successor_revoked = RefreshToken::find_by_id(successor_id)
            .one(db)
            .await?
            .is_some_and(|s| s.revoked_at.is_some());
        if successor_revoked {
            burn_family(db, &row).await?;
            return Err(AppError::Unauthorized("invalid refresh token".to_string()));
        }
    }

    // The row exists and we detected no theft. Report `Superseded` so the caller
    // leaves the client's refresh cookie ALONE. This covers:
    //   * a live successor  -> a sibling tab/request already rotated this token;
    //     the browser holds that newer valid cookie, and clearing it here would
    //     race the winner's Set-Cookie and log every tab out.
    //   * no successor yet (`replaced_by_id` is None) -> either the winning
    //     rotation has claimed the token but not yet committed its successor
    //     link (a live cookie is imminent — clearing would clobber it), OR the
    //     token was logged out / revoked by a password reset. In the latter case
    //     the cookie is already gone (logout clears it) or inert (a revoked token
    //     can never be exchanged — the reset "ends every session" invariant still
    //     holds via the 401), so NOT clearing is harmless.
    Ok(RotateOutcome::Superseded)
}

/// Revoke every still-active token in the replayed token's family. Pre-migration
/// rows have no family id; for those, fall back to revoking all of the user's
/// tokens (over-broad but safe — such rows age out with the token expiry).
async fn burn_family(db: &DatabaseConnection, row: &refresh_token::Model) -> Result<(), AppError> {
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

/// Revoke the single refresh token matching `presented_plaintext` (logout).
/// Idempotent: a missing / already-revoked / unknown token is a no-op success.
pub async fn revoke_one(
    db: &DatabaseConnection,
    presented_plaintext: &str,
) -> Result<(), AppError> {
    let token_hash = super::secret::sha256_hex(presented_plaintext);

    let row = find_by_hash(db, &token_hash).await?;

    if let Some(row) = row
        && row.revoked_at.is_none()
    {
        let mut active: refresh_token::ActiveModel = row.into();
        active.revoked_at = Set(Some(Utc::now()));
        active.update(db).await?;
    }

    Ok(())
}

/// Revoke every still-active refresh token belonging to `user_id`, across all
/// families/devices. Used by password reset (a changed password must end every
/// existing session) and as the burn fallback for pre-migration rows.
pub async fn revoke_all_for_user(db: &DatabaseConnection, user_id: i32) -> Result<(), AppError> {
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

    /// Unwrap a rotation that was expected to mint a fresh token.
    fn expect_rotated(outcome: RotateOutcome) -> RotatedToken {
        match outcome {
            RotateOutcome::Rotated(token) => token,
            RotateOutcome::Superseded => panic!("expected a fresh rotation, got Superseded"),
        }
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

        let rotated = expect_rotated(rotate(&db, &original, 30).await.expect("rotate"));

        assert_eq!(rotated.user.id, user_id);
        // A rotated (revoked) token's replacement has a different hash.
        assert_ne!(rotated.plaintext, original);
        assert_ne!(hash_token(&rotated.plaintext), original_hash);

        // Old row is now revoked, new row is active.
        let old_row = find_row(&db, &original).await;
        let new_row = find_row(&db, &rotated.plaintext).await;
        assert!(old_row.revoked_at.is_some());
        assert!(new_row.revoked_at.is_none());
        // The old row records its successor for reuse detection.
        assert_eq!(old_row.replaced_by_id, Some(new_row.id));
        // The lineage is threaded: the root stamped itself, the successor inherits.
        assert_eq!(old_row.family_id, Some(old_row.id));
        assert_eq!(new_row.family_id, Some(old_row.id));
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
        // Negative expiry => already past.
        let expired = issue_refresh_token(&db, user_id, -1).await.expect("issue");
        let err = rotate(&db, &expired, 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
        // The claim of an expired token commits (it is not rolled back with the
        // rejected rotation): presenting a token spends it, expired or not.
        assert!(find_row(&db, &expired).await.revoked_at.is_some());
    }

    #[tokio::test]
    async fn replay_of_consumed_token_burns_only_its_family() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "reuse@example.com").await;

        let t1 = issue_refresh_token(&db, user_id, 30).await.expect("t1");
        let other_device = issue_refresh_token(&db, user_id, 30)
            .await
            .expect("t2 (other device)");

        // Rotate t1 -> t3, then t3 -> t4, so t1's successor (t3) is itself revoked.
        let r3 = expect_rotated(rotate(&db, &t1, 30).await.expect("rotate t1 -> t3"));
        let r4 = expect_rotated(
            rotate(&db, &r3.plaintext, 30)
                .await
                .expect("rotate t3 -> t4"),
        );

        // Replaying the long-superseded t1 is now unambiguous reuse/theft: its
        // successor t3 has itself been revoked, so this is a hard error (not a
        // benign Superseded), and the replayed LINEAGE is burned.
        let err = rotate(&db, &t1, 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));

        // The lineage head t4 is revoked...
        assert!(find_row(&db, &r4.plaintext).await.revoked_at.is_some());
        // ...but the user's OTHER device (an independent family) survives: a
        // browser replaying a stale jar must not log the user's phone out too.
        assert!(find_row(&db, &other_device).await.revoked_at.is_none());
        expect_rotated(
            rotate(&db, &other_device, 30)
                .await
                .expect("other device still rotatable"),
        );
    }

    #[tokio::test]
    async fn replay_of_legacy_family_less_token_burns_all_sessions() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "legacy@example.com").await;

        let t1 = issue_refresh_token(&db, user_id, 30).await.expect("t1");
        let other_device = issue_refresh_token(&db, user_id, 30)
            .await
            .expect("other device");

        let r3 = expect_rotated(rotate(&db, &t1, 30).await.expect("t1 -> t3"));
        expect_rotated(rotate(&db, &r3.plaintext, 30).await.expect("t3 -> t4"));

        // Simulate a pre-migration lineage: strip the family ids.
        RefreshToken::update_many()
            .col_expr(refresh_token::Column::FamilyId, Expr::value(None::<i32>))
            .filter(refresh_token::Column::UserId.eq(user_id))
            .exec(&db)
            .await
            .expect("strip family ids");

        let err = rotate(&db, &t1, 30).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));

        // Without a family id the burn cannot be scoped, so it falls back to
        // revoking everything the user has (the safe pre-migration behavior).
        assert!(find_row(&db, &other_device).await.revoked_at.is_some());
    }

    #[tokio::test]
    async fn concurrent_double_submit_does_not_revoke_family() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "concurrent@example.com").await;

        let t1 = issue_refresh_token(&db, user_id, 30).await.expect("t1");

        // First rotation wins and issues the successor t2.
        let r2 = expect_rotated(rotate(&db, &t1, 30).await.expect("rotate t1 -> t2"));

        // A near-simultaneous second request carried the same just-rotated t1. Its
        // successor t2 is still active, so this is a benign double-submit: reported
        // as `Superseded` (NOT an error), the family is NOT burned, and — crucially
        // — the refresh handler leaves the browser's live cookie (t2) untouched
        // instead of clearing it. Clearing here is what logged multi-tab users out.
        let outcome = rotate(&db, &t1, 30)
            .await
            .expect("benign superseded, not an error");
        assert!(matches!(outcome, RotateOutcome::Superseded));

        // t2 remains usable — the session survives the concurrent refresh.
        assert!(find_row(&db, &r2.plaintext).await.revoked_at.is_none());
        expect_rotated(
            rotate(&db, &r2.plaintext, 30)
                .await
                .expect("t2 still rotatable"),
        );
    }

    #[tokio::test]
    async fn revoked_token_without_a_successor_is_superseded_not_reuse() {
        // A logged-out (or password-reset-revoked) token has `revoked_at` set but
        // no `replaced_by_id`. Replaying it must NOT be exchanged for a new token,
        // and it is NOT theft (no successor was ever revoked), so it reports
        // `Superseded` rather than an error — letting the refresh handler leave the
        // client's cookie alone instead of racing a sibling's live cookie. The
        // session stays effectively dead: the token is never rotated (the handler
        // still answers 401), it just does not clobber a cookie.
        let db = setup_db().await;
        let user_id = insert_user(&db, "loggedout@example.com").await;
        let token = issue_refresh_token(&db, user_id, 30).await.expect("issue");

        revoke_one(&db, &token)
            .await
            .expect("logout revokes the token");

        let outcome = rotate(&db, &token, 30)
            .await
            .expect("a revoked-but-not-reused token is Superseded, not an error");
        assert!(matches!(outcome, RotateOutcome::Superseded));
        // Nothing new was minted for this user.
        let live = RefreshToken::find()
            .filter(refresh_token::Column::UserId.eq(user_id))
            .filter(refresh_token::Column::RevokedAt.is_null())
            .one(&db)
            .await
            .expect("query");
        assert!(live.is_none(), "logout must stay logged out");
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

        assert!(find_row(&db, &token).await.revoked_at.is_some());
    }
}

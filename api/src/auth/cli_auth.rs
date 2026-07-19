//! Short-lived, single-use authorization codes backing the CLI's browser
//! (loopback) sign-in (see [`crate::handlers::cli_auth`]).
//!
//! Mirrors the email-token store ([`super::email_token`]): the code is a
//! high-entropy random value (32 bytes, hex), only its SHA-256 hex digest is
//! persisted, and it is spent exactly once via an atomic conditional `UPDATE` on
//! `consumed_at`. A code is bound to a **PKCE** challenge — the SHA-256 hex of a
//! verifier the CLI keeps private — so intercepting the code as it rides the
//! loopback redirect URL is useless to anyone without the verifier (RFC 8252
//! §8.1). It also records the account's session generation, so a password reset
//! landing inside the (~5 min) window invalidates any outstanding code.

use chrono::{Duration, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
    prelude::ActiveModelTrait, sea_query::Expr,
};

use crate::{
    entities::{cli_auth_code, prelude::CliAuthCode},
    error::AppError,
};

/// Lifetime of a CLI authorization code. Tight: the browser hands it to the
/// waiting loopback listener within seconds, so a few minutes is ample slack
/// while keeping the interception window small.
pub const CODE_TTL: Duration = Duration::minutes(5);

/// A consumed code's owner plus the session generation it was minted under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CliAuthClaim {
    pub user_id: i32,
    pub session_version: i64,
}

/// Issue a fresh authorization code for `user_id`, bound to `code_challenge` (the
/// SHA-256 hex of the CLI's verifier) and the account's current `session_version`.
/// Persists only the code's SHA-256 hash; returns the PLAINTEXT code — the only
/// time it leaves this module (the SPA relays it to the CLI via the loopback
/// redirect).
pub async fn issue_code(
    db: &DatabaseConnection,
    user_id: i32,
    session_version: i64,
    code_challenge: &str,
    client_name: Option<&str>,
) -> Result<String, AppError> {
    let plaintext = super::secret::generate_secret();
    let now = Utc::now();
    cli_auth_code::ActiveModel {
        user_id: Set(user_id),
        code_hash: Set(super::secret::sha256_hex(&plaintext)),
        code_challenge: Set(code_challenge.to_string()),
        session_version: Set(session_version),
        client_name: Set(client_name.map(str::to_string)),
        expires_at: Set(now + CODE_TTL),
        consumed_at: Set(None),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(plaintext)
}

/// Spend the presented code for `code_verifier`: verify it is live and that the
/// verifier matches the stored PKCE challenge, then claim it single-use.
///
/// The verifier is checked BEFORE the code is consumed, so a wrong verifier never
/// burns a legitimate code (only the holder of the verifier can spend it); the
/// atomic conditional `UPDATE` then makes the spend single-use, so two concurrent
/// valid redemptions can't both win. Any failure — unknown / consumed / expired
/// code, or a challenge mismatch — is the same generic `Unauthorized`, so the
/// endpoint is no oracle for which held.
pub async fn consume_code<C>(
    db: &C,
    presented_code: &str,
    code_verifier: &str,
) -> Result<CliAuthClaim, AppError>
where
    C: ConnectionTrait,
{
    let code_hash = super::secret::sha256_hex(presented_code);
    let now = Utc::now();
    let invalid = || AppError::Unauthorized("invalid or expired code".to_string());

    // Look the code up first so a wrong verifier can't consume a valid code.
    let row = CliAuthCode::find()
        .filter(cli_auth_code::Column::CodeHash.eq(&code_hash))
        .one(db)
        .await?
        .ok_or_else(invalid)?;

    if row.consumed_at.is_some() || row.expires_at <= now {
        return Err(invalid());
    }

    // PKCE: only the party holding the verifier whose hash was registered at
    // authorize time may redeem the code. The verifier is high-entropy, so a plain
    // digest comparison (as the token stores use for their hashes) is sufficient.
    if super::secret::sha256_hex(code_verifier) != row.code_challenge {
        return Err(invalid());
    }

    // Atomically claim the code by flipping consumed_at NULL -> now (the same
    // single-use idiom as email-token consumption); only one concurrent redemption
    // can match the `ConsumedAt IS NULL` predicate.
    let claimed = CliAuthCode::update_many()
        .col_expr(cli_auth_code::Column::ConsumedAt, Expr::value(now))
        .filter(cli_auth_code::Column::Id.eq(row.id))
        .filter(cli_auth_code::Column::ConsumedAt.is_null())
        .exec(db)
        .await?;
    if claimed.rows_affected != 1 {
        return Err(invalid());
    }

    Ok(CliAuthClaim {
        user_id: row.user_id,
        session_version: row.session_version,
    })
}

/// Delete codes past their expiry. Expired codes are rejected on use regardless,
/// so removing them only bounds table growth. Returns the number of rows pruned.
pub async fn prune_expired(db: &DatabaseConnection) -> Result<u64, AppError> {
    let result = CliAuthCode::delete_many()
        .filter(cli_auth_code::Column::ExpiresAt.lte(Utc::now()))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::secret::sha256_hex;
    use crate::test_support::insert_user;

    async fn setup_db() -> DatabaseConnection {
        crate::test_support::migrated_memory_db().await
    }

    /// A verifier plus the challenge (its SHA-256 hex) the CLI would send.
    fn pkce() -> (String, String) {
        let verifier = super::super::secret::generate_secret();
        let challenge = sha256_hex(&verifier);
        (verifier, challenge)
    }

    #[tokio::test]
    async fn issue_then_consume_returns_owner_and_generation_and_spends_it() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "cli@example.com").await;
        let (verifier, challenge) = pkce();

        let code = issue_code(&db, user_id, 0, &challenge, Some("laptop"))
            .await
            .expect("issue");
        assert_eq!(code.len(), 64); // 32 bytes -> 64 hex chars

        let claim = consume_code(&db, &code, &verifier).await.expect("consume");
        assert_eq!(claim.user_id, user_id);
        assert_eq!(claim.session_version, 0);

        // Single-use: a second redemption of the same code is rejected.
        let err = consume_code(&db, &code, &verifier).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn a_wrong_verifier_is_rejected_without_burning_the_code() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "pkce@example.com").await;
        let (verifier, challenge) = pkce();
        let code = issue_code(&db, user_id, 0, &challenge, None)
            .await
            .expect("issue");

        // A wrong verifier fails...
        let err = consume_code(&db, &code, "not-the-verifier")
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));

        // ...and does NOT consume the code — the real verifier still redeems it.
        let claim = consume_code(&db, &code, &verifier).await.expect("consume");
        assert_eq!(claim.user_id, user_id);
    }

    #[tokio::test]
    async fn unknown_and_expired_codes_are_rejected() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "expired@example.com").await;
        let (verifier, challenge) = pkce();

        let err = consume_code(&db, "not-a-real-code", &verifier)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));

        // Plant an already-expired code directly, then confirm it's refused.
        let plaintext = super::super::secret::generate_secret();
        let now = Utc::now();
        cli_auth_code::ActiveModel {
            user_id: Set(user_id),
            code_hash: Set(sha256_hex(&plaintext)),
            code_challenge: Set(challenge),
            session_version: Set(0),
            client_name: Set(None),
            expires_at: Set(now - Duration::minutes(1)),
            consumed_at: Set(None),
            created_at: Set(now - Duration::minutes(6)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("plant expired code");

        let err = consume_code(&db, &plaintext, &verifier).await.unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
    }

    #[tokio::test]
    async fn concurrent_redemptions_spend_the_code_exactly_once() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "race@example.com").await;
        let (verifier, challenge) = pkce();
        let code = issue_code(&db, user_id, 0, &challenge, None)
            .await
            .expect("issue");

        let (a, b) = tokio::join!(
            consume_code(&db, &code, &verifier),
            consume_code(&db, &code, &verifier),
        );
        let wins = [a, b].into_iter().filter(Result::is_ok).count();
        assert_eq!(wins, 1, "exactly one concurrent redemption may win");
    }

    #[tokio::test]
    async fn prune_expired_removes_only_expired_codes() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "prune@example.com").await;
        let (_, challenge) = pkce();

        let live = issue_code(&db, user_id, 0, &challenge, None)
            .await
            .expect("live");
        let expired_plain = super::super::secret::generate_secret();
        let now = Utc::now();
        cli_auth_code::ActiveModel {
            user_id: Set(user_id),
            code_hash: Set(sha256_hex(&expired_plain)),
            code_challenge: Set(challenge),
            session_version: Set(0),
            client_name: Set(None),
            expires_at: Set(now - Duration::minutes(1)),
            consumed_at: Set(None),
            created_at: Set(now - Duration::minutes(6)),
            ..Default::default()
        }
        .insert(&db)
        .await
        .expect("plant expired");

        assert_eq!(prune_expired(&db).await.expect("prune"), 1);
        // The live code survives and is still in the table (unconsumed).
        let remaining = CliAuthCode::find()
            .filter(cli_auth_code::Column::CodeHash.eq(sha256_hex(&live)))
            .one(&db)
            .await
            .expect("query");
        assert!(remaining.is_some());
    }
}

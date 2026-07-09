//! API-key service backing the public programmatic API.
//!
//! An API key is a user-generated, long-lived opaque credential (the deliberate
//! counterpart to the short-lived access JWT): a signed-in user mints one from the
//! management UI and presents it as `Authorization: Bearer tcgl_<hex>` to reach
//! their own collection / wish-list endpoints from a script.
//!
//! Storage mirrors the refresh/email-token design (see [`super::refresh`]): the
//! key is 32 CSPRNG bytes hex-encoded behind a `tcgl_` label, only its SHA-256 hex
//! digest is persisted, and the plaintext is returned to the caller exactly once
//! (at creation) and never logged. Because the key is already uniformly random a
//! fast cryptographic hash (SHA-256) is the correct choice — argon2 is only for
//! low-entropy passwords — and it lets the auth path resolve a presented key with a
//! single indexed lookup on `token_hash`.
//!
//! Unlike email tokens a key is multi-use and not consumed on use, so resolution is
//! a plain `SELECT`; revocation is a soft `revoked_at` stamp (an audit trail, and
//! an in-flight request sees the revocation), and an optional `expires_at` bounds a
//! key's lifetime.

use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set, prelude::DateTimeUtc,
};
use sha2::{Digest, Sha256};

use crate::{
    entities::{api_key, prelude::ApiKey, prelude::User, user},
    error::AppError,
};

/// The human-visible label prefixing every key's plaintext (`tcgl_<hex>`). Also
/// what the extractor branches on to tell an API key from a JWT bearer token.
pub const KEY_PLAINTEXT_PREFIX: &str = "tcgl_";

/// How many hex chars of the key follow the `tcgl_` label in the stored display
/// prefix (`tcgl_` + 8 hex = 13 chars). 32 bits is enough to tell keys apart in the
/// UI, far too few to narrow the 256-bit secret.
const PREFIX_DISPLAY_HEX_LEN: usize = 8;

/// Only re-stamp `last_used_at` when the last recorded use is at least this old, so
/// a busy key doesn't incur a DB write on every single authenticated request.
const LAST_USED_THROTTLE_SECONDS: i64 = 60;

/// What an API key is allowed to do. Stored as a string discriminator and checked
/// at the authorization seam so a read-only key can't drive a mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKeyScope {
    /// GET-only: read the owner's collection / wish list.
    Read,
    /// Read plus mutate (upsert holdings, import/sync, save/forget a source).
    ReadWrite,
}

impl ApiKeyScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::ReadWrite => "read_write",
        }
    }

    /// Parse the stored/requested discriminator. Unknown values decay to the
    /// least-privileged `Read` (defence in depth — a garbled scope never grants
    /// write), so callers that must reject an unknown request value validate before
    /// calling here.
    pub fn from_str_lenient(value: &str) -> Self {
        match value {
            "read_write" => Self::ReadWrite,
            _ => Self::Read,
        }
    }

    /// Strict parse for a *request* value: an unrecognised scope is a 422 rather
    /// than a silent downgrade.
    pub fn parse_request(value: &str) -> Result<Self, AppError> {
        match value {
            "read" => Ok(Self::Read),
            "read_write" => Ok(Self::ReadWrite),
            other => Err(AppError::Validation(format!(
                "unknown scope '{other}' (expected 'read' or 'read_write')"
            ))),
        }
    }

    pub fn permits_write(self) -> bool {
        matches!(self, Self::ReadWrite)
    }
}

/// Generate a new opaque API key: the `tcgl_` label + 32 CSPRNG bytes, hex-encoded.
fn generate_key() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    format!("{KEY_PLAINTEXT_PREFIX}{}", hex::encode(bytes))
}

/// SHA-256 hex digest of the opaque key (what we store / look up by).
fn hash_key(plaintext: &str) -> String {
    hex::encode(Sha256::digest(plaintext.as_bytes()))
}

/// The stored display prefix: `tcgl_` + the first [`PREFIX_DISPLAY_HEX_LEN`] hex
/// chars, so the UI can identify a key after the secret is gone.
fn display_prefix(plaintext: &str) -> String {
    plaintext
        .chars()
        .take(KEY_PLAINTEXT_PREFIX.len() + PREFIX_DISPLAY_HEX_LEN)
        .collect()
}

/// A freshly-minted key: the plaintext (shown to the user exactly once) and the
/// persisted row (for the creation response metadata).
#[derive(Debug)]
pub struct IssuedApiKey {
    pub plaintext: String,
    pub model: api_key::Model,
}

/// The result of authenticating a presented key: the owning user and the key's scope.
#[derive(Debug)]
pub struct ResolvedApiKey {
    pub user: user::Model,
    pub scope: ApiKeyScope,
}

/// Mint a new key for `user_id`, persisting only its hash. Returns the plaintext
/// exactly once (the only time it ever leaves this module).
pub async fn issue_api_key(
    db: &DatabaseConnection,
    user_id: i32,
    name: &str,
    scope: ApiKeyScope,
    expires_at: Option<DateTimeUtc>,
) -> Result<IssuedApiKey, AppError> {
    let plaintext = generate_key();
    let now = Utc::now();

    let model = api_key::ActiveModel {
        user_id: Set(user_id),
        token_hash: Set(hash_key(&plaintext)),
        name: Set(name.to_string()),
        key_prefix: Set(display_prefix(&plaintext)),
        scope: Set(scope.as_str().to_string()),
        created_at: Set(now),
        last_used_at: Set(None),
        expires_at: Set(expires_at),
        revoked_at: Set(None),
        ..Default::default()
    }
    .insert(db)
    .await?;

    Ok(IssuedApiKey { plaintext, model })
}

/// Look up the live (non-revoked, non-expired) key row matching `presented`, if any.
async fn find_live(
    db: &DatabaseConnection,
    presented: &str,
) -> Result<Option<api_key::Model>, AppError> {
    let hash = hash_key(presented);
    let row = ApiKey::find()
        .filter(api_key::Column::TokenHash.eq(hash))
        .filter(api_key::Column::RevokedAt.is_null())
        .one(db)
        .await?;

    Ok(match row {
        Some(row) if is_expired(&row, Utc::now()) => None,
        other => other,
    })
}

fn is_expired(row: &api_key::Model, now: DateTime<Utc>) -> bool {
    row.expires_at.is_some_and(|exp| exp <= now)
}

/// SQL predicate matching a *live* key: not revoked, and either non-expiring or not
/// yet past its expiry. Shared by the list + cap-count so both agree with [`resolve`]'s
/// liveness — a dead (revoked or expired) key is neither shown as active nor counted
/// against the cap; the maintenance prune hard-deletes it shortly after.
fn live_filter(now: DateTimeUtc) -> sea_orm::sea_query::SimpleExpr {
    api_key::Column::RevokedAt.is_null().and(
        api_key::Column::ExpiresAt
            .is_null()
            .or(api_key::Column::ExpiresAt.gt(now)),
    )
}

/// Resolve a presented key to its owner + scope (the extractor's authorization
/// path). Rejects an unknown / revoked / expired key, or one whose user is gone,
/// with `Unauthorized`. Best-effort stamps `last_used_at` (throttled).
pub async fn resolve(db: &DatabaseConnection, presented: &str) -> Result<ResolvedApiKey, AppError> {
    let invalid = || AppError::Unauthorized("invalid or expired api key".to_string());

    let row = find_live(db, presented).await?.ok_or_else(invalid)?;

    let user = User::find_by_id(row.user_id)
        .one(db)
        .await?
        .ok_or_else(invalid)?;

    let scope = ApiKeyScope::from_str_lenient(&row.scope);

    touch_last_used(db, row).await;

    Ok(ResolvedApiKey { user, scope })
}

/// Resolve a presented key to just its owning user id — the cheap lookup the
/// per-user rate limiter needs (no user load, no `last_used_at` write). `Ok(None)`
/// for an unknown / revoked / expired key, so the limiter passes it through (the
/// extractor then rejects it with 401).
pub async fn resolve_user_id(
    db: &DatabaseConnection,
    presented: &str,
) -> Result<Option<i32>, AppError> {
    Ok(find_live(db, presented).await?.map(|row| row.user_id))
}

/// Best-effort, throttled `last_used_at` stamp. A failure here must never fail the
/// authenticated request (the timestamp is cosmetic), so the error is logged, not
/// propagated.
async fn touch_last_used(db: &DatabaseConnection, row: api_key::Model) {
    let now = Utc::now();
    let due = row
        .last_used_at
        .is_none_or(|prev| (now - prev) >= Duration::seconds(LAST_USED_THROTTLE_SECONDS));
    if !due {
        return;
    }

    let mut active: api_key::ActiveModel = row.into();
    active.last_used_at = Set(Some(now));
    if let Err(err) = active.update(db).await {
        tracing::debug!(error = %err, "failed to stamp api_key.last_used_at (non-fatal)");
    }
}

/// Soft-revoke the key `key_id` owned by `user_id`. Returns `false` when no such
/// key exists for that user (so the caller can 404 without leaking another user's
/// key ids); revoking an already-revoked key is an idempotent `true`.
pub async fn revoke(
    db: &DatabaseConnection,
    key_id: i32,
    user_id: i32,
) -> Result<bool, AppError> {
    let Some(row) = ApiKey::find_by_id(key_id)
        .filter(api_key::Column::UserId.eq(user_id))
        .one(db)
        .await?
    else {
        return Ok(false);
    };

    if row.revoked_at.is_none() {
        let mut active: api_key::ActiveModel = row.into();
        active.revoked_at = Set(Some(Utc::now()));
        active.update(db).await?;
    }

    Ok(true)
}

/// The user's live (usable) keys, newest first — the management list. Excludes
/// revoked and expired keys, so the list only ever shows keys that actually work.
pub async fn list_active_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<api_key::Model>, AppError> {
    Ok(ApiKey::find()
        .filter(api_key::Column::UserId.eq(user_id))
        .filter(live_filter(Utc::now()))
        .order_by_desc(api_key::Column::CreatedAt)
        .all(db)
        .await?)
}

/// How many live (usable) keys the user currently holds — the per-user cap check.
/// Counts only keys that still work (not revoked, not expired), matching the list, so
/// an already-dead key never blocks a new one; pruning hard-deletes it shortly after.
pub async fn count_active_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<u64, AppError> {
    Ok(ApiKey::find()
        .filter(api_key::Column::UserId.eq(user_id))
        .filter(live_filter(Utc::now()))
        .count(db)
        .await?)
}

/// Delete keys that are already dead — expired or revoked — to bound table growth.
/// Both are rejected on use regardless, so removing them changes no behaviour.
/// Returns the number of rows pruned.
pub async fn prune_dead(db: &DatabaseConnection) -> Result<u64, AppError> {
    let result = ApiKey::delete_many()
        .filter(
            api_key::Column::ExpiresAt
                .lte(Utc::now())
                .or(api_key::Column::RevokedAt.is_not_null()),
        )
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{insert_user, migrated_memory_db};

    async fn setup_db() -> DatabaseConnection {
        migrated_memory_db().await
    }

    #[test]
    fn generated_keys_are_prefixed_distinct_and_hash_is_deterministic() {
        let a = generate_key();
        let b = generate_key();
        assert!(a.starts_with("tcgl_"));
        // "tcgl_" (5) + 32 bytes -> 64 hex chars.
        assert_eq!(a.len(), 5 + 64);
        assert_ne!(a, b);
        assert_eq!(hash_key(&a), hash_key(&a));
        assert_ne!(hash_key(&a), hash_key(&b));
        // The display prefix is the label + 8 hex chars and is a strict prefix.
        let prefix = display_prefix(&a);
        assert_eq!(prefix.len(), 5 + 8);
        assert!(a.starts_with(&prefix));
    }

    #[test]
    fn scope_round_trips_and_defaults_to_least_privilege() {
        assert_eq!(ApiKeyScope::from_str_lenient("read"), ApiKeyScope::Read);
        assert_eq!(
            ApiKeyScope::from_str_lenient("read_write"),
            ApiKeyScope::ReadWrite
        );
        // Unknown -> Read (never silently grants write).
        assert_eq!(ApiKeyScope::from_str_lenient("garbage"), ApiKeyScope::Read);
        assert!(!ApiKeyScope::Read.permits_write());
        assert!(ApiKeyScope::ReadWrite.permits_write());
        assert!(ApiKeyScope::parse_request("nope").is_err());
    }

    #[tokio::test]
    async fn issue_then_resolve_returns_owner_and_scope() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "keys@example.com").await;

        let issued = issue_api_key(&db, user_id, "ci", ApiKeyScope::ReadWrite, None)
            .await
            .expect("issue");
        assert!(issued.plaintext.starts_with("tcgl_"));
        assert_eq!(issued.model.scope, "read_write");
        assert!(issued.model.last_used_at.is_none());

        let resolved = resolve(&db, &issued.plaintext).await.expect("resolve");
        assert_eq!(resolved.user.id, user_id);
        assert_eq!(resolved.scope, ApiKeyScope::ReadWrite);

        // last_used_at is stamped on first resolve.
        let row = ApiKey::find_by_id(issued.model.id)
            .one(&db)
            .await
            .expect("query")
            .expect("row");
        assert!(row.last_used_at.is_some());

        // The rate-limiter helper returns the same user.
        assert_eq!(
            resolve_user_id(&db, &issued.plaintext).await.expect("uid"),
            Some(user_id)
        );
    }

    #[tokio::test]
    async fn unknown_and_revoked_and_expired_keys_are_rejected() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "reject@example.com").await;

        // Unknown.
        assert!(resolve(&db, "tcgl_deadbeef").await.is_err());
        assert_eq!(resolve_user_id(&db, "tcgl_deadbeef").await.unwrap(), None);

        // Revoked.
        let issued = issue_api_key(&db, user_id, "revoke-me", ApiKeyScope::Read, None)
            .await
            .expect("issue");
        assert!(revoke(&db, issued.model.id, user_id).await.expect("revoke"));
        assert!(resolve(&db, &issued.plaintext).await.is_err());
        assert_eq!(resolve_user_id(&db, &issued.plaintext).await.unwrap(), None);

        // Expired (planted in the past).
        let expired = issue_api_key(
            &db,
            user_id,
            "expired",
            ApiKeyScope::Read,
            Some(Utc::now() - Duration::hours(1)),
        )
        .await
        .expect("issue");
        assert!(resolve(&db, &expired.plaintext).await.is_err());
    }

    #[tokio::test]
    async fn revoke_is_scoped_and_idempotent() {
        let db = setup_db().await;
        let owner = insert_user(&db, "owner@example.com").await;
        let other = insert_user(&db, "other@example.com").await;

        let issued = issue_api_key(&db, owner, "k", ApiKeyScope::Read, None)
            .await
            .expect("issue");

        // Another user cannot revoke it (and learns nothing — false, not an error).
        assert!(!revoke(&db, issued.model.id, other).await.expect("revoke"));
        // A non-existent id for the real owner is likewise false.
        assert!(!revoke(&db, 999_999, owner).await.expect("revoke"));

        // The owner revokes it, and a second revoke is an idempotent true.
        assert!(revoke(&db, issued.model.id, owner).await.expect("revoke"));
        assert!(revoke(&db, issued.model.id, owner).await.expect("revoke"));
    }

    #[tokio::test]
    async fn list_and_count_show_only_live_keys_and_prune_reclaims() {
        let db = setup_db().await;
        let user_id = insert_user(&db, "list@example.com").await;

        let live = issue_api_key(&db, user_id, "live", ApiKeyScope::Read, None)
            .await
            .expect("issue");
        let revoked = issue_api_key(&db, user_id, "revoked", ApiKeyScope::Read, None)
            .await
            .expect("issue");
        let _expired = issue_api_key(
            &db,
            user_id,
            "expired",
            ApiKeyScope::Read,
            Some(Utc::now() - Duration::hours(1)),
        )
        .await
        .expect("issue");
        revoke(&db, revoked.model.id, user_id).await.expect("revoke");

        // List/count show only usable keys — the revoked and expired ones (both dead,
        // both rejected by `resolve`) are excluded, so just the live key remains.
        let listed = list_active_for_user(&db, user_id).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, live.model.id);
        assert_eq!(count_active_for_user(&db, user_id).await.expect("count"), 1);

        // Prune drops the dead rows (revoked + expired), leaving just the live key.
        let pruned = prune_dead(&db).await.expect("prune");
        assert_eq!(pruned, 2);
        let remaining = ApiKey::find().all(&db).await.expect("all");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, live.model.id);
    }
}

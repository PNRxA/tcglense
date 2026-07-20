//! Cross-replica coordination via Postgres **session advisory locks** (issue #413).
//!
//! Two places race when several API replicas share one Postgres (the prod-split
//! compose scaled past one instance):
//!
//! * **Migrations on boot** — every process runs `Migrator::up` unconditionally;
//!   simultaneous boots race the same DDL (`seaql_migrations` is bookkeeping, not
//!   a lock).
//! * **The card-sync tick** — every process runs its own ticker, and the version
//!   gate only short-circuits on a *completed* import, so a replica ticking while
//!   a peer is mid-import starts a second full ~500 MB import (plus the doubled
//!   daily snapshot / upsert storm against the shared DB).
//!
//! A Postgres advisory lock is the right primitive here (not Redis): it needs no
//! new infrastructure, it's already the in-repo pattern for exactly-once across
//! instances (`auth::email_token`'s `pg_advisory_xact_lock` cooldown), and a
//! session lock auto-releases the moment its connection dies — a crashed leader
//! needs no TTL bookkeeping. The locks here use the **single-`bigint`** key form,
//! which cannot collide with the email cooldown's two-`int` `(user_id, purpose)`
//! key space (Postgres encodes the two forms differently).
//!
//! Each lock lives on its own **dedicated connection**, dialled straight from the
//! `DATABASE_URL` — deliberately *not* a checkout from the SeaORM pool. A pooled
//! checkout would pin a pool slot for the lock's whole lifetime (the sync lease
//! spans a multi-hour import), and at `DB_MAX_CONNECTIONS=1` the migration path
//! would deadlock against itself: the lock holds the only slot while
//! `Migrator::up` waits for one. Releasing is simply closing that connection —
//! the server drops a session's advisory locks with the session, so there is no
//! unlock statement to get wrong and nothing lock-tainted ever returns to a pool.
//!
//! Session locks assume `DATABASE_URL` is a **direct** connection, exactly as the
//! migrations themselves already do (see `m..027`'s pooler note): behind a
//! transaction-mode pooler (e.g. pgbouncer), each statement may land on a
//! different server connection and session-scoped locks are meaningless.
//!
//! **Degradation contract** (the rate limiters' fail-open, applied to
//! coordination): on SQLite the lock is a trivially-held no-op (the default
//! self-host is a single process, and none of our deploys share a SQLite file
//! between replicas); on any acquisition *error* (dial failure, dropped
//! connection) the caller proceeds as if it held the lock, with a warning — the
//! worst case is exactly today's unguarded behaviour, never a refused boot or a
//! skipped-forever sync.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection};
use sqlx::{Connection, PgConnection, Row};

/// The app's advisory-lock keys, namespaced under an arbitrary high tag so they
/// can never collide with anything else int8-keyed on a shared database.
const KEY_NAMESPACE: i64 = 0x7C67_4C00 << 16;

/// Serialises `Migrator::up` across simultaneously booting replicas.
pub const MIGRATIONS: i64 = KEY_NAMESPACE | 1;

/// Elects the card-sync leader for one tick (refresh + snapshot + backfill spawn).
pub const CARD_SYNC: i64 = KEY_NAMESPACE | 2;

/// Elects the price-alert evaluation leader for one tick (issue #525), so a
/// multi-replica deployment delivers each triggered alert once, not once per replica.
pub const ALERTS: i64 = KEY_NAMESPACE | 3;

/// A held (or trivially-held) advisory lock, owning the dedicated connection the
/// lock lives on. Release by [`Self::release`] (graceful close) or by dropping
/// (the socket closes and the server frees the session's locks either way).
pub struct AdvisoryLock {
    /// `None` = nothing to release (SQLite, or a fail-open acquisition error).
    conn: Option<PgConnection>,
}

impl AdvisoryLock {
    fn noop() -> Self {
        Self { conn: None }
    }

    /// Block until the lock for `key` is available, then hold it. Used by the
    /// migration path: a second booting replica *waits* for the first rather
    /// than racing it. Fails open on error (see the module docs).
    pub async fn acquire(db: &DatabaseConnection, database_url: &str, key: i64) -> Self {
        Self::lock(db, database_url, key, "SELECT pg_advisory_lock($1)", |_| {
            true
        })
        .await
        .unwrap_or_else(Self::noop)
    }

    /// Try to take the lock for `key` without waiting. `None` = a peer holds it
    /// (the caller should skip its turn); a trivially-held lock on SQLite or on
    /// an acquisition error (fail open).
    pub async fn try_acquire(
        db: &DatabaseConnection,
        database_url: &str,
        key: i64,
    ) -> Option<Self> {
        Self::lock(
            db,
            database_url,
            key,
            "SELECT pg_try_advisory_lock($1)",
            |row| row.try_get::<bool, _>(0).unwrap_or(true),
        )
        .await
    }

    /// Shared acquisition: dial a dedicated connection (see the module docs for
    /// why never a pool checkout) and run `sql` on it. `granted` reads the
    /// try-variant's boolean; the blocking variant always grants. Returns
    /// `Some(noop)` on SQLite/error (fail open) and `None` only when the
    /// try-variant reports the lock as held elsewhere.
    async fn lock(
        db: &DatabaseConnection,
        database_url: &str,
        key: i64,
        sql: &str,
        granted: impl Fn(&sqlx::postgres::PgRow) -> bool,
    ) -> Option<Self> {
        if db.get_database_backend() != DatabaseBackend::Postgres {
            return Some(Self::noop());
        }
        let mut conn = match PgConnection::connect(database_url).await {
            Ok(conn) => conn,
            Err(err) => {
                tracing::warn!(error = %err, key, "advisory lock: dedicated connect failed; failing open");
                return Some(Self::noop());
            }
        };
        match sqlx::query(sql).bind(key).fetch_one(&mut conn).await {
            Ok(row) if granted(&row) => Some(Self { conn: Some(conn) }),
            Ok(_) => None,
            Err(err) => {
                tracing::warn!(error = %err, key, "advisory lock: acquisition failed; failing open");
                Some(Self::noop())
            }
        }
    }

    /// Release the lock by gracefully closing its dedicated connection — the
    /// server frees a session's advisory locks with the session, so a close *is*
    /// the unlock. `Drop` covers the paths that never call this (the socket
    /// close releases server-side the same way, just less politely).
    pub async fn release(mut self) {
        if let Some(conn) = self.conn.take()
            && let Err(err) = conn.close().await
        {
            tracing::debug!(error = %err, "advisory lock: close failed (lock still releases with the session)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SQLite: both acquisition forms are trivially held (single process — nothing
    /// to coordinate) and release is a no-op.
    #[tokio::test]
    async fn sqlite_arm_is_a_trivially_held_noop() {
        let db = crate::test_support::migrated_memory_db().await;

        let blocking = AdvisoryLock::acquire(&db, "sqlite::memory:", CARD_SYNC).await;
        let try_taken = AdvisoryLock::try_acquire(&db, "sqlite::memory:", CARD_SYNC)
            .await
            .expect("sqlite try_acquire is always trivially held");
        blocking.release().await;
        try_taken.release().await;
    }

    /// Postgres: a held lock makes a peer's `try_acquire` skip, and releasing (or
    /// dropping) hands it over. `#[ignore]`d like the rest of the live-Postgres
    /// suite; run with `TCGLENSE_TEST_POSTGRES_URL=… cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
    async fn postgres_arm_excludes_peers_until_released() {
        let Ok(url) = std::env::var("TCGLENSE_TEST_POSTGRES_URL") else {
            return;
        };
        let db = sea_orm::Database::connect(crate::db::connect_options(url.clone()))
            .await
            .expect("connect test postgres");

        // A per-run key so parallel CI runs on a shared Postgres never collide.
        let key = KEY_NAMESPACE
            | i64::from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .subsec_nanos(),
            );

        let held = AdvisoryLock::try_acquire(&db, &url, key)
            .await
            .expect("first acquisition is granted");
        assert!(
            AdvisoryLock::try_acquire(&db, &url, key).await.is_none(),
            "a peer must be excluded while the lock is held"
        );

        held.release().await;
        let reacquired = AdvisoryLock::try_acquire(&db, &url, key)
            .await
            .expect("released lock is acquirable again");

        // Dropping without release() must also free it (the dedicated connection
        // closes, releasing the session lock server-side).
        drop(reacquired);
        // The server-side release is asynchronous with the socket close; poll briefly.
        let mut freed = false;
        for _ in 0..50 {
            if let Some(again) = AdvisoryLock::try_acquire(&db, &url, key).await {
                again.release().await;
                freed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            freed,
            "a dropped guard's lock must release when its connection closes"
        );
    }
}

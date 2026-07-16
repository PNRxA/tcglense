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
//! **Degradation contract** (the rate limiters' fail-open, applied to
//! coordination): on SQLite the lock is a trivially-held no-op (the default
//! self-host is a single process, and none of our deploys share a SQLite file
//! between replicas); on any acquisition *error* (pool exhausted, connection
//! drop) the caller proceeds as if it held the lock, with a warning — the worst
//! case is exactly today's unguarded behaviour, never a refused boot or a
//! skipped-forever sync.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection};
use sqlx::Row;
use sqlx::pool::PoolConnection;
use sqlx::postgres::Postgres;

/// The app's advisory-lock keys, namespaced under an arbitrary high tag so they
/// can never collide with anything else int8-keyed on a shared database.
const KEY_NAMESPACE: i64 = 0x7C67_4C00 << 16;

/// Serialises `Migrator::up` across simultaneously booting replicas.
pub const MIGRATIONS: i64 = KEY_NAMESPACE | 1;

/// Elects the card-sync leader for one tick (refresh + snapshot + backfill spawn).
pub const CARD_SYNC: i64 = KEY_NAMESPACE | 2;

/// A held (or trivially-held) advisory lock. Release by dropping — the normal
/// path unlocks explicitly and returns the connection to the pool; if the unlock
/// fails the pinned connection is detached instead, so it closes rather than
/// re-entering the pool with the server-side lock still attached.
pub struct AdvisoryLock {
    /// `None` = nothing to release (SQLite, or a fail-open acquisition error).
    conn: Option<PoolConnection<Postgres>>,
    key: i64,
}

impl AdvisoryLock {
    fn noop() -> Self {
        Self { conn: None, key: 0 }
    }

    /// Block until the lock for `key` is available, then hold it. Used by the
    /// migration path: a second booting replica *waits* for the first rather
    /// than racing it. Fails open on error (see the module docs).
    pub async fn acquire(db: &DatabaseConnection, key: i64) -> Self {
        Self::lock(db, key, "SELECT pg_advisory_lock($1)", |_| true)
            .await
            .unwrap_or_else(Self::noop)
    }

    /// Try to take the lock for `key` without waiting. `None` = a peer holds it
    /// (the caller should skip its turn); a trivially-held lock on SQLite or on
    /// an acquisition error (fail open).
    pub async fn try_acquire(db: &DatabaseConnection, key: i64) -> Option<Self> {
        match Self::lock(db, key, "SELECT pg_try_advisory_lock($1)", |row| {
            row.try_get::<bool, _>(0).unwrap_or(true)
        })
        .await
        {
            Some(lock) => Some(lock),
            // Held elsewhere: genuinely skip.
            None => None,
        }
    }

    /// Shared acquisition: pin one pooled connection (session locks belong to a
    /// connection, and SeaORM statements otherwise hop between pool members) and
    /// run `sql` on it. `granted` reads the try-variant's boolean; the blocking
    /// variant always grants. Returns `Some(noop)` on SQLite/error (fail open)
    /// and `None` only when the try-variant reports the lock as held elsewhere.
    async fn lock(
        db: &DatabaseConnection,
        key: i64,
        sql: &str,
        granted: impl Fn(&sqlx::postgres::PgRow) -> bool,
    ) -> Option<Self> {
        if db.get_database_backend() != DatabaseBackend::Postgres {
            return Some(Self::noop());
        }
        let pool = db.get_postgres_connection_pool();
        let mut conn = match pool.acquire().await {
            Ok(conn) => conn,
            Err(err) => {
                tracing::warn!(error = %err, key, "advisory lock: pool acquire failed; failing open");
                return Some(Self::noop());
            }
        };
        match sqlx::query(sql).bind(key).fetch_one(&mut *conn).await {
            Ok(row) if granted(&row) => Some(Self {
                conn: Some(conn),
                key,
            }),
            Ok(_) => None,
            Err(err) => {
                tracing::warn!(error = %err, key, "advisory lock: acquisition failed; failing open");
                Some(Self::noop())
            }
        }
    }

    /// Release the lock and return the pinned connection to the pool. Consumes
    /// the guard; `Drop` covers the paths that never call this.
    pub async fn release(mut self) {
        let Some(mut conn) = self.conn.take() else {
            return;
        };
        match sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(self.key)
            .execute(&mut *conn)
            .await
        {
            // Unlocked server-side: the connection is clean to re-pool (drop returns it).
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(error = %err, key = self.key, "advisory unlock failed; closing the pinned connection");
                // Detach so the connection closes instead of re-entering the pool
                // with the server-side lock still attached; the server releases
                // session locks when the connection dies.
                drop(conn.detach());
            }
        }
    }
}

impl Drop for AdvisoryLock {
    fn drop(&mut self) {
        // A guard dropped without `release()` (early return, panic unwind) must
        // not hand its still-locked connection back to the pool — detach it so it
        // closes, which releases the session lock server-side.
        if let Some(conn) = self.conn.take() {
            drop(conn.detach());
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

        let blocking = AdvisoryLock::acquire(&db, CARD_SYNC).await;
        let try_taken = AdvisoryLock::try_acquire(&db, CARD_SYNC)
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
        let db = sea_orm::Database::connect(crate::db::connect_options(url))
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

        let held = AdvisoryLock::try_acquire(&db, key)
            .await
            .expect("first acquisition is granted");
        assert!(
            AdvisoryLock::try_acquire(&db, key).await.is_none(),
            "a peer must be excluded while the lock is held"
        );

        held.release().await;
        let reacquired = AdvisoryLock::try_acquire(&db, key)
            .await
            .expect("released lock is acquirable again");

        // Dropping without release() must also free it (the pinned connection is
        // detached and closed, releasing the session lock server-side).
        drop(reacquired);
        // The server-side release is asynchronous with the socket close; poll briefly.
        let mut freed = false;
        for _ in 0..50 {
            if let Some(again) = AdvisoryLock::try_acquire(&db, key).await {
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

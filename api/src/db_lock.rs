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
//!
//! **Observability + zombie hygiene** (the 2026-08 snapshot outage): the dedicated
//! connection is dialled with an `application_name` naming its key
//! (`tcglense-lock:card_sync`), a granted lease turns on aggressive server-side TCP
//! keepalives so a holder that dies *without* closing its socket is reaped in ~2
//! minutes rather than holding the lock indefinitely, and a lost `try_acquire` logs
//! who holds the lock (pid, state, `backend_start`, client address) from `pg_locks`
//! before skipping — a live peer mid-sync reads very differently from a days-idle
//! stranded session.

use std::str::FromStr;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection};
use sqlx::{ConnectOptions, Connection, PgConnection, Row, postgres::PgConnectOptions};

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

/// Elects the release-notification leader for one tick (Secret Lair drop / set release
/// heads-ups), so a multi-replica deployment delivers each heads-up once, not per replica.
pub const RELEASE_ALERTS: i64 = KEY_NAMESPACE | 4;

/// Serialises the one-time TCGCSV historic price backfill for the walk's **whole
/// duration** (held by the backfill task itself, not per tick): the backfill's internal
/// `ingest_state` completion gate stays open while a resumable, possibly multi-hour walk
/// is in progress, so without this lock several replicas booting together would all walk
/// the daily archives at once and interleave the shared resume cursor.
pub const PRICE_BACKFILL: i64 = KEY_NAMESPACE | 5;

/// Human-readable name for a known lock key. Stamped into the dedicated connection's
/// `application_name` (so `pg_stat_activity` names a holder as e.g. `tcglense-lock:card_sync`
/// instead of an anonymous session) and used in the held-elsewhere log line.
fn key_name(key: i64) -> &'static str {
    match key {
        MIGRATIONS => "migrations",
        CARD_SYNC => "card_sync",
        ALERTS => "alerts",
        RELEASE_ALERTS => "release_alerts",
        PRICE_BACKFILL => "price_backfill",
        _ => "other",
    }
}

/// The halves an int8 advisory key surfaces as in `pg_locks` (`classid` = high 32 bits,
/// `objid` = low 32 bits, `objsubid` = 1) — the join key for naming a lock's holder.
fn key_parts(key: i64) -> (i64, i64) {
    (key >> 32, key & 0xFFFF_FFFF)
}

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
        // Name the session after the lock it exists for, so `pg_stat_activity` identifies
        // a holder at a glance (a URL parse failure falls back to an unnamed dial rather
        // than failing the lock).
        let connect = match PgConnectOptions::from_str(database_url) {
            Ok(options) => {
                options
                    .application_name(&format!("tcglense-lock:{}", key_name(key)))
                    .connect()
                    .await
            }
            Err(_) => PgConnection::connect(database_url).await,
        };
        let mut conn = match connect {
            Ok(conn) => conn,
            Err(err) => {
                tracing::warn!(error = %err, key, "advisory lock: dedicated connect failed; failing open");
                return Some(Self::noop());
            }
        };
        match sqlx::query(sql).bind(key).fetch_one(&mut conn).await {
            Ok(row) if granted(&row) => {
                enable_session_keepalives(&mut conn, key).await;
                Some(Self { conn: Some(conn) })
            }
            Ok(_) => {
                log_holder(&mut conn, key).await;
                None
            }
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

/// Ask the server to probe this session's TCP peer aggressively (`tcp_keepalives_*`
/// session GUCs: first probe after 60 s idle, then every 15 s, dead after 4 misses), so a
/// lock whose holder died *without* its socket closing — an abrupt container teardown, a
/// network partition, anything that strands a half-open connection — is reaped by the
/// server in ~2 minutes instead of held until the OS-default keepalive fires hours later.
/// A CARD_SYNC lock stranded exactly that way blocked every daily price snapshot for a
/// week in 2026-08. Best-effort: the settings are ignored on Unix-socket connections and
/// rejected on platforms without per-socket keepalive support, both of which just keep the
/// old behaviour.
async fn enable_session_keepalives(conn: &mut PgConnection, key: i64) {
    // Fixed name/value literals — the format! carries no untrusted input.
    for (setting, value) in [
        ("tcp_keepalives_idle", 60),
        ("tcp_keepalives_interval", 15),
        ("tcp_keepalives_count", 4),
    ] {
        if let Err(err) = sqlx::query(&format!("SET {setting} = {value}"))
            .execute(&mut *conn)
            .await
        {
            tracing::debug!(error = %err, key, setting, "advisory lock: keepalive setting rejected");
        }
    }
}

/// Best-effort: name the session holding `key` (via `pg_locks` x `pg_stat_activity`)
/// before reporting it as taken, so a "held by another session" skip says *who* — a live
/// peer mid-work reads very differently from a `state = idle` session whose
/// `backend_start` is days old (a stranded holder an operator should terminate). Runs on
/// the already-dialled candidate connection; any failure downgrades to the bare skip.
async fn log_holder(conn: &mut PgConnection, key: i64) {
    let (classid, objid) = key_parts(key);
    let row = sqlx::query(
        "SELECT a.pid, a.state, a.backend_start::text, a.state_change::text, \
                a.client_addr::text, a.application_name \
         FROM pg_locks l JOIN pg_stat_activity a ON a.pid = l.pid \
         WHERE l.locktype = 'advisory' AND l.granted \
           AND l.classid::bigint = $1 AND l.objid::bigint = $2 AND l.objsubid = 1",
    )
    .bind(classid)
    .bind(objid)
    .fetch_optional(&mut *conn)
    .await;
    match row {
        Ok(Some(row)) => {
            let text = |i: usize| {
                row.try_get::<Option<String>, _>(i)
                    .ok()
                    .flatten()
                    .unwrap_or_default()
            };
            tracing::info!(
                key = key_name(key),
                holder_pid = row.try_get::<i32, _>(0).unwrap_or_default(),
                holder_state = %text(1),
                holder_since = %text(2),
                holder_state_change = %text(3),
                holder_addr = %text(4),
                holder_app = %text(5),
                "advisory lock is held by another session"
            );
        }
        Ok(None) => tracing::info!(
            key = key_name(key),
            "advisory lock is held, but its holder is not visible in pg_stat_activity"
        ),
        Err(err) => {
            tracing::debug!(error = %err, key = key_name(key), "advisory lock: holder lookup failed")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the `pg_locks` encoding of the app's int8 advisory keys: `classid` is the high
    /// half, `objid` the low half. These are the values the holder-diagnostics join uses
    /// AND the values an operator greps `pg_locks` for by hand — a drifting split would
    /// silently break both.
    #[test]
    fn key_parts_match_the_pg_locks_encoding() {
        assert_eq!(key_parts(CARD_SYNC), (31847, 1275068418));
        assert_eq!(key_parts(MIGRATIONS), (31847, 1275068417));
        assert_eq!(key_parts(ALERTS), (31847, 1275068419));
        assert_eq!(key_parts(RELEASE_ALERTS), (31847, 1275068420));
        assert_eq!(key_parts(PRICE_BACKFILL), (31847, 1275068421));
    }

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

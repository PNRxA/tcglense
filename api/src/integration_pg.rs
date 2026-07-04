//! Opt-in Postgres integration tests. Gated on the `TCGLENSE_TEST_POSTGRES_URL` env
//! var **and** `#[ignore]`, so the default `cargo test` (in-memory SQLite, the
//! `backend-tests` CI job) never runs them and `cargo test -- --ignored` without a
//! Postgres reachable passes trivially (each test early-returns).
//!
//! Run locally against a docker Postgres (see `deploy/docker-compose.yml`) with:
//!   TCGLENSE_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres \
//!   cargo test -- --ignored
//!
//! Each test carves an isolated database off the base/admin URL (`CREATE DATABASE`),
//! runs the real migrations against it, exercises the code under test on **live
//! Postgres**, and drops the database after. These are the acceptance gate for the
//! backend-aware SQL seam (the search compiler / `issue_with_cooldown` / NULLS-LAST
//! rewrites) and the branched m001/m017 migrations: on a Postgres that hasn't had
//! those fixes they fail, which is the point — they gate a Postgres deploy.
//!
//! The Redis-backed rate-limiter tests live next to the limiter code in
//! `ratelimit.rs` (they need its private internals) and share the same
//! `cargo test -- --ignored` run via `TCGLENSE_TEST_REDIS_URL`; there is deliberately
//! no Redis slot here.

use std::collections::HashSet;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectOptions, ConnectionTrait, Database, DatabaseConnection,
    EntityTrait, PaginatorTrait, QueryFilter, Set, sea_query::OnConflict,
};
use sea_orm_migration::MigratorTrait;
use serde_json::Value;
use tower::ServiceExt;

use crate::auth::email_token::{self, EmailTokenPurpose};
use crate::auth::refresh;
use crate::entities::prelude::{CardPriceHistory, CollectionItem};
use crate::entities::{card_price_history, card_set, collection_item, user};
use crate::scryfall::{GAME, snapshot_prices};
use crate::test_support::{insert_card, insert_user, owned_counts, url_encode as enc};
use crate::{build_router, catalog, config::Config, migrator::Migrator, state::AppState};

/// A per-process counter so concurrently-created test databases never collide by name.
static DB_SEQ: AtomicU64 = AtomicU64::new(0);

/// Serialise `CREATE DATABASE` across the parallel test threads. Postgres clones
/// `template1` for a new database and two concurrent clones intermittently error
/// `source database "template1" is being accessed by other users` (SQLSTATE 55006);
/// one-at-a-time creation sidesteps the race without a retry loop. The guard is
/// released before connecting to the new database, so migrations still run in parallel.
static CREATE_DB_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

/// The base/admin Postgres URL (an existing database, e.g. `postgres`), or `None`
/// (skip the test) when the gate env var is unset/blank.
fn test_pg_url() -> Option<String> {
    std::env::var("TCGLENSE_TEST_POSTGRES_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Connect options for the integration harness with an **explicitly tiny** pool: many
/// parallel tests each open two pools (admin + test), so the ambient `DB_MAX_CONNECTIONS`
/// default (10) would blow `postgres:17`'s default `max_connections` (100). Two
/// connections per pool is plenty for a single test.
fn it_connect_options(url: &str) -> ConnectOptions {
    let mut opts = crate::db::connect_options(url);
    opts.max_connections(2).min_connections(0);
    opts
}

/// Swap the database name in a `postgres://…[/<db>][?params]` URL, preserving
/// credentials, host, and any query string. The base URL may omit the database path
/// entirely (`postgres://host:port`) — the path slash must be looked up *after* the
/// `scheme://` authority, or a bare URL would be split inside the scheme's own slashes.
fn swap_db_name(base: &str, name: &str) -> String {
    let (head, query) = match base.split_once('?') {
        Some((h, q)) => (h, Some(q)),
        None => (base, None),
    };
    let authority_start = head.find("://").map(|i| i + 3).unwrap_or(0);
    let prefix = match head[authority_start..].find('/') {
        Some(i) => &head[..authority_start + i],
        None => head,
    };
    let mut url = format!("{prefix}/{name}");
    if let Some(q) = query {
        url.push('?');
        url.push_str(q);
    }
    url
}

/// Not gated: `swap_db_name` is a pure function, and both admin-URL shapes (with and
/// without a database path) must produce a well-formed per-test URL.
#[test]
fn swap_db_name_handles_all_admin_url_shapes() {
    assert_eq!(
        swap_db_name("postgres://u:p@host:5433/postgres", "it_1"),
        "postgres://u:p@host:5433/it_1"
    );
    assert_eq!(
        swap_db_name("postgres://u:p@host:5433", "it_1"),
        "postgres://u:p@host:5433/it_1"
    );
    assert_eq!(
        swap_db_name("postgres://host/db?sslmode=disable", "it_1"),
        "postgres://host/it_1?sslmode=disable"
    );
}

/// An isolated, migrated Postgres database for one test, plus the admin connection it
/// was carved from (kept open for teardown).
struct PgTestDb {
    admin: DatabaseConnection,
    conn: DatabaseConnection,
    name: String,
}

impl PgTestDb {
    /// `CREATE` a uniquely-named database off `base`, connect to it, and run every
    /// migration. `CREATE DATABASE` is serialised (see [`CREATE_DB_LOCK`]).
    async fn create(base: &str) -> Self {
        let admin = Database::connect(it_connect_options(base))
            .await
            .expect("connect admin");
        // `[a-z0-9_]` only (pid + counter), never user input — safe to interpolate.
        let name = format!(
            "tcglense_it_{}_{}",
            std::process::id(),
            DB_SEQ.fetch_add(1, Ordering::SeqCst)
        );
        {
            let _guard = CREATE_DB_LOCK.lock().await;
            admin
                .execute_unprepared(&format!(r#"CREATE DATABASE "{name}""#))
                .await
                .expect("create database");
        }
        let url = swap_db_name(base, &name);
        let conn = Database::connect(it_connect_options(&url))
            .await
            .expect("connect test db");
        Migrator::up(&conn, None).await.expect("migrate");
        Self { admin, conn, name }
    }

    fn conn(&self) -> &DatabaseConnection {
        &self.conn
    }

    /// Best-effort teardown: close the test pool, terminate any lingering backends on
    /// the database (a leaked connection would block `DROP DATABASE`), then drop it. A
    /// panicking test skips this and leaks a throwaway database in the ephemeral CI
    /// container — harmless (unique names never collide across reruns).
    async fn teardown(self) {
        let _ = self.conn.close().await;
        let _ = self
            .admin
            .execute_unprepared(&format!(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
                 WHERE datname = '{}' AND pid <> pg_backend_pid()",
                self.name
            ))
            .await;
        let _ = self
            .admin
            .execute_unprepared(&format!(r#"DROP DATABASE IF EXISTS "{}""#, self.name))
            .await;
        let _ = self.admin.close().await;
    }
}

/// Build the real router over a Postgres-backed `AppState` (mirrors the security-test
/// harness, but with a caller-supplied `db`). Passes `None` for the Redis connection —
/// the in-memory limiter is exercised, identical behaviour, matching `harness.rs`.
fn pg_router(db: DatabaseConnection) -> Router {
    let config = Config {
        data_dir: std::env::temp_dir().join("tcglense-it-pg"),
        ..crate::test_support::test_config()
    };
    let http = reqwest::Client::builder().build().expect("http");
    let image_http = reqwest::Client::builder().build().expect("image http");
    let state = AppState::new(config, db, http, image_http, None).expect("state");
    build_router(state)
}

/// Drive one GET through the router and return `(status, json_body)`.
async fn get(router: &Router, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.expect("router infallible");
    let status = res.status();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.expect("body");
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

/// `data.len()` for a `{ data: [...] }` body (0 if absent).
fn data_len(body: &Value) -> usize {
    body["data"].as_array().map(|a| a.len()).unwrap_or(0)
}

/// Whether any card in a `{ data: [...] }` list has a name containing `needle`.
fn any_name_contains(body: &Value, needle: &str) -> bool {
    body["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .any(|c| c["name"].as_str().is_some_and(|n| n.contains(needle)))
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Migrations / schema
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn migrations_apply_and_are_idempotent_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    // `create` already ran the full chain; re-running is a no-op (sea-orm skips applied
    // migrations). Proves the 17-migration chain (incl. the branched m001/m017 PG arms)
    // builds cleanly and is re-runnable — the boot path.
    Migrator::up(db.conn(), None)
        .await
        .expect("re-running migrations is idempotent");
    db.teardown().await;
}

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn migration_down_up_roundtrip_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    // Reverts all 17 (strongest exercise of the PG `down` arms: m017's DELETE-NULLs +
    // SET NOT NULL — 0 rows on a fresh DB — and m001's DROP TABLE), then re-applies.
    Migrator::down(db.conn(), None).await.expect("migrate down");
    Migrator::up(db.conn(), None).await.expect("migrate back up");
    db.teardown().await;
}

// ---------------------------------------------------------------------------
// Auth / DB semantics
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn email_case_insensitive_uniqueness_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    let conn = db.conn();
    let now = Utc::now();

    let insert = |email: &str| {
        let email = email.to_string();
        async move {
            user::ActiveModel {
                email: Set(email),
                password_hash: Set(Some("x".to_string())),
                display_name: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(conn)
            .await
        }
    };

    insert("foo@x.test").await.expect("first insert");
    // A different-case duplicate must be rejected by the m001 lower(email) functional
    // unique index (Postgres has no COLLATE NOCASE — this is the PG-arm equivalent).
    assert!(
        insert("FOO@x.test").await.is_err(),
        "case-variant email must collide on the lower(email) unique index"
    );

    db.teardown().await;
}

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn issue_with_cooldown_holds_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    let conn = db.conn();

    // Sequential: first issues, an immediate second is inside the cooldown -> None.
    let uid = insert_user(conn, "cd@x.test").await;
    let first = email_token::issue_with_cooldown(conn, uid, EmailTokenPurpose::VerifyEmail)
        .await
        .expect("first issue");
    assert!(first.is_some());
    let second = email_token::issue_with_cooldown(conn, uid, EmailTokenPurpose::VerifyEmail)
        .await
        .expect("second issue");
    assert!(second.is_none(), "the immediate re-issue is inside the cooldown");

    // The critical concurrency invariant (mirrors the SQLite unit test on real pooled
    // Postgres): a 20-way burst must issue EXACTLY ONE token. On Postgres the atomic
    // `INSERT … WHERE NOT EXISTS` is NOT atomic under READ COMMITTED, so this only holds
    // if the `pg_advisory_xact_lock` path works — this test is that path's only guard.
    let burst_uid = insert_user(conn, "burst@x.test").await;
    let mut handles = Vec::new();
    for _ in 0..20 {
        let conn = conn.clone();
        handles.push(tokio::spawn(async move {
            email_token::issue_with_cooldown(&conn, burst_uid, EmailTokenPurpose::ResetPassword)
                .await
        }));
    }
    let mut issued = 0;
    for handle in handles {
        if handle.await.expect("task").expect("issue").is_some() {
            issued += 1;
        }
    }
    assert_eq!(issued, 1, "exactly one token may be issued within the cooldown");

    let rows = crate::entities::prelude::EmailToken::find()
        .filter(crate::entities::email_token::Column::UserId.eq(burst_uid))
        .count(conn)
        .await
        .expect("count tokens");
    assert_eq!(rows, 1, "exactly one row lands for the burst user");

    db.teardown().await;
}

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn refresh_rotation_is_single_use_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    let conn = db.conn();

    let uid = insert_user(conn, "rot@x.test").await;
    let original = refresh::issue_refresh_token(conn, uid, 30)
        .await
        .expect("issue");
    let rotated = refresh::rotate(conn, &original, 30).await.expect("rotate");

    // Replaying the now-superseded original is rejected (the conditional UPDATE claim,
    // `rows_affected`-gated, works under a real Postgres pool)...
    assert!(
        refresh::rotate(conn, &original, 30).await.is_err(),
        "a superseded refresh token can't be rotated again"
    );
    // ...while the freshly-minted successor still rotates.
    refresh::rotate(conn, &rotated.plaintext, 30)
        .await
        .expect("successor rotates");

    db.teardown().await;
}

// ---------------------------------------------------------------------------
// Collection / prices
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn collection_upsert_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    let conn = db.conn();

    let uid = insert_user(conn, "coll@x.test").await;
    let card_id = insert_card(conn, "coll-ext-1").await;

    // The exact ON CONFLICT upsert `handlers/collection/write.rs` uses, run twice: the
    // second insert conflicts on (user_id, game, card_id) and DO UPDATEs. On Postgres
    // this only works if the arbiter is inferred from the real unique index.
    let upsert = |q: i32, f: i32| {
        let now = Utc::now();
        async move {
            CollectionItem::insert(collection_item::ActiveModel {
                user_id: Set(uid),
                game: Set(GAME.to_string()),
                card_id: Set(card_id),
                quantity: Set(q),
                foil_quantity: Set(f),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::columns([
                    collection_item::Column::UserId,
                    collection_item::Column::Game,
                    collection_item::Column::CardId,
                ])
                .update_columns([
                    collection_item::Column::Quantity,
                    collection_item::Column::FoilQuantity,
                    collection_item::Column::UpdatedAt,
                ])
                .to_owned(),
            )
            .exec(conn)
            .await
            .expect("upsert");
        }
    };

    upsert(2, 0).await;
    upsert(5, 1).await;
    assert_eq!(
        owned_counts(conn, uid, card_id).await,
        Some((5, 1)),
        "the conflicting insert updated the row (not duplicated it)"
    );

    db.teardown().await;
}

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn foil_star_consolidation_folds_on_pg() {
    // The issue #209 legacy fold on live Postgres: exercises the correlated-subquery UPDATE,
    // the insert-missing-base SELECT, and `REPLACE`/`LIKE` over the multibyte `★` — all of
    // which the SQLite unit tests cover, but the SQL runs on both backends unbranched.
    use crate::entities::card;
    use sea_orm::IntoActiveModel;

    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    let conn = db.conn();

    // A nonfoil base and its separately-modelled foil `★` sibling (share an oracle id).
    let insert = |id: i32, number: &'static str, finishes: &'static str| async move {
        card::Model {
            external_id: format!("ext-{id}"),
            set_code: "sld".into(),
            collector_number: number.into(),
            finishes: Some(finishes.into()),
            oracle_id: Some("ora-1".into()),
            ..crate::test_support::card_model(id)
        }
        .into_active_model()
        .insert(conn)
        .await
        .expect("insert card")
        .id
    };
    let base_id = insert(1, "741", "nonfoil").await;
    let star_id = insert(2, "741★", "foil").await;

    let uid = insert_user(conn, "foilstar@x.test").await;
    // A legacy star holding (stored as regular, pre-#209) and no base holding yet.
    CollectionItem::insert(collection_item::ActiveModel {
        user_id: Set(uid),
        game: Set(GAME.to_string()),
        card_id: Set(star_id),
        quantity: Set(2),
        foil_quantity: Set(0),
        created_at: Set(Utc::now()),
        updated_at: Set(Utc::now()),
        ..Default::default()
    })
    .exec(conn)
    .await
    .expect("insert star holding");

    crate::migrator::consolidate_foil_star_holdings(conn)
        .await
        .expect("fold");

    assert_eq!(owned_counts(conn, uid, base_id).await, Some((0, 2)), "folded to base foil");
    assert_eq!(owned_counts(conn, uid, star_id).await, None, "star holding removed");

    db.teardown().await;
}

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn price_snapshot_upsert_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    let conn = db.conn();
    catalog::seed_all(conn).await;

    let n = snapshot_prices(conn, GAME, "2099-01-01")
        .await
        .expect("first snapshot");
    assert!(n > 0, "seeded catalog should produce price rows");
    let n2 = snapshot_prices(conn, GAME, "2099-01-01")
        .await
        .expect("second snapshot");
    assert_eq!(n, n2, "re-snapshotting the same day processes the same rows");

    // The upsert on (game, card_id, as_of_date) means the second run must not duplicate:
    // exactly `n` rows exist for that date.
    let rows_for_date = CardPriceHistory::find()
        .filter(card_price_history::Column::AsOfDate.eq("2099-01-01"))
        .count(conn)
        .await
        .expect("count price rows");
    assert_eq!(rows_for_date, n, "same-day re-snapshot upserts, never duplicates");

    db.teardown().await;
}

// ---------------------------------------------------------------------------
// Search battery (router-driven over the seeded dummy catalog). Each case asserts no
// 5xx (catching SQL-dialect *errors*) plus a positive row assertion where a silent
// divergence would otherwise pass unnoticed.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn search_smoke_battery_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    catalog::seed_all(db.conn()).await;
    let router = pg_router(db.conn().clone());

    let cards = |q: &str| format!("/api/games/mtg/cards?q={}", enc(q));

    // Case-insensitive substring (LOWER LIKE / ILIKE) — silent-wrong on raw PG.
    let (status, body) = get(&router, &cards("sentinel")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(data_len(&body) > 0 && any_name_contains(&body, "Sentinel"));

    // Exact-name match (`!"…"`) — a single unique dummy name.
    let (status, body) = get(&router, &cards("!\"Dummy Foil-Only Showcase\"")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"].as_u64(), Some(1), "exact match hits one card");

    // Regex (`~*`) — matches the double-faced werewolf card.
    let (status, body) = get(&router, &cards("/Werewolf/")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(data_len(&body) > 0, "regex matches the transform card");

    // Legality via json ->> — seed one card's legalities so the filter must positively
    // match (the dummy catalog carries none, which would let a broken expression that
    // returns 0 rows slip through).
    db.conn()
        .execute_unprepared(
            "UPDATE cards SET legalities = '{\"standard\":\"legal\"}', full_art = TRUE \
             WHERE name = 'Dummy Foil-Only Showcase'",
        )
        .await
        .expect("seed legality + full_art on one card");
    let (status, body) = get(&router, &cards("f:standard")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["total"].as_u64(),
        Some(1),
        "json ->> legality matches the seeded card"
    );

    // Boolean flag `col IS TRUE` — the same seeded card is now the one full-art print.
    let (status, body) = get(&router, &cards("is:fullart")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"].as_u64(), Some(1), "IS TRUE matches the seeded card");

    // is:spell — STRPOS + type_line case-fold; instants/sorceries match.
    let (status, body) = get(&router, &cards("is:spell")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(data_len(&body) > 0, "is:spell matches non-land cards");

    // is:permanent — type_line case-fold LIKE.
    let (status, body) = get(&router, &cards("is:permanent")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(data_len(&body) > 0, "is:permanent matches creatures/artifacts");

    // cmc parity (integer-guard, no FLOOR) — dummy cmc values include 2 and 4.
    let (status, body) = get(&router, &cards("cmc:even")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(data_len(&body) > 0, "cmc:even matches even-cost cards");

    // Cross-column numeric compare (integer-string guard + CASE-guarded CAST). Dummy
    // power/toughness are NULL → 0 rows, but the query must not error.
    let (status, _) = get(&router, &cards("pow>tou")).await;
    assert_eq!(status, StatusCode::OK, "pow>tou compiles");

    // Negation totality (F1): the leaf for a numeric-stat range compare is total (0/1,
    // never NULL) because the integer guard is re-ANDed outside the CASE, so `-pow>=5`
    // INCLUDES the NULL-power dummy cards rather than silently dropping them. Every
    // dummy card has NULL power, so the negation returns the whole catalog.
    let (status, neg) = get(&router, &cards("-pow>=5")).await;
    assert_eq!(status, StatusCode::OK, "-pow>=5 compiles");
    let (all_status, all) = get(&router, "/api/games/mtg/cards?page_size=200").await;
    assert_eq!(all_status, StatusCode::OK);
    assert!(data_len(&neg) > 0, "-pow>=5 matches the NULL-power dummy cards");
    assert_eq!(
        neg["total"].as_u64(),
        all["total"].as_u64(),
        "-pow>=5 includes every (NULL-power) dummy card, not zero"
    );

    db.teardown().await;
}

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn invalid_posix_regex_is_422_not_500_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    catalog::seed_all(db.conn()).await;
    let router = pg_router(db.conn().clone());

    // `\p{L}` is a valid Rust-regex (so it clears our pre-validation) but invalid POSIX
    // ARE, so Postgres's `~*` raises SQLSTATE 2201B at execution. The DbErr→AppError
    // mapping must reclassify that as the same 422 a bad pattern gets on SQLite, not a
    // 500 leaking from the public, unauthenticated search box.
    let (status, body) = get(&router, &format!("/api/games/mtg/cards?q={}", enc("/\\p{L}/"))).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid POSIX regex must be 422, not 500: {body:?}"
    );
    assert_eq!(
        body["error"].as_str(),
        Some("invalid regular expression"),
        "422 carries the invalid-regex message: {body:?}"
    );

    db.teardown().await;
}

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn unique_cards_dedupes_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    catalog::seed_all(db.conn()).await;
    let router = pg_router(db.conn().clone());

    // The reprinted relic has two printings sharing one oracle_id.
    let (status, body) = get(
        &router,
        &format!("/api/games/mtg/cards?q={}", enc("!\"Dummy Reprinted Relic\"")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"].as_u64(), Some(2), "two printings before de-dup");

    // `unique:cards` collapses them to one — the GROUP BY → min-id IN-subquery rewrite
    // Postgres needs (it rejects the SQLite bare-column GROUP BY form).
    let (status, body) = get(
        &router,
        &format!(
            "/api/games/mtg/cards?q={}",
            enc("!\"Dummy Reprinted Relic\" unique:cards")
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["total"].as_u64(),
        Some(1),
        "unique:cards de-dups the shared oracle_id"
    );

    db.teardown().await;
}

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn order_usd_sorts_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    catalog::seed_all(db.conn()).await;
    let router = pg_router(db.conn().clone());

    let (status, body) = get(
        &router,
        "/api/games/mtg/cards?sort=usd&dir=asc&page_size=200",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "numeric CAST price sort compiles");
    let data = body["data"].as_array().expect("data array");
    assert!(!data.is_empty(), "the seeded catalog has priced cards");

    // The sort key is `COALESCE(usd, usd_foil)` cast to REAL, NULLS LAST (matches
    // `price_real_expr`). Assert the effective price is non-decreasing and every
    // unpriced (both null) card sorts after every priced one.
    let effective = |card: &Value| -> Option<f64> {
        for key in ["usd", "usd_foil"] {
            if let Some(v) = card["prices"][key].as_str().and_then(|s| s.parse::<f64>().ok()) {
                return Some(v);
            }
        }
        None
    };
    let mut prev: Option<f64> = None;
    let mut seen_null = false;
    for card in data {
        match effective(card) {
            Some(v) => {
                assert!(!seen_null, "a priced card sorted after an unpriced one");
                if let Some(p) = prev {
                    assert!(v >= p - 1e-9, "usd asc not non-decreasing: {p} then {v}");
                }
                prev = Some(v);
            }
            None => seen_null = true,
        }
    }

    db.teardown().await;
}

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn card_names_autocomplete_case_insensitive_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    catalog::seed_all(db.conn()).await;
    let router = pg_router(db.conn().clone());

    // A lowercase term must match the title-case names (ILIKE/LOWER); the result is a
    // list of DISTINCT names (the `GROUP BY name` + `ORDER BY MAX(rank)` rewrite that
    // Postgres needs — its DISTINCT form rejects an order-by expr not in the select).
    let (status, body) = get(&router, "/api/games/mtg/card-names?q=sentinel").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(!names.is_empty(), "case-insensitive match returns names");
    assert!(
        names.contains(&"Dummy White Sentinel"),
        "expected the Sentinel card among the suggestions: {names:?}"
    );
    let distinct: HashSet<&&str> = names.iter().collect();
    assert_eq!(names.len(), distinct.len(), "suggestions must be distinct");

    db.teardown().await;
}

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn set_list_nulls_last_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    catalog::seed_all(db.conn()).await;

    // Insert a set with no release date; it must sort LAST under `released_at DESC NULLS
    // LAST` (Postgres defaults DESC to NULLs FIRST, so a bare order_by_desc would put it
    // first — the `order_by_with_nulls(.., Last)` fix is what this pins).
    let now = Utc::now();
    card_set::ActiveModel {
        game: Set(GAME.to_string()),
        code: Set("zzz".to_string()),
        name: Set("Zzz Undated".to_string()),
        card_count: Set(0),
        digital: Set(false),
        released_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db.conn())
    .await
    .expect("insert undated set");

    let router = pg_router(db.conn().clone());
    let (status, body) = get(&router, "/api/games/mtg/sets").await;
    assert_eq!(status, StatusCode::OK);
    let data = body["data"].as_array().expect("data array");
    assert_eq!(
        data.last().and_then(|s| s["code"].as_str()),
        Some("zzz"),
        "the undated set sorts last (NULLS LAST), not first"
    );

    db.teardown().await;
}

// ---------------------------------------------------------------------------
// Sealed products (public catalog) — the PR #184 surface. Seeds the dummy catalog
// (which seeds products + their price history through the same ON CONFLICT upsert
// paths, so this also proves those arbiters resolve on PG) and exercises the two
// backend-aware fragments in the product handler: the case-folded `q` name search
// (LOWER-both) and the nullable, decimal-guarded price sort (CAST + NULLS LAST).
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a live Postgres; set TCGLENSE_TEST_POSTGRES_URL, run with --ignored"]
async fn products_list_search_and_price_sort_on_pg() {
    let Some(base) = test_pg_url() else {
        return;
    };
    let db = PgTestDb::create(&base).await;
    catalog::seed_all(db.conn()).await;
    let router = pg_router(db.conn().clone());

    // The full list comes back (the dummy catalog seeds several sealed products).
    let (status, body) = get(&router, "/api/games/mtg/products?page_size=200").await;
    assert_eq!(status, StatusCode::OK);
    assert!(data_len(&body) >= 4, "seeded catalog has sealed products");

    // A lowercase `q` must match the title-cased product names (the LOWER-both fold —
    // silent-wrong on raw PG, whose LIKE is case-sensitive).
    let (status, body) = get(
        &router,
        &format!("/api/games/mtg/products?q={}", enc("commander")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        data_len(&body) > 0 && any_name_contains(&body, "Commander"),
        "case-insensitive product search matches the Commander deck: {body:?}"
    );

    // Price sort asc: the numeric CAST (decimal-shape-guarded on PG, so a non-decimal
    // price string can't error the CAST) must compile, the effective price must be
    // non-decreasing, and the unpriced product parks last (NULLS LAST — Postgres
    // defaults DESC/ASC NULL placement such that a bare order_by would misplace it).
    let (status, body) = get(
        &router,
        "/api/games/mtg/products?sort=price&dir=asc&page_size=200",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "product price CAST sort compiles");
    let data = body["data"].as_array().expect("data array");
    assert!(!data.is_empty(), "the seeded catalog has priced products");

    let effective = |p: &Value| -> Option<f64> {
        for key in ["usd", "usd_foil"] {
            if let Some(v) = p["prices"][key].as_str().and_then(|s| s.parse::<f64>().ok()) {
                return Some(v);
            }
        }
        None
    };
    let mut prev: Option<f64> = None;
    let mut seen_null = false;
    for p in data {
        match effective(p) {
            Some(v) => {
                assert!(!seen_null, "a priced product sorted after an unpriced one");
                if let Some(prev) = prev {
                    assert!(v >= prev - 1e-9, "price asc not non-decreasing: {prev} then {v}");
                }
                prev = Some(v);
            }
            None => seen_null = true,
        }
    }
    assert!(
        seen_null,
        "the null-priced dummy product is present and parked last (NULLS LAST)"
    );

    db.teardown().await;
}

use sea_orm::{ConnectionTrait, DatabaseBackend};
use sea_orm_migration::prelude::*;

use crate::scryfall::search::array_member_expr;

/// Postgres-only trigram (`pg_trgm`) GIN index on the `kw:`/`keyword:` search leaf's
/// expression — the fourth of `m..027`'s family, surfaced by the 2026-09 prod slow log.
///
/// The keyword glossary (`/keywords/{game}/{slug}`, one page per rules keyword, every one
/// of them in the sitemap) shows a few priced cards that carry the keyword through the
/// catalog listing's `kw:"<Keyword>"` search. That leaf compiles
/// (`scryfall::search::compile::common::array_member`) to
/// `(',' || LOWER(COALESCE(keywords, '')) || ',') LIKE '%,<kw>,%' ESCAPE '\'` — doubly
/// unindexable by a b-tree (a function-wrapped column *and* a leading `%`), and nothing
/// else indexed it. So each glossary page sequentially scanned the whole ~140 MB `cards`
/// heap **twice** (the listing's `COUNT(*)`, then the price-sorted page): 5.0 s + 5.2 s per
/// page on the weak prod Postgres, for a filter whose selectivity is irrelevant to the cost
/// (a keyword no card carries scans exactly as much).
///
/// A GIN index with `gin_trgm_ops` on the exact compiled expression turns the `LIKE` into a
/// bitmap index scan (the `m..027` mechanism). It works better here than for a generic
/// substring: the wrapping commas are word separators to `pg_trgm`, so the needle's trigram
/// set is the keyword's own, undiluted by neighbours. Measured on a 106k-row repro of the
/// real schema (Postgres 16), the cost now scales with the matches — a bitmap heap scan
/// touches roughly one heap page per matching printing — where the scan read ~36,700
/// buffers whatever the keyword: a keyword under half a percent of the catalog (about 340
/// of the 365 glossary entries) reads 44–507 buffers, and the twenty-odd common ones a few
/// thousand; on prod that is seconds → milliseconds for the tail and a clear win for the
/// rest. Two honest caveats, both the same shape as `m..027`'s:
///
/// - **The bitmap plan's page order is readahead-hostile**, so on cold storage at
///   Postgres's default `effective_io_concurrency = 1` any broad keyword's bitmap scan
///   pays random reads where the sequential scan streamed (measured ~3x between 1 and 0 or
///   64 on a third-of-the-heap scan; worth setting on a self-host). Flying — ~9 % of all
///   printings, a third of the heap's pages — is where that stops being a win: the planner
///   may keep the sequential scan or pick the bitmap plan, and either way that one page is
///   a wash. Its real fix is the sibling change in the same PR: the glossary panel reads
///   `/cards/preview`, which drops the `COUNT(*)` half entirely for every keyword.
/// - **A needle with no word characters extracts no trigrams.** `kw:%` / `kw:.` / `kw:-`
///   fall into a full GIN scan plus a whole-table recheck, ~1.5–2x a sequential scan.
///   Reachable anonymously, per-IP limited, and already the case for `name:%` since
///   `m..027`; not a regression, but not free either.
///
/// **Lock-step.** The `CREATE INDEX` expression is rendered from
/// [`array_member_expr`] — the same function the leaf compiles through — so the two cannot
/// textually diverge; `keyword_filter_renders_the_indexed_expression`
/// (`scryfall::search::tests`) is the drift canary. Postgres matches an expression index by
/// comparing the expression, so a leaf that stopped rendering it would silently fall back
/// to the full scan (the way the name autocomplete once missed `idx_cards_name_trgm`,
/// issue #413). Changing the expression needs a *new* migration to rebuild the index.
///
/// **Postgres only**, like `m..027`: `pg_trgm` is a Postgres extension and the dev/test
/// SQLite DB is tiny, so the SQLite arm is a no-op and the `LIKE` runs byte-identically via
/// a scan. Same build notes as `m..027`: `CREATE EXTENSION IF NOT EXISTS pg_trgm` (already
/// present since that migration; a restricted managed role must pre-provision it), a plain
/// (non-`CONCURRENTLY`) build under a `SHARE` lock (~0.2 s and ~500 kB on the 106k-row
/// repro — `keywords` is short), and `SET LOCAL statement_timeout = 0` so a server-default
/// timeout can't roll the whole boot batch back. One more GIN index on the bulk card upsert's
/// write path; `fastupdate` absorbs it and the sync is a background job, so read latency wins.
///
/// The three sibling `array_member` leaves (`finishes`, `promo_types`, `frame_effects`) are
/// deliberately **not** indexed: no surface issues one unaccompanied, and this one-line
/// migration is the fix if a slow log ever shows one.
#[derive(DeriveMigrationName)]
pub struct Migration;

const INDEX_NAME: &str = "idx_cards_keywords_trgm";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite has no pg_trgm; the scan on the tiny dev DB is instant. No-op there.
        if manager.get_database_backend() != DatabaseBackend::Postgres {
            return Ok(());
        }
        let conn = manager.get_connection();
        // The whole pending batch runs in one transaction, so a server/role-default
        // statement_timeout killing the build would roll the batch back and fail boot.
        conn.execute_unprepared("SET LOCAL statement_timeout = 0")
            .await?;
        conn.execute_unprepared("CREATE EXTENSION IF NOT EXISTS pg_trgm")
            .await?;
        // The expression must be the leaf's own, verbatim — see the lock-step note above.
        let expr = array_member_expr("keywords");
        conn.execute_unprepared(&format!(
            "CREATE INDEX IF NOT EXISTS \"{INDEX_NAME}\" ON \"cards\" \
             USING gin ({expr} gin_trgm_ops)"
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() != DatabaseBackend::Postgres {
            return Ok(());
        }
        // Drop the index only; `pg_trgm` is `m..027`'s to own.
        manager
            .get_connection()
            .execute_unprepared(&format!("DROP INDEX IF EXISTS \"{INDEX_NAME}\""))
            .await?;
        Ok(())
    }
}

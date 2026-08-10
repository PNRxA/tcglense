//! Foil-variant pairing: the price enrichment (issue #209) and the catalog-listing fold.
//!
//! Some sets — Secret Lair especially — model a card's **foil** printing as a *separate*
//! Scryfall object whose collector number is the nonfoil's plus a star (U+2605): `sld` `741`
//! (nonfoil) and `741★` (foil). Scryfall keeps the foil price only on the `741★` object; the
//! nonfoil base `741` carries a nonfoil price and an **empty foil price**.
//!
//! The collection consolidates a foil-★ holding onto its nonfoil base as a foil copy (see
//! `crate::collection_import::consolidate`), and collection valuation prices a foil copy from
//! its card's `price_usd_foil` — which on the base is empty, so a folded foil would value at
//! $0. [`enrich_foil_variant_prices`] copies each foil-★ sibling's foil price onto its nonfoil
//! base so the base carries **both** prices, and the folded foil values correctly (and the
//! public catalog shows the base's foil price too). Runs on every sync tick, before the daily
//! price snapshot, so the enriched price is captured into the base card's history like any
//! other.
//!
//! Because the base then carries both prices, the star is a **duplicate tile** in every card
//! grid — "Secret Lair x Hatsune Miku: Sakura Superstar" listed `1587` and `1587★` side by
//! side, both showing the same $12.33 foil. [`not_folded_foil_variant`] is the predicate the
//! public catalog listings filter on so a folded star stays out of the grid (and out of a
//! drop's `card_count`), and [`has_folded_foil_variant`] is its mirror, so `is:foil` still
//! finds the base whose foil lives on a folded star. The star row itself is **kept**: its
//! Scryfall id is a live wire id (card detail, collection/wishlist/deck/alert rows, provider
//! imports all resolve it), so this is a presentation fold, never a delete.
//!
//! All three spellings in this module match the consolidation rule exactly (foil-only star ↔
//! nonfoil base, same set + oracle id + collector number sans the star), so a card whose foil
//! never folds is never touched — see [`SQL_STAR_IS_FOLDED`].

use chrono::Utc;
use sea_orm::sea_query::{Expr, SimpleExpr};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

use super::ingest::IngestError;

/// Copy every foil-★ sibling's `price_usd_foil` onto its nonfoil base card. Idempotent and
/// safe to run every tick: the nonfoil base never carries its own foil price, so overwriting
/// it with the current sibling price just keeps it fresh as prices move. Returns the number of
/// base rows updated (for logging).
///
/// **Star-driven** (issue-#209 follow-up perf, `m..044`): the `…★` foil stars are a tiny set
/// (~1,851 rows against ~40k nonfoil bases and ~106k cards), so the `UPDATE` starts from them —
/// found through the partial index `idx_cards_foil_variant_star` on `finishes = 'foil' AND
/// collector_number LIKE '%★'` — and joins each back to its base by **stripping the trailing
/// star** (`substr(..., 1, length(...) - 1)`; the star is a single char, so this is exact). The
/// earlier base-driven form re-derived the match with a correlated subquery per candidate, which
/// made the planner sequential-scan the whole wide `cards` heap twice (once for the ~40k nonfoil
/// bases, once to hash the ~12k foil rows) — ~8 s on the weak, cold prod Postgres. This form
/// scans only the ~1,851 stars and point-seeks each base, ~3.9× fewer buffers there, and is
/// verified to produce byte-identical results (0 mismatches over the 1,627 real pairs).
///
/// Cross-backend plain SQL: `UPDATE…FROM` self-join + `substr`/`length` + `LIKE` + `||` concat,
/// all of which render byte-identically on SQLite (≥ 3.33 for `UPDATE…FROM`; the shipping
/// `IS DISTINCT FROM` below already requires ≥ 3.39) and Postgres — no `db::Dialect` gate.
/// Game-agnostic (the star↔base join is same-game), so the star convention is handled for
/// whatever game has such pairs; today only MTG does.
pub(crate) async fn enrich_foil_variant_prices(
    db: &DatabaseConnection,
) -> Result<u64, IngestError> {
    // Stamp `updated_at` too, and only on the rows this actually changes (the `IS DISTINCT
    // FROM` guard). This is a **live price write**, so it must bump `updated_at` exactly like
    // the guarded card upsert does — the price-alert evaluator's change-narrowing
    // (`crate::alerts`) treats `cards.updated_at` as "a datum (incl. foil price) changed", and
    // for ★-variant Secret Lair cards the base's foil price arrives *only* through this path,
    // so an un-stamped write here would hide a foil-price crossing from the narrowed scan. The
    // `?` binds a chrono `Value` (the same encoding SeaORM stores `updated_at` with, so the
    // narrowing's `updated_at >= since` comparison lines up); the shared `Dialect::placeholders`
    // seam renumbers it to `$1` on Postgres.
    let backend = db.get_database_backend();
    let sql = crate::db::Dialect::from_backend(backend).placeholders(ENRICH_SQL);
    let stmt = Statement::from_sql_and_values(backend, sql, [Utc::now().into()]);
    let result = db.execute(stmt).await?;
    Ok(result.rows_affected())
}

const ENRICH_SQL: &str = r#"
UPDATE cards AS base
SET price_usd_foil = star.price_usd_foil, updated_at = ?
FROM cards AS star
WHERE star.finishes = 'foil'
  AND star.collector_number LIKE '%★'
  AND base.game = star.game
  AND base.set_code = star.set_code
  AND base.oracle_id = star.oracle_id
  AND base.finishes = 'nonfoil'
  AND base.collector_number = substr(star.collector_number, 1, length(star.collector_number) - 1)
  -- Only rewrite a base whose foil price would actually change. Without this, every matched base
  -- is re-written each tick even when the sibling price is unchanged, churning an MVCC tuple +
  -- indexes for nothing. `IS DISTINCT FROM` is null-safe and valid on both Postgres and SQLite
  -- (≥ 3.39).
  AND base.price_usd_foil IS DISTINCT FROM star.price_usd_foil"#;

// ---------- The catalog-listing fold ----------

/// The pairing rule as a **row predicate on `cards`**, in the star's direction: true iff this
/// row is a purely-foil `…★` printing whose purely-nonfoil base sibling exists — i.e. exactly
/// the rows [`ENRICH_SQL`] copies a foil price *out of*, and exactly the rows
/// `collection_import::consolidate` folds a holding *off*.
///
/// Deliberately the same conservative rule, spelled once here: a star whose base is itself
/// foilable (`nonfoil,foil`), an `etched` star, and a standalone `…★` promo with no base
/// sibling are all **not** folded — those are distinct printings, not a nonfoil card's foil
/// counterpart.
///
/// Correlates on the outer table by its real name, `cards` — it has to, or the inner alias
/// would shadow the outer row's columns. That holds because every consumer applies these
/// predicates either to a `Card::find()` (the catalog listings) or, for the `is:foil` mirror,
/// to a holdings query whose `find_also_related(Card)` renders an **unaliased** `LEFT JOIN
/// "cards"`; `handlers::collection::tests` pins the joined case. The `COALESCE(…, '')`
/// wrappers make the finish tests
/// total (`finishes` is nullable) so the negated form in [`not_folded_foil_variant`] can't go
/// three-valued and silently drop a NULL-finish row; they match no value a real star or base
/// carries, so the rule itself is unchanged.
///
/// Cross-backend plain SQL, like `ENRICH_SQL`: `EXISTS`, `LIKE`, `COALESCE`, `substr`/`length`
/// and `||` concat all render byte-identically on SQLite and Postgres, so there's no
/// `db::Dialect` gate and no bound value to renumber.
///
/// **Cost.** This is a residual per-row filter, never a driving one. The two leading tests are
/// column compares on a row the plan already has, and both backends short-circuit `AND`, so
/// the correlated `EXISTS` only runs for the ~1,851 `…★` rows in a ~106k-row catalog — and
/// when it does, it point-seeks the base through `idx_cards_game_set_code_collector_number`
/// (`m..024`). Selectivity runs ~98% *true*, so the negated form barely perturbs the listing's
/// `ORDER BY name, set_code, collector_number_int, id` + `LIMIT` estimate and can't tip it off
/// the sort index — the opposite of the *selective* leaf that cost `m..068` 86 s in prod. It
/// cannot be served *from* `idx_cards_foil_variant_star` (`m..044`): a partial index answers
/// its own predicate, never that predicate's negation.
///
/// Note the inner finish test is plain equality while the outer one is `COALESCE`d: inside an
/// `EXISTS` a NULL is already a non-match, so the wrapper would buy nothing and would only
/// stop the planner proving an index predicate (see [`SQL_BASE_HAS_FOLDED_STAR`]).
const SQL_STAR_IS_FOLDED: &str = "\
COALESCE(cards.finishes, '') = 'foil' \
AND cards.collector_number LIKE '%★' \
AND EXISTS (SELECT 1 FROM cards AS fv \
            WHERE fv.game = cards.game \
              AND fv.set_code = cards.set_code \
              AND fv.oracle_id = cards.oracle_id \
              AND fv.finishes = 'nonfoil' \
              AND fv.collector_number = \
                  substr(cards.collector_number, 1, length(cards.collector_number) - 1))";

/// The same pairing rule from the **base's** side: true iff this row is a purely-nonfoil
/// printing whose foil lives on a folded `…★` sibling. The mirror of [`SQL_STAR_IS_FOLDED`] —
/// a row satisfies one or the other, never both.
///
/// Re-appending the star (`|| '★'`) rather than stripping it keeps the probe an equality on
/// `collector_number`; gated on the base's own nonfoil-only finish so the sub-select never
/// runs for the foilable majority.
///
/// This is the more exposed of the two — `is:foil` ORs it in, and an `OR` can't short-circuit
/// past its second arm, so the sub-select runs for every nonfoil-only row (~40k) on both
/// halves of such a search. Hence the two apparently redundant conjuncts: spelling the star
/// test exactly as `idx_cards_foil_variant_star`'s partial predicate does (`m..044`:
/// `finishes = 'foil' AND collector_number LIKE '%★'`, un-`COALESCE`d so the implication is
/// provable) lets Postgres serve the probe from that 96 KB / ~1,851-entry index instead of
/// seeking `m..024` across the whole ~106k-row catalog. The `LIKE` is implied by the equality
/// on any real data; it's there for the planner, not the semantics.
const SQL_BASE_HAS_FOLDED_STAR: &str = "\
COALESCE(cards.finishes, '') = 'nonfoil' \
AND EXISTS (SELECT 1 FROM cards AS fv \
            WHERE fv.game = cards.game \
              AND fv.set_code = cards.set_code \
              AND fv.oracle_id = cards.oracle_id \
              AND fv.finishes = 'foil' \
              AND fv.collector_number LIKE '%★' \
              AND fv.collector_number = cards.collector_number || '★')";

/// This row **is** a foil-★ variant folded onto its nonfoil base ([`SQL_STAR_IS_FOLDED`]).
/// Exposed for the tests that pin the rule; listings want [`not_folded_foil_variant`].
#[cfg(test)]
pub(crate) fn folded_foil_variant() -> SimpleExpr {
    Expr::cust(format!("({SQL_STAR_IS_FOLDED})"))
}

/// The public catalog listings' fold: keep every row **except** a foil-★ variant folded onto
/// its nonfoil base. Applied by `handlers::catalog::catalog_cards`, the one base query every
/// card grid starts from, so a printing shows up once — with both its prices — instead of as
/// a base tile and a near-identical star tile.
pub(crate) fn not_folded_foil_variant() -> SimpleExpr {
    Expr::cust(format!("NOT ({SQL_STAR_IS_FOLDED})"))
}

/// This row is a nonfoil base whose foil printing is a folded `…★` sibling
/// ([`SQL_BASE_HAS_FOLDED_STAR`]) — so it *is* obtainable in foil, even though its own
/// `finishes` says `nonfoil`.
///
/// `is:foil` ORs this in: the star that used to answer that search is now folded out of the
/// grid, and rewriting the base's `finishes` instead is not an option — `nonfoil`-exactly is
/// the load-bearing half of the pairing rule in all three of its homes (`ENRICH_SQL`,
/// `collection_import::consolidate`, and the `m..023` migration), so widening it would stop
/// the very enrichment and consolidation that make the fold correct.
pub(crate) fn has_folded_foil_variant() -> SimpleExpr {
    Expr::cust(format!("({SQL_BASE_HAS_FOLDED_STAR})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::card;
    use crate::test_support::{card_model, migrated_memory_db};
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter};

    #[allow(clippy::too_many_arguments)]
    async fn insert_in_set(
        db: &DatabaseConnection,
        id: i32,
        set_code: &str,
        collector_number: &str,
        finishes: &str,
        oracle_id: &str,
        usd: Option<&str>,
        usd_foil: Option<&str>,
    ) {
        card::Model {
            external_id: format!("ext-{id}"),
            set_code: set_code.into(),
            collector_number: collector_number.into(),
            finishes: Some(finishes.into()),
            oracle_id: Some(oracle_id.into()),
            price_usd: usd.map(str::to_string),
            price_usd_foil: usd_foil.map(str::to_string),
            ..card_model(id)
        }
        .into_active_model()
        .insert(db)
        .await
        .expect("insert card");
    }

    async fn insert(
        db: &DatabaseConnection,
        id: i32,
        collector_number: &str,
        finishes: &str,
        oracle_id: &str,
        usd: Option<&str>,
        usd_foil: Option<&str>,
    ) {
        insert_in_set(
            db,
            id,
            "sld",
            collector_number,
            finishes,
            oracle_id,
            usd,
            usd_foil,
        )
        .await;
    }

    async fn fetch(db: &DatabaseConnection, external_id: &str) -> card::Model {
        card::Entity::find()
            .filter(card::Column::ExternalId.eq(external_id))
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    async fn foil_price(db: &DatabaseConnection, external_id: &str) -> Option<String> {
        fetch(db, external_id).await.price_usd_foil
    }

    #[tokio::test]
    async fn enriches_a_nonfoil_base_from_its_foil_star_sibling() {
        let db = migrated_memory_db().await;
        // The issue's case: base 741 (nonfoil, no foil price) + star 741★ (foil, $29.39).
        insert(&db, 1, "741", "nonfoil", "ora-chaos", Some("26.75"), None).await;
        insert(&db, 2, "741★", "foil", "ora-chaos", None, Some("29.39")).await;
        // An ambiguous base (itself foilable) and its star -> NOT enriched (rule needs a
        // strictly-nonfoil base).
        insert(
            &db,
            3,
            "33",
            "nonfoil,foil",
            "ora-proctor",
            Some("1.00"),
            Some("2.00"),
        )
        .await;
        insert(&db, 4, "33★", "foil", "ora-proctor", None, Some("9.99")).await;
        // A plain card with no star sibling -> untouched.
        insert(&db, 5, "100", "nonfoil", "ora-plain", Some("5.00"), None).await;
        // An alphanumeric collector number -> the star-strip (`substr(..., length - 1)`) must
        // drop only the trailing ★, matching "W3a" not a numeric prefix.
        insert(&db, 6, "W3a", "nonfoil", "ora-alpha", Some("4.00"), None).await;
        insert(&db, 7, "W3a★", "foil", "ora-alpha", None, Some("3.33")).await;

        let n = enrich_foil_variant_prices(&db).await.expect("enrich");

        assert_eq!(n, 2, "only the clean nonfoil bases are enriched");
        assert_eq!(
            foil_price(&db, "ext-1").await.as_deref(),
            Some("29.39"),
            "base gets star foil price"
        );
        assert_eq!(
            foil_price(&db, "ext-2").await.as_deref(),
            Some("29.39"),
            "star unchanged"
        );
        assert_eq!(
            foil_price(&db, "ext-3").await.as_deref(),
            Some("2.00"),
            "ambiguous base kept its own"
        );
        assert_eq!(
            foil_price(&db, "ext-5").await,
            None,
            "no-sibling card untouched"
        );
        assert_eq!(
            foil_price(&db, "ext-6").await.as_deref(),
            Some("3.33"),
            "alphanumeric base gets star foil price"
        );

        // A live foil-price write must bump `updated_at` (the alert evaluator's change-narrowing
        // depends on it) — the seeded rows carry a fixed 2024-01-01 stamp, so an enriched base is
        // now-stamped while an untouched card keeps the old stamp.
        let seeded: sea_orm::prelude::DateTimeUtc = "2024-01-02T00:00:00Z".parse().unwrap();
        assert!(
            fetch(&db, "ext-1").await.updated_at > seeded,
            "enriched base is re-stamped"
        );
        assert!(
            fetch(&db, "ext-5").await.updated_at < seeded,
            "untouched card keeps its stamp"
        );
    }

    #[tokio::test]
    async fn enrichment_refreshes_a_stale_base_price_and_is_idempotent() {
        let db = migrated_memory_db().await;
        insert(&db, 1, "741", "nonfoil", "ora-chaos", Some("26.75"), None).await;
        insert(&db, 2, "741★", "foil", "ora-chaos", None, Some("29.39")).await;

        enrich_foil_variant_prices(&db).await.expect("enrich once");
        assert_eq!(foil_price(&db, "ext-1").await.as_deref(), Some("29.39"));
        // Re-running is a no-op (same value): the guard skips the write, so zero rows are
        // touched; a later price move re-copies the new value.
        let reran = enrich_foil_variant_prices(&db).await.expect("enrich again");
        assert_eq!(reran, 0, "unchanged base is not rewritten");
        assert_eq!(foil_price(&db, "ext-1").await.as_deref(), Some("29.39"));

        let star = card::Entity::find()
            .filter(card::Column::ExternalId.eq("ext-2"))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let mut star = star.into_active_model();
        star.price_usd_foil = sea_orm::Set(Some("31.00".into()));
        star.update(&db).await.expect("bump star price");

        enrich_foil_variant_prices(&db).await.expect("re-enrich");
        assert_eq!(
            foil_price(&db, "ext-1").await.as_deref(),
            Some("31.00"),
            "base tracks the new star price"
        );
    }

    /// Every row a card grid could show, run through the two listing predicates. The set is
    /// the same rule matrix `enrich_foil_variant_prices` obeys, so what the enrichment copies
    /// a foil price *out of* is exactly what a listing hides, and exactly whose base answers
    /// `is:foil` — the three spellings can't drift apart without failing here.
    async fn matching(db: &DatabaseConnection, expr: SimpleExpr) -> Vec<String> {
        use sea_orm::QueryOrder;
        let mut ids: Vec<String> = card::Entity::find()
            .filter(expr)
            .order_by_asc(card::Column::Id)
            .all(db)
            .await
            .expect("filter cards")
            .into_iter()
            .map(|c| c.external_id)
            .collect();
        ids.sort();
        ids
    }

    #[tokio::test]
    async fn the_listing_fold_hides_only_a_star_folded_onto_a_nonfoil_base() {
        let db = migrated_memory_db().await;
        // The issue's case: a nonfoil base and the foil-only star Scryfall models separately.
        insert(&db, 1, "1587", "nonfoil", "ora-shelter", Some("6.26"), None).await;
        insert(&db, 2, "1587★", "foil", "ora-shelter", None, Some("12.33")).await;
        // An ambiguous base — foilable in its own right — and its star: two real printings,
        // so neither folds (matching the enrichment, which won't copy onto such a base).
        insert(
            &db,
            3,
            "33",
            "nonfoil,foil",
            "ora-proctor",
            Some("1.00"),
            Some("2.00"),
        )
        .await;
        insert(&db, 4, "33★", "foil", "ora-proctor", None, Some("9.99")).await;
        // A standalone `★` promo with no base sibling (the dummy catalog ships one).
        insert(&db, 5, "★", "foil", "ora-promo", None, Some("4.00")).await;
        // An etched star: a distinct finish, never a nonfoil card's foil counterpart.
        insert(&db, 6, "900", "nonfoil", "ora-etched", Some("1.00"), None).await;
        insert(&db, 7, "900★", "etched", "ora-etched", None, Some("8.00")).await;
        // A star whose only same-numbered nonfoil card lives in another set.
        insert_in_set(
            &db,
            8,
            "sld",
            "77★",
            "foil",
            "ora-split",
            None,
            Some("3.00"),
        )
        .await;
        insert_in_set(
            &db,
            9,
            "who",
            "77",
            "nonfoil",
            "ora-split",
            Some("2.00"),
            None,
        )
        .await;
        // A star sharing its base's number but not its gameplay identity.
        insert(&db, 10, "500", "nonfoil", "ora-a", Some("1.00"), None).await;
        insert(&db, 11, "500★", "foil", "ora-b", None, Some("7.00")).await;
        // A plain card with no star at all.
        insert(&db, 12, "100", "nonfoil", "ora-plain", Some("5.00"), None).await;

        assert_eq!(
            matching(&db, folded_foil_variant()).await,
            vec!["ext-2"],
            "only the star with a purely-nonfoil base of the same identity folds"
        );
        assert_eq!(
            matching(&db, not_folded_foil_variant()).await,
            vec![
                "ext-1", "ext-10", "ext-11", "ext-12", "ext-3", "ext-4", "ext-5", "ext-6", "ext-7",
                "ext-8", "ext-9"
            ],
            "every other row still lists, including the orphan/etched/cross-set stars"
        );
        assert_eq!(
            matching(&db, has_folded_foil_variant()).await,
            vec!["ext-1"],
            "only the base that lost its star is foil-available without saying so"
        );
    }

    /// A `finishes`-less row (the dummy seeder never sets the column, and Scryfall's field is
    /// optional) must survive the *negated* predicate rather than vanishing into SQL's
    /// three-valued logic — the reason both finish tests are `COALESCE`d.
    #[tokio::test]
    async fn a_null_finish_row_is_never_folded_away() {
        let db = migrated_memory_db().await;
        card::Model {
            external_id: "ext-null".into(),
            set_code: "sld".into(),
            collector_number: "42★".into(),
            oracle_id: Some("ora-null".into()),
            ..card_model(1)
        }
        .into_active_model()
        .insert(&db)
        .await
        .expect("insert card");
        insert(&db, 2, "42", "nonfoil", "ora-null", Some("1.00"), None).await;

        assert_eq!(
            matching(&db, not_folded_foil_variant()).await,
            vec!["ext-2", "ext-null"],
            "a row with no finishes is listed, not silently dropped"
        );
        assert!(
            matching(&db, has_folded_foil_variant()).await.is_empty(),
            "and its base is not claimed to be foil-available"
        );
    }
}

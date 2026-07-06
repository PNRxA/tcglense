//! Foil-variant price enrichment (issue #209).
//!
//! Some sets — Secret Lair especially — model a card's **foil** printing as a *separate*
//! Scryfall object whose collector number is the nonfoil's plus a star (U+2605): `sld` `741`
//! (nonfoil) and `741★` (foil). Scryfall keeps the foil price only on the `741★` object; the
//! nonfoil base `741` carries a nonfoil price and an **empty foil price**.
//!
//! The collection consolidates a foil-★ holding onto its nonfoil base as a foil copy (see
//! `crate::collection_import::consolidate`), and collection valuation prices a foil copy from
//! its card's `price_usd_foil` — which on the base is empty, so a folded foil would value at
//! $0. This copies each foil-★ sibling's foil price onto its nonfoil base so the base carries
//! **both** prices, and the folded foil values correctly (and the public catalog shows the
//! base's foil price too). Runs on every sync tick, before the daily price snapshot, so the
//! enriched price is captured into the base card's history like any other.
//!
//! Matches the consolidation rule exactly (foil-only star ↔ nonfoil base, same set + oracle id
//! + collector number sans the star), so a card whose foil never folds is never touched.

use sea_orm::{ConnectionTrait, DatabaseConnection};

use super::ingest::IngestError;

/// Copy every foil-★ sibling's `price_usd_foil` onto its nonfoil base card. Idempotent and
/// safe to run every tick: the nonfoil base never carries its own foil price, so overwriting
/// it with the current sibling price just keeps it fresh as prices move. Returns the number of
/// base rows updated (for logging).
///
/// Cross-backend plain SQL: correlated scalar subquery + `||` concat + `LIKE`-free `= x || '★'`
/// match, no `UPDATE…FROM` — so it runs byte-identically on SQLite and Postgres. Game-agnostic
/// (the star↔base join is same-game), so the star convention is handled for whatever game has
/// such pairs; today only MTG does.
pub(crate) async fn enrich_foil_variant_prices(
    db: &DatabaseConnection,
) -> Result<u64, IngestError> {
    let result = db.execute_unprepared(ENRICH_SQL).await?;
    Ok(result.rows_affected())
}

const ENRICH_SQL: &str = r#"
UPDATE cards
SET price_usd_foil = (
        SELECT sc.price_usd_foil
        FROM cards sc
        WHERE sc.game = cards.game
          AND sc.set_code = cards.set_code
          AND sc.oracle_id = cards.oracle_id
          AND sc.finishes = 'foil'
          AND sc.collector_number = cards.collector_number || '★'
        LIMIT 1
    )
WHERE cards.id IN (
        SELECT b.id
        FROM cards b
        JOIN cards s ON s.game = b.game
                    AND s.set_code = b.set_code
                    AND s.oracle_id = b.oracle_id
                    AND s.finishes = 'foil'
                    AND s.collector_number = b.collector_number || '★'
        WHERE b.finishes = 'nonfoil'
    )
  -- Only rewrite a base whose foil price would actually change. Without this, every
  -- matched base is re-written each tick even when the sibling price is unchanged,
  -- churning an MVCC tuple + indexes for nothing. `IS DISTINCT FROM` is null-safe and
  -- valid on both Postgres and SQLite (≥ 3.39).
  AND cards.price_usd_foil IS DISTINCT FROM (
        SELECT sc.price_usd_foil
        FROM cards sc
        WHERE sc.game = cards.game
          AND sc.set_code = cards.set_code
          AND sc.oracle_id = cards.oracle_id
          AND sc.finishes = 'foil'
          AND sc.collector_number = cards.collector_number || '★'
        LIMIT 1
    )"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::card;
    use crate::test_support::{card_model, migrated_memory_db};
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter};

    async fn insert(
        db: &DatabaseConnection,
        id: i32,
        collector_number: &str,
        finishes: &str,
        oracle_id: &str,
        usd: Option<&str>,
        usd_foil: Option<&str>,
    ) {
        card::Model {
            external_id: format!("ext-{id}"),
            set_code: "sld".into(),
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

    async fn foil_price(db: &DatabaseConnection, external_id: &str) -> Option<String> {
        card::Entity::find()
            .filter(card::Column::ExternalId.eq(external_id))
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .price_usd_foil
    }

    #[tokio::test]
    async fn enriches_a_nonfoil_base_from_its_foil_star_sibling() {
        let db = migrated_memory_db().await;
        // The issue's case: base 741 (nonfoil, no foil price) + star 741★ (foil, $29.39).
        insert(&db, 1, "741", "nonfoil", "ora-chaos", Some("26.75"), None).await;
        insert(&db, 2, "741★", "foil", "ora-chaos", None, Some("29.39")).await;
        // An ambiguous base (itself foilable) and its star -> NOT enriched (rule needs a
        // strictly-nonfoil base).
        insert(&db, 3, "33", "nonfoil,foil", "ora-proctor", Some("1.00"), Some("2.00")).await;
        insert(&db, 4, "33★", "foil", "ora-proctor", None, Some("9.99")).await;
        // A plain card with no star sibling -> untouched.
        insert(&db, 5, "100", "nonfoil", "ora-plain", Some("5.00"), None).await;

        let n = enrich_foil_variant_prices(&db).await.expect("enrich");

        assert_eq!(n, 1, "only the clean nonfoil base is enriched");
        assert_eq!(foil_price(&db, "ext-1").await.as_deref(), Some("29.39"), "base gets star foil price");
        assert_eq!(foil_price(&db, "ext-2").await.as_deref(), Some("29.39"), "star unchanged");
        assert_eq!(foil_price(&db, "ext-3").await.as_deref(), Some("2.00"), "ambiguous base kept its own");
        assert_eq!(foil_price(&db, "ext-5").await, None, "no-sibling card untouched");
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
        assert_eq!(foil_price(&db, "ext-1").await.as_deref(), Some("31.00"), "base tracks the new star price");
    }
}

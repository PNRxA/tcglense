//! Secret Lair per-product **release-date derivation**, from each product's own contents.
//!
//! TCGCSV files *every* Secret Lair drop under one group ("Secret Lair", abbreviation `SLD`)
//! with a single, **rolling** group-level `publishedOn` (it advances as new drops are added).
//! So the group date is the same for every drop and drifts to ~now — useless as a per-drop
//! release date, and actively harmful: stamping it on every SLD product makes ~half the
//! catalog share one near-today date and clump at the top of the newest-first sort. Each drop
//! actually released on its own date, and that date lives on the drop's *cards*
//! ([`crate::scryfall`] gives every `sld` printing a per-card `released_at`).
//!
//! Rather than re-resolve a product to a Scryfall gallery drop by name (brittle — storefront
//! names diverge from gallery titles: an artist prefix, a year suffix, a `(WPN Exclusive)`
//! clause, bundles/decks/promos that are no single drop), this reads the cards the product is
//! *already known to contain*: its `sealed_contents` `contains` rows (populated by
//! [`crate::mtgjson::ingest`]). A product's date is the **modal** `released_at` among those
//! cards (ties → earliest), via the shared [`drops::modal_release_date`] — the same reducer the
//! card by-drop view ([`crate::handlers::catalog::sets`]'s `drop_released_at`) uses, so the two
//! surfaces derive the identical date. A drop's cards share one street date, so the mode *is*
//! that date and shrugs off a stray reprint.
//!
//! A product with **no dated contents** (a content-less bundle, say) is left `NULL` — honest
//! ("we don't know its date") and, with the products list's `NULLS LAST`, parked at the bottom
//! of the newest-first order rather than masquerading as new. That is strictly better than the
//! rolling group date it used to inherit.
//!
//! [`restamp_from_contents`] runs on every sync tick in [`crate::catalog::refresh_all`], **after**
//! the contents sync, so the rows exist to read — and unconditionally, so a card-date change or
//! a newly-ingested product re-stamps even when the version-gated syncs were skipped. It owns
//! the SLD date end to end: [`crate::tcgcsv::ingest`] deliberately leaves SLD products undated
//! (every other set has one group per set, where the group date *is* the release).

use std::collections::HashMap;

use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QuerySelect, sea_query::Expr,
};

use crate::entities::prelude::{Card, Product, SealedContent};
use crate::entities::sealed_content::Membership;
use crate::entities::{card, product, sealed_content};
use crate::mtgjson::sld;
use crate::scryfall::{GAME, drops};

/// SQLite caps host parameters per statement (as few as 999 on old builds), so the
/// by-id lookups/updates are chunked well under the bind limit — a giant SLD festival bundle
/// can reference thousands of cards.
const IN_CHUNK: usize = 900;

/// Recompute every Secret Lair (`sld`) product's `released_at` from the cards it **contains**,
/// updating only the rows whose date actually changes. Returns how many product rows changed.
///
/// A product's date is the modal `released_at` among its `contains` cards (see the module docs);
/// a product with no dated contents is set to `NULL`. Reads three small, indexed slices (the
/// `sld` products, their `contains` membership rows, and the referenced cards' dates), reduces in
/// memory, and writes back grouped by target date, so the whole pass is a handful of statements.
///
/// `updated_at` is intentionally **not** bumped: `released_at` isn't a price column, so leaving it
/// keeps the alert evaluator's change-narrowing from re-scanning these rows for no reason.
pub async fn restamp_from_contents(db: &DatabaseConnection) -> Result<u64, DbErr> {
    // The SLD products and their current date (to skip no-op writes).
    let products: Vec<(i32, Option<String>)> = Product::find()
        .select_only()
        .column(product::Column::Id)
        .column(product::Column::ReleasedAt)
        .filter(product::Column::Game.eq(GAME))
        .filter(product::Column::SetCode.eq(sld::SET_CODE))
        .into_tuple()
        .all(db)
        .await?;
    if products.is_empty() {
        return Ok(0);
    }
    let product_ids: Vec<i32> = products.iter().map(|(id, _)| *id).collect();

    // Their guaranteed cards: `(product_id, card_id)` for the `contains` membership only (a
    // drop's own cards define its date — the random bonus pool is `variable` and excluded).
    let mut contains: Vec<(i32, i32)> = Vec::new();
    for chunk in product_ids.chunks(IN_CHUNK) {
        let rows: Vec<(i32, i32)> = SealedContent::find()
            .select_only()
            .column(sealed_content::Column::ProductId)
            .column(sealed_content::Column::CardId)
            .filter(sealed_content::Column::Game.eq(GAME))
            .filter(sealed_content::Column::Membership.eq(Membership::Contains.as_str()))
            .filter(sealed_content::Column::ProductId.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(db)
            .await?;
        contains.extend(rows);
    }

    // The referenced cards' release dates, keyed by internal id (dateless cards dropped).
    let mut card_ids: Vec<i32> = contains.iter().map(|(_, cid)| *cid).collect();
    card_ids.sort_unstable();
    card_ids.dedup();
    let mut card_dates: HashMap<i32, String> = HashMap::new();
    for chunk in card_ids.chunks(IN_CHUNK) {
        let rows: Vec<(i32, Option<String>)> = Card::find()
            .select_only()
            .column(card::Column::Id)
            .column(card::Column::ReleasedAt)
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(db)
            .await?;
        for (id, date) in rows {
            if let Some(date) = date {
                card_dates.insert(id, date);
            }
        }
    }

    // Reduce each product's contained-card dates to its modal date (pure).
    let modal = modal_date_by_product(&contains, &card_dates);

    // Write back only what changed, grouped by target date (+ a `NULL` group for the
    // content-less products) so distinct dates share one `UPDATE … WHERE id IN (…)`.
    let mut by_target: HashMap<Option<String>, Vec<i32>> = HashMap::new();
    for (id, current) in &products {
        let target = modal.get(id).cloned();
        if target.as_deref() != current.as_deref() {
            by_target.entry(target).or_default().push(*id);
        }
    }

    let mut changed: u64 = 0;
    for (target, ids) in by_target {
        for chunk in ids.chunks(IN_CHUNK) {
            let result = Product::update_many()
                .col_expr(product::Column::ReleasedAt, Expr::value(target.clone()))
                .filter(product::Column::Id.is_in(chunk.iter().copied()))
                .exec(db)
                .await?;
            changed += result.rows_affected;
        }
    }
    Ok(changed)
}

/// Group `contains` `(product_id, card_id)` rows by product and reduce each product's
/// contained-card release dates (looked up in `card_dates`) to their modal date. A product with
/// no dated contained card is **absent** from the result (the caller reads that as `NULL`).
/// Reuses [`drops::modal_release_date`] so the mode + earliest-on-tie rule matches the card
/// by-drop view.
fn modal_date_by_product(
    contains: &[(i32, i32)],
    card_dates: &HashMap<i32, String>,
) -> HashMap<i32, String> {
    let mut by_product: HashMap<i32, Vec<&str>> = HashMap::new();
    for (product_id, card_id) in contains {
        if let Some(date) = card_dates.get(card_id) {
            by_product.entry(*product_id).or_default().push(date);
        }
    }
    by_product
        .into_iter()
        .filter_map(|(product_id, dates)| {
            drops::modal_release_date(dates.into_iter()).map(|date| (product_id, date))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{card, sealed_content};
    use crate::test_support::{insert_product, migrated_memory_db};
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};

    #[test]
    fn modal_date_by_product_reduces_each_products_contents() {
        let card_dates = HashMap::from([
            (10, "2024-05-03".to_string()),
            (11, "2024-05-03".to_string()),
            // A stray reprint carrying an earlier, less-common date: the mode must beat it.
            (12, "2019-01-01".to_string()),
            (20, "2020-02-18".to_string()),
            // card 99 has no date entry -> contributes nothing.
        ]);
        let contains = vec![
            (100, 10),
            (100, 11),
            (100, 12),
            (100, 99),
            (200, 20),
            // Product 300's only contained card is dateless -> it must be absent (→ NULL).
            (300, 99),
        ];
        let modal = modal_date_by_product(&contains, &card_dates);
        assert_eq!(modal.get(&100).map(String::as_str), Some("2024-05-03"));
        assert_eq!(modal.get(&200).map(String::as_str), Some("2020-02-18"));
        assert!(!modal.contains_key(&300), "no dated contents -> absent");
    }

    /// Insert an `sld` card carrying a release date; returns its internal id.
    async fn insert_sld_card(
        db: &DatabaseConnection,
        external_id: &str,
        collector_number: &str,
        released_at: Option<&str>,
    ) -> i32 {
        let now = Utc::now();
        card::ActiveModel {
            game: Set(GAME.to_string()),
            external_id: Set(external_id.to_string()),
            name: Set(format!("Card {external_id}")),
            set_code: Set(sld::SET_CODE.to_string()),
            set_name: Set("Secret Lair".to_string()),
            collector_number: Set(collector_number.to_string()),
            lang: Set("en".to_string()),
            digital: Set(false),
            released_at: Set(released_at.map(str::to_string)),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert sld card")
        .id
    }

    /// Record a `contains` membership row linking a product to a card.
    async fn insert_contains(db: &DatabaseConnection, product_id: i32, card_id: i32) {
        let now = Utc::now();
        sealed_content::ActiveModel {
            game: Set(GAME.to_string()),
            product_id: Set(product_id),
            card_id: Set(card_id),
            membership: Set(Membership::Contains.as_str().to_string()),
            foil: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert sealed content");
    }

    async fn released_at(db: &DatabaseConnection, id: i32) -> Option<String> {
        Product::find_by_id(id)
            .one(db)
            .await
            .expect("query product")
            .expect("product exists")
            .released_at
    }

    #[tokio::test]
    async fn restamps_sld_dates_from_contents_and_leaves_others_alone() {
        let db = migrated_memory_db().await;

        // A resolvable drop product: seeded with the (wrong) rolling group placeholder date that
        // `insert_product` stamps, plus three contained cards — two share a street date, one is an
        // earlier reprint outlier. The modal (2024-05-03) must win.
        let drop = insert_product(
            &db,
            "700795",
            "Secret Lair Drop: Cats of Chaos - Non-Foil Edition",
            "sld",
            "secret_lair",
            None,
        )
        .await;
        let c1 = insert_sld_card(&db, "sld-c1", "2690", Some("2024-05-03")).await;
        let c2 = insert_sld_card(&db, "sld-c2", "2691", Some("2024-05-03")).await;
        let c3 = insert_sld_card(&db, "sld-c3", "2692", Some("2019-01-01")).await;
        for cid in [c1, c2, c3] {
            insert_contains(&db, drop, cid).await;
        }

        // A content-less SLD product (a bundle): no `contains` rows, so its date must be cleared to
        // NULL rather than keep the rolling group placeholder.
        let bundle = insert_product(
            &db,
            "700802",
            "Secret Lair Superdrop: Everything Bundle",
            "sld",
            "secret_lair",
            None,
        )
        .await;

        // A non-SLD product must never be touched — its (real) group date stands.
        let other = insert_product(
            &db,
            "100",
            "Murders at Karlov Manor Collector Booster Box",
            "mkm",
            "collector_display",
            Some("199.99"),
        )
        .await;

        let changed = restamp_from_contents(&db).await.expect("restamp");
        assert_eq!(changed, 2, "only the two SLD rows change");
        assert_eq!(released_at(&db, drop).await.as_deref(), Some("2024-05-03"));
        assert_eq!(released_at(&db, bundle).await, None);
        assert_eq!(released_at(&db, other).await.as_deref(), Some("2024-02-09"));

        // Idempotent: a second pass over unchanged data writes nothing.
        let again = restamp_from_contents(&db).await.expect("restamp again");
        assert_eq!(again, 0, "no-op on a second pass");
    }
}

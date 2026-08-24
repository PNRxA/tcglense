//! Preconstructed-deck **value derivation**: fold each precon's card prices into
//! `precon_decks.price_cents`, so the browse can show and sort by what a decklist is worth.
//!
//! Derived once per sync tick rather than at read time or at ingest, for two reasons that
//! pull in opposite directions and meet here:
//!
//! * The precon list is a public, CDN-cacheable read that must not pay a per-row card scan
//!   (the stance every stored facet on `precon_decks` takes — `card_count`,
//!   `color_identity`, `face_card_id`), so the value has to be a **column**, not a read-time
//!   aggregate — and a plain integer column is also what lets `sort=price` be an ordinary
//!   `ORDER BY … NULLS LAST` instead of a dialect-guarded string cast.
//! * Card prices move on **every** sync tick, while the precon tables are rebuilt only when
//!   MTGJSON's ETag changes — so a value folded at rebuild would go stale for weeks. Hence
//!   the `cards.folded_onto_id` model (`m..076`): the rebuild writes `NULL` and
//!   [`refresh_precon_values`] recomputes the column from the live card prices each tick, in
//!   [`crate::catalog::refresh_all`] right after the sealed-contents sync (so a fresh
//!   rebuild's rows are priced in the same pass) — and once at boot on the no-sync path
//!   (`tasks::spawn_derived_price_passes`), so `m..077` needs no backfill.
//!
//! The fold itself must agree with the precon **detail** page's `summary.total_value_usd`
//! (`handlers::precons::read::get_precon`), or the tile and the page it opens would name two
//! different values for one deck. Both therefore reduce through the shared
//! [`Valuation`](crate::handlers::shared::valuation::Valuation) with the same row semantics:
//! deck proper only (the sideboard is summarised apart, exactly as `card_count` counts), a
//! row's single finish priced at `usd` or `usd_foil` by its `foil` flag (the
//! `HoldingCounts` impl for a precon card row), cards whose catalog row is gone skipped
//! (inner join), and "nothing priced" kept distinct from "$0.00" (`NULL`, never `0`).

use std::collections::HashMap;

use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QuerySelect, sea_query::Expr,
};

use crate::entities::precon_deck_card::PreconBoard;
use crate::entities::prelude::{PreconDeck, PreconDeckCard};
use crate::entities::{card, precon_deck, precon_deck_card};
use crate::handlers::shared::valuation::Valuation;

/// SQLite caps host parameters per statement (as few as 999 on old builds), so the by-id
/// scans/updates are chunked well under the bind limit.
const IN_CHUNK: usize = 900;

/// Recompute every precon deck's `price_cents` from the live card prices, updating only the
/// rows whose value actually changes. Returns how many deck rows changed.
///
/// One chunked scan of the deck-proper membership rows joined to their cards' two price
/// columns, a per-deck [`Valuation`] fold in memory, and write-backs grouped by target value
/// so the steady state (few prices moved) is a handful of statements. `updated_at` is
/// intentionally **not** bumped: nothing keys on a precon row's freshness, and the column
/// should keep meaning "when the deck was last rebuilt".
pub async fn refresh_precon_values(db: &DatabaseConnection) -> Result<u64, DbErr> {
    // Every deck's current value (to skip no-op writes). A deck absent from the fold below
    // (all its cards gone, or all of them sideboard) resolves to NULL, never a stale number.
    let decks: Vec<(i32, Option<i64>)> = PreconDeck::find()
        .select_only()
        .column(precon_deck::Column::Id)
        .column(precon_deck::Column::PriceCents)
        .into_tuple()
        .all(db)
        .await?;
    if decks.is_empty() {
        return Ok(0);
    }
    let deck_ids: Vec<i32> = decks.iter().map(|(id, _)| *id).collect();

    // The deck-proper rows (the sideboard never values the deck, as it never counts in
    // `card_count`), inner-joined to `cards` so an orphaned row is skipped, selecting only
    // what the fold reads rather than two full models per row.
    let mut folds: HashMap<i32, Valuation> = HashMap::new();
    for chunk in deck_ids.chunks(IN_CHUNK) {
        let rows: Vec<(i32, i32, bool, Option<String>, Option<String>)> = PreconDeckCard::find()
            .select_only()
            .column(precon_deck_card::Column::PreconDeckId)
            .column(precon_deck_card::Column::Quantity)
            .column(precon_deck_card::Column::Foil)
            .column(card::Column::PriceUsd)
            .column(card::Column::PriceUsdFoil)
            .inner_join(crate::entities::prelude::Card)
            .filter(precon_deck_card::Column::PreconDeckId.is_in(chunk.iter().copied()))
            .filter(precon_deck_card::Column::Board.ne(PreconBoard::Side.as_str()))
            .into_tuple()
            .all(db)
            .await?;
        for (deck_id, quantity, foil, usd, usd_foil) in rows {
            // A precon row is a single finish: its copies land in whichever count bucket
            // `foil` selects — the exact shape `HoldingCounts for precon_deck_card::Model`
            // gives the detail page's summary fold. Both *prices* are always passed, zero
            // quantity and all, because that is what `summarize_holdings` does and
            // `Valuation::add_finish` flips `any_priced` on any parseable price regardless
            // of count: substituting `None` for the other finish here would store NULL for
            // a deck the detail page calls "$0.00" (a foil-only row of a card priced only
            // as regular), splitting the tile from the page on the null-vs-zero line.
            folds.entry(deck_id).or_default().add(
                usd.as_deref(),
                if foil { 0 } else { quantity },
                usd_foil.as_deref(),
                if foil { quantity } else { 0 },
            );
        }
    }

    // Write back only what changed, grouped by target value (+ a NULL group) so decks that
    // moved to the same value share one `UPDATE … WHERE id IN (…)`.
    let mut by_target: HashMap<Option<i64>, Vec<i32>> = HashMap::new();
    for (id, current) in &decks {
        let target = folds.get(id).and_then(value_cents);
        if target != *current {
            by_target.entry(target).or_default().push(*id);
        }
    }

    let mut changed: u64 = 0;
    for (target, ids) in by_target {
        for chunk in ids.chunks(IN_CHUNK) {
            let result = PreconDeck::update_many()
                .col_expr(precon_deck::Column::PriceCents, Expr::value(target))
                .filter(precon_deck::Column::Id.is_in(chunk.iter().copied()))
                .exec(db)
                .await?;
            changed += result.rows_affected;
        }
    }
    Ok(changed)
}

/// A fold's storable value: its total in cents, or `None` when nothing was priced — the
/// same "an unpriced deck answers null, never $0.00" rule `Valuation::total_usd` renders on
/// the wire. Saturates into the column's `i64` (the fold is `i128`) rather than wrapping,
/// though no real decklist approaches either bound.
fn value_cents(valuation: &Valuation) -> Option<i64> {
    valuation
        .any_priced
        .then(|| i64::try_from(valuation.cents).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::migrated_memory_db;
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};

    /// Insert a card with the two prices; returns its internal id.
    async fn insert_card(
        db: &DatabaseConnection,
        ext: &str,
        usd: Option<&str>,
        usd_foil: Option<&str>,
    ) -> i32 {
        let now = Utc::now();
        card::ActiveModel {
            game: Set("mtg".to_string()),
            external_id: Set(ext.to_string()),
            name: Set(format!("Card {ext}")),
            set_code: Set("tmc".to_string()),
            set_name: Set("TMC".to_string()),
            collector_number: Set("1".to_string()),
            lang: Set("en".to_string()),
            digital: Set(false),
            price_usd: Set(usd.map(str::to_string)),
            price_usd_foil: Set(usd_foil.map(str::to_string)),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert card")
        .id
    }

    /// Insert a precon deck shell (value unset, as the rebuild leaves it); returns its id.
    async fn insert_deck(db: &DatabaseConnection, slug: &str) -> i32 {
        let now = Utc::now();
        precon_deck::ActiveModel {
            game: Set("mtg".to_string()),
            slug: Set(slug.to_string()),
            name: Set(slug.to_string()),
            set_code: Set("tmc".to_string()),
            deck_type: Set("Commander Deck".to_string()),
            released_at: Set(None),
            color_identity: Set(None),
            card_count: Set(0),
            sideboard_count: Set(0),
            face_card_id: Set(None),
            product_id: Set(None),
            price_cents: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert precon deck")
        .id
    }

    async fn insert_row(
        db: &DatabaseConnection,
        deck_id: i32,
        card_id: i32,
        board: PreconBoard,
        quantity: i32,
        foil: bool,
    ) {
        precon_deck_card::ActiveModel {
            precon_deck_id: Set(deck_id),
            card_id: Set(card_id),
            board: Set(board.as_str().to_string()),
            quantity: Set(quantity),
            foil: Set(foil),
            position: Set(0),
            ..Default::default()
        }
        .insert(db)
        .await
        .expect("insert precon card row");
    }

    async fn stored(db: &DatabaseConnection, id: i32) -> Option<i64> {
        PreconDeck::find_by_id(id)
            .one(db)
            .await
            .expect("query deck")
            .expect("deck exists")
            .price_cents
    }

    /// The fold prices each row's own finish, never counts the sideboard, keeps an unpriced
    /// deck `NULL`, and a second pass over unchanged prices writes nothing.
    #[tokio::test]
    async fn folds_deck_proper_by_finish_and_is_idempotent() {
        let db = migrated_memory_db().await;
        // $2.00 regular / $10.00 foil — the foil row must take the foil price.
        let split = insert_card(&db, "sf-split", Some("2.00"), Some("10.00")).await;
        // Priced only as regular; its foil row values nothing.
        let plain = insert_card(&db, "sf-plain", Some("0.50"), None).await;
        let unpriced = insert_card(&db, "sf-unpriced", None, None).await;

        let deck = insert_deck(&db, "valued-tmc").await;
        insert_row(&db, deck, split, PreconBoard::Commander, 1, true).await; // 10.00
        insert_row(&db, deck, split, PreconBoard::Main, 3, false).await; // 6.00
        insert_row(&db, deck, plain, PreconBoard::Main, 4, false).await; // 2.00
        insert_row(&db, deck, unpriced, PreconBoard::Main, 9, false).await; // nothing
        // A pricey sideboard must not inflate the deck, matching `card_count`'s grain.
        insert_row(&db, deck, split, PreconBoard::Side, 4, true).await;

        let ghost = insert_deck(&db, "unpriced-tmc").await;
        insert_row(&db, ghost, unpriced, PreconBoard::Main, 60, false).await;

        // A foil-only row of a card priced only as *regular*: the copies value nothing, but
        // the card is priced, so this stores "$0.00" — exactly what `summarize_holdings`
        // answers on the detail page (its `any_priced` flips on the other finish's price at
        // zero quantity), and a different claim from `ghost`'s NULL.
        let zero = insert_deck(&db, "zero-not-null-tmc").await;
        insert_row(&db, zero, plain, PreconBoard::Main, 2, true).await;

        let changed = refresh_precon_values(&db).await.expect("refresh");
        assert_eq!(changed, 2, "the priceable and the zero-valued decks change");
        assert_eq!(stored(&db, deck).await, Some(1800));
        assert_eq!(
            stored(&db, ghost).await,
            None,
            "nothing priced answers NULL, never $0.00"
        );
        assert_eq!(
            stored(&db, zero).await,
            Some(0),
            "priced-but-worth-nothing answers $0.00, matching the detail fold"
        );

        let again = refresh_precon_values(&db).await.expect("refresh again");
        assert_eq!(again, 0, "no-op on a second pass");
    }

    /// A price change re-values on the next pass — the reason this runs per tick rather
    /// than at rebuild — and a value can also *clear* back to `NULL` when the price goes.
    #[tokio::test]
    async fn revalues_when_prices_move_and_clears_when_they_vanish() {
        let db = migrated_memory_db().await;
        let card_id = insert_card(&db, "sf-mover", Some("1.00"), None).await;
        let deck = insert_deck(&db, "mover-tmc").await;
        insert_row(&db, deck, card_id, PreconBoard::Main, 2, false).await;

        refresh_precon_values(&db).await.expect("first pass");
        assert_eq!(stored(&db, deck).await, Some(200));

        let mut spike: card::ActiveModel = crate::entities::prelude::Card::find_by_id(card_id)
            .one(&db)
            .await
            .expect("query card")
            .expect("card exists")
            .into();
        spike.price_usd = Set(Some("3.25".to_string()));
        spike.update(&db).await.expect("update price");
        refresh_precon_values(&db).await.expect("second pass");
        assert_eq!(stored(&db, deck).await, Some(650));

        let mut gone: card::ActiveModel = crate::entities::prelude::Card::find_by_id(card_id)
            .one(&db)
            .await
            .expect("query card")
            .expect("card exists")
            .into();
        gone.price_usd = Set(None);
        gone.update(&db).await.expect("clear price");
        refresh_precon_values(&db).await.expect("third pass");
        assert_eq!(stored(&db, deck).await, None, "a vanished price clears");
    }
}

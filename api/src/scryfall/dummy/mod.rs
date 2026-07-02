//! Deterministic dummy MTG catalog for offline development, CI, and tests.
//!
//! When `SEED_DUMMY_DATA` is set the server seeds this fabricated catalog instead
//! of streaming Scryfall's ~550 MB bulk file — no network, no images. The fake
//! sets/cards are built as the same `ScryfallSet`/`ScryfallCard` shapes the real
//! importer consumes and then run through the *exact* `ingest::import_sets` /
//! `map::map_card` / `ingest::flush_cards` / `ingest::put_state` path, so seeded
//! rows are byte-identical in shape to production rows (collector-number sort key,
//! comma-joined colours, faces JSON) and no upsert column list is duplicated.
//!
//! Everything is **deterministic**: ids, set codes, and collector numbers are fixed
//! (no clock-derived identities), and the fabricated year of daily price history is a
//! per-card *seeded* random walk (the RNG is seeded from the card id), so it looks
//! random yet reseeds to byte-identical values. The upserts key on
//! `(game, external_id)` / `(game, code)`, so re-seeding on every boot overwrites the
//! same rows rather than growing the catalog. Cards carry no image URLs, so the image
//! proxy is never hit and `has_image` resolves to false everywhere.
//!
//! The fabricated data lives in [`catalog`]; the price random walk in [`prices`];
//! this module orchestrates seeding it into the database.

mod catalog;
mod prices;

use chrono::{Duration, Utc};
use rand::SeedableRng;
use rand::rngs::StdRng;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    DatabaseConnection,
};

use super::GAME;
use super::ingest::{self, IngestError};
use super::map;
use super::price_history;
use catalog::{dummy_cards, dummy_sets};
use crate::entities::{card, card_price_history};
use prices::price_walk;

/// Synthetic `ingest_state.source_updated_at` recorded for a dummy seed. It never
/// equals a real Scryfall RFC3339 timestamp, so a later real sync's version gate
/// (`scryfall::ingest::refresh`) sees a mismatch and re-imports — dummy mode never
/// locks out a switch back to real data. The seed runs unconditionally on every boot,
/// so changing the generated data takes effect on the next restart with no version
/// bump needed; this value only needs to stay distinct from a real Scryfall timestamp.
const DUMMY_SOURCE_VERSION: &str = "dummy-seed-v1";

/// A year of fabricated price history seeded per card (one row per day, ending today),
/// so the chart shows a year of movement rather than a single flat point.
const PRICE_HISTORY_DAYS: i64 = 365;

/// Seed `PRICE_HISTORY_DAYS` of fabricated daily price history per seeded card,
/// reusing the real `(game, card_id, as_of_date)` unique key and upsert helper. Reads
/// the just-seeded `cards` rows for their ids and base prices (the same shape the live
/// `snapshot_prices` reads), then writes one walked row per card per day ending today.
/// Returns the number of rows written.
///
/// A reseed on the **same day** is byte-identical: the same dates and per-card seed
/// reproduce the same values, and the upsert preserves `created_at`. Like the dummy
/// seed generally, this is upsert-only (never deletes), and the window ends at "today",
/// so a long-lived on-disk dummy DB rebooted on a *later* calendar day re-stamps the
/// shifted older dates with fresh walk values and leaves rows past the year mark in
/// place — harmless drift on fabricated offline data; point `SEED_DUMMY_DATA` at a
/// fresh/dedicated DB as the module already advises.
async fn seed_price_history(db: &DatabaseConnection) -> Result<u64, IngestError> {
    let cards = price_history::load_price_columns(db, GAME).await?;

    let today = Utc::now().date_naive();
    let now = Utc::now();
    let days = PRICE_HISTORY_DAYS as usize;
    let mut models: Vec<card_price_history::ActiveModel> = Vec::with_capacity(cards.len() * days);
    for (card_id, usd, usd_foil, eur, tix) in &cards {
        // Seed the walk from the card id so every card has its own reproducible series
        // (independent of iteration order) and a reseed upserts identical values.
        let mut rng = StdRng::seed_from_u64(*card_id as u64);
        let usd_series = price_walk(usd, &mut rng, days);
        let foil_series = price_walk(usd_foil, &mut rng, days);
        let eur_series = price_walk(eur, &mut rng, days);
        let tix_series = price_walk(tix, &mut rng, days);
        for d in 0..days {
            let as_of = price_history::format_date(today - Duration::days(d as i64));
            models.push(card_price_history::ActiveModel {
                id: NotSet,
                game: Set(GAME.to_string()),
                card_id: Set(*card_id),
                as_of_date: Set(as_of),
                price_usd: Set(usd_series[d].clone()),
                price_usd_foil: Set(foil_series[d].clone()),
                price_eur: Set(eur_series[d].clone()),
                price_tix: Set(tix_series[d].clone()),
                created_at: Set(now),
            });
        }
    }

    let total = models.len() as u64;
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<card_price_history::ActiveModel> =
            iter.by_ref().take(price_history::PRICE_HISTORY_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        price_history::upsert_price_history(db, chunk).await?;
    }
    Ok(total)
}

/// Seed the dummy MTG catalog, recording status in `ingest_state`. On failure the
/// state row is best-effort marked `"error"` (mirroring `super::ingest::refresh`) so
/// `GET /status` stays honest, and the error is returned for the caller to log.
pub async fn seed(db: &DatabaseConnection) -> Result<(), IngestError> {
    match seed_inner(db).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = ingest::put_state(
                db,
                ingest::IngestStatus::Error,
                ingest::IngestStateUpdate {
                    detail: Some(ingest::truncate(&err.to_string(), 500)),
                    finished_at: Some(Utc::now()),
                    ..Default::default()
                },
            )
            .await;
            Err(err)
        }
    }
}

async fn seed_inner(db: &DatabaseConnection) -> Result<(), IngestError> {
    let started = Utc::now();
    let cards = dummy_cards();
    let sets = dummy_sets(&cards);
    tracing::info!(
        sets = sets.len(),
        cards = cards.len(),
        "seeding dummy {GAME} catalog"
    );

    // Reuse the real set/card mapping + upsert path so dummy rows are shaped exactly
    // like imported ones (and no on_conflict column list is duplicated here).
    let sets_imported = ingest::import_sets(db, &sets).await?;

    let now = Utc::now();
    let models: Vec<card::ActiveModel> = cards
        .into_iter()
        .map(|c| map::map_card(c, now))
        .collect();
    let cards_imported = models.len() as i32;
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<card::ActiveModel> = iter.by_ref().take(ingest::CARD_BATCH).collect();
        if chunk.is_empty() {
            break;
        }
        ingest::flush_cards(db, chunk).await?;
    }

    // Seed a year of price history so the chart shows a real trend offline.
    let history_rows = seed_price_history(db).await?;
    tracing::info!(rows = history_rows, "seeded dummy price history");

    // Record ONE ingest_state row under the same dataset key the real importer uses,
    // because `ingest_status` loads it with `.one()` filtered only by game (not
    // dataset) and so relies on there being exactly one row per game. A synthetic
    // source version lets a later real sync detect the change and re-import. Do not
    // give dummy its own dataset key — that would create a second row and make the
    // status route ambiguous.
    ingest::put_state(
        db,
        ingest::IngestStatus::Complete,
        ingest::IngestStateUpdate {
            source_updated_at: Some(DUMMY_SOURCE_VERSION.to_string()),
            detail: Some("seeded dummy offline catalog".to_string()),
            sets_imported,
            cards_imported,
            started_at: Some(started),
            finished_at: Some(Utc::now()),
        },
    )
    .await?;
    tracing::info!(
        sets = sets_imported,
        cards = cards_imported,
        "dummy catalog seed complete"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn seeds_multi_day_price_history_and_reseed_is_idempotent() {
        use crate::entities::prelude::{Card, CardPriceHistory};
        use crate::entities::card_price_history;
        use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect};

        let db = crate::test_support::migrated_memory_db().await;

        seed(&db).await.expect("seed succeeds");

        let card_count = dummy_cards().len() as u64;
        let expected_rows = card_count * PRICE_HISTORY_DAYS as u64;

        // One history row per (card, day).
        let rows = CardPriceHistory::find()
            .filter(card_price_history::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(rows, expected_rows, "expected {PRICE_HISTORY_DAYS} days per card");

        // The series spans exactly `PRICE_HISTORY_DAYS` distinct dates.
        let dates: Vec<String> = CardPriceHistory::find()
            .select_only()
            .column(card_price_history::Column::AsOfDate)
            .filter(card_price_history::Column::Game.eq(GAME))
            .distinct()
            .into_tuple()
            .all(&db)
            .await
            .unwrap();
        assert_eq!(dates.len(), PRICE_HISTORY_DAYS as usize);

        // End-to-end anchoring: the newest (today) history point equals the card's
        // current price, confirming day offset 0 maps to the series' first value.
        let today = price_history::format_date(Utc::now().date_naive());
        let sample = Card::find()
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::PriceUsd.is_not_null())
            .one(&db)
            .await
            .unwrap()
            .expect("a card with a usd price");
        let today_row = CardPriceHistory::find()
            .filter(card_price_history::Column::Game.eq(GAME))
            .filter(card_price_history::Column::CardId.eq(sample.id))
            .filter(card_price_history::Column::AsOfDate.eq(today))
            .one(&db)
            .await
            .unwrap()
            .expect("a price row dated today");
        assert_eq!(
            today_row.price_usd, sample.price_usd,
            "today's history must equal the card's current price"
        );

        // Reseeding upserts on the unique key rather than duplicating rows.
        seed(&db).await.expect("reseed succeeds");
        let rows_again = CardPriceHistory::find()
            .filter(card_price_history::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(rows_again, expected_rows, "reseed must not duplicate price history");
    }

    #[tokio::test]
    async fn seed_populates_catalog_and_reseed_is_idempotent() {
        use crate::entities::prelude::{Card, CardSet, IngestState};
        use crate::entities::{card_set, ingest_state};
        use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

        let db = crate::test_support::migrated_memory_db().await;

        seed(&db).await.expect("seed succeeds");

        let expected_cards = dummy_cards().len() as u64;
        let expected_sets = dummy_sets(&dummy_cards()).len() as u64;
        let cards = Card::find()
            .filter(card::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        let sets = CardSet::find()
            .filter(card_set::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(cards, expected_cards);
        assert_eq!(sets, expected_sets);

        // The status row is one-per-game, marked complete, with honest counts.
        let state = IngestState::find()
            .filter(ingest_state::Column::Game.eq(GAME))
            .one(&db)
            .await
            .unwrap()
            .expect("an ingest_state row");
        assert_eq!(state.status, "complete");
        assert_eq!(state.cards_imported as u64, expected_cards);
        assert_eq!(state.sets_imported as u64, expected_sets);

        // The multi-faced card round-trips with faces JSON and no stored image.
        let dfc = Card::find()
            .filter(card::Column::Layout.eq("transform"))
            .one(&db)
            .await
            .unwrap()
            .expect("a transform card");
        assert!(dfc.card_faces.is_some());
        assert!(dfc.image_normal.is_none() && dfc.image_small.is_none());

        // A non-numeric collector number maps to a NULL collector_number_int — the
        // NULLS-LAST sort the set-cards read path relies on.
        let promo = Card::find()
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::CollectorNumber.eq("★"))
            .one(&db)
            .await
            .unwrap()
            .expect("the ★ promo card");
        assert!(promo.collector_number_int.is_none());

        // Re-seeding upserts the same rows rather than inserting duplicates, and keeps
        // exactly one ingest_state row per game (the status route's `.one()` needs it).
        seed(&db).await.expect("reseed succeeds");
        let cards_again = Card::find()
            .filter(card::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        let sets_again = CardSet::find()
            .filter(card_set::Column::Game.eq(GAME))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(cards_again, expected_cards, "reseed must be idempotent");
        assert_eq!(sets_again, expected_sets, "reseed must not add sets");
        let state_rows = IngestState::find()
            .filter(ingest_state::Column::Game.eq(GAME))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(
            state_rows.len(),
            1,
            "reseed must upsert the single status row"
        );
    }
}

//! Game-agnostic card catalog.
//!
//! Holds the registry of supported trading-card games and the entry points for
//! refreshing their card data ([`refresh_all`]) or seeding a dummy offline catalog
//! ([`seed_all`]). Adding a TCG is two steps: add a [`Game`] entry here and route it
//! to a provider in those dispatchers. Everything downstream (entities, handlers,
//! routes, the SPA) is already generic over `game`.

pub mod fingerprint_sync;
pub(crate) mod fingerprint_tasks;
pub mod fingerprints;
pub mod images;
pub mod ingest_state;
pub mod keywords;
pub mod precon_values;
pub mod sld_product_dates;
pub(crate) mod sync_state;

use reqwest::Client;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection};
use serde::Serialize;

/// Static metadata describing a supported game (serialised to the SPA).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct Game {
    /// Stable id slug used in URLs and the `game` column, e.g. `"mtg"`.
    #[schema(value_type = String)]
    pub id: &'static str,
    #[schema(value_type = String)]
    pub name: &'static str,
    #[schema(value_type = String)]
    pub publisher: &'static str,
    /// Upstream data source, shown as attribution in the UI.
    #[schema(value_type = String)]
    pub data_source: &'static str,
}

/// Every game the app knows about.
pub const GAMES: &[Game] = &[Game {
    id: crate::scryfall::GAME,
    name: "Magic: The Gathering",
    publisher: "Wizards of the Coast",
    data_source: "Scryfall",
}];

/// Look up a game by its id slug.
pub fn find(id: &str) -> Option<&'static Game> {
    GAMES.iter().find(|game| game.id == id)
}

/// Refresh catalog data for every supported game from its provider. For MTG this
/// covers the Scryfall card sync and, on top of it, the TCGCSV sealed-product sweep
/// (both version-gated, so an unchanged tick is cheap). `tcgcsv_user_agent` is the
/// descriptive UA TCGCSV requires; `source` decides whether each provider's dataset is
/// pulled from the upstream service or a TCGLense mirror (see [`crate::datasets`]). A
/// failure for one game/source is logged and does not abort the others.
pub async fn refresh_all(
    db: &DatabaseConnection,
    client: &Client,
    tcgcsv_user_agent: &str,
    source: &crate::datasets::SyncSource,
) {
    for game in GAMES {
        match game.id {
            crate::scryfall::GAME => {
                // The three Scryfall datasets below (cards, rulings, art tags) each read
                // one entry out of the same small bulk-data catalog. Fetch it once and
                // lend it to all three: one request per tick instead of three identical
                // ones, so a transport blip has a third as many chances to cost a dataset
                // its whole sync interval — the expected loss is unchanged (one failure
                // now skips three datasets instead of one skipping one), but the failure
                // events are a third as many and so is the load on Scryfall.
                //
                // A failed fetch skips all three: none of them can resolve a download URL
                // without it. It deliberately does *not* mark `ingest_state` errors — no
                // import was attempted, so each dataset stays legitimately `complete` at
                // its current version and the next tick's version gates short-circuit
                // normally instead of forcing an unnecessary full re-download.
                let catalog = match crate::scryfall::client::BulkCatalog::fetch(
                    client,
                    &source.scryfall_bulk_data_url(),
                )
                .await
                {
                    Ok(catalog) => Some(catalog),
                    Err(err) => {
                        // `?err` not `%err`: the Debug chain carries the underlying
                        // cause (connect refused / timed out / DNS), which the Display
                        // form flattens to an undiagnosable "error sending request".
                        tracing::error!(
                            game = game.id,
                            error = ?err,
                            "scryfall bulk catalog fetch failed; skipping its datasets this tick"
                        );
                        None
                    }
                };
                if let Some(catalog) = &catalog
                    && let Err(err) = crate::scryfall::refresh(db, client, source, catalog).await
                {
                    tracing::error!(game = game.id, error = %err, "card data refresh failed");
                }
                // Copy each foil-★ variant's foil price onto its nonfoil base card (issue
                // #209), so a consolidated foil holding values correctly. Runs every tick —
                // even when the version-gated import above was skipped — before the daily
                // snapshot (`snapshot_all`), so the enriched price lands in history too.
                match crate::scryfall::enrich_foil_variant_prices(db).await {
                    Ok(rows) if rows > 0 => {
                        tracing::info!(game = game.id, rows, "enriched foil-variant base prices")
                    }
                    Ok(_) => {}
                    Err(err) => tracing::error!(
                        game = game.id,
                        error = %err,
                        "foil-variant price enrichment failed"
                    ),
                }
                // Recompute which foil-★ variants are folded onto their nonfoil base for
                // display (`cards.folded_onto_id`). Runs right after the enrichment above —
                // same pairs, same tick — so a newly-imported star is hidden from the grids
                // in the same pass that gives its base the foil price.
                crate::tasks::refresh_foil_variant_folds(db).await;
                // Card rulings ("Notes and Rules Information", issue #522): official
                // clarifications keyed by oracle_id. Runs after the card sync so it can
                // scope rulings to stored cards' gameplay ids; version-gated independently,
                // so an unchanged tick is skipped.
                if let Some(catalog) = &catalog
                    && let Err(err) =
                        crate::scryfall::rulings::refresh(db, client, source, catalog).await
                {
                    tracing::error!(game = game.id, error = %err, "card rulings refresh failed");
                }
                // Tagger art tags (issue #140): community "what's in the artwork" labels
                // keyed by illustration_id, behind the `art:` search filter. Runs after
                // the card sync so taggings scope to stored artworks; version-gated
                // independently, so an unchanged tick is skipped.
                if let Some(catalog) = &catalog
                    && let Err(err) =
                        crate::scryfall::art_tags::refresh(db, client, source, catalog).await
                {
                    tracing::error!(game = game.id, error = %err, "art tags refresh failed");
                }
                // Sealed products (TCGCSV). Runs after the card sync so cards exist for
                // the later historic price backfill to join against.
                if let Err(err) =
                    crate::tcgcsv::ingest::refresh(db, client, tcgcsv_user_agent, source).await
                {
                    tracing::error!(game = game.id, error = %err, "product data refresh failed");
                }
                // Sealed-product contents (MTGJSON): which sealed products each card is
                // found in / can be pulled from. Runs last, after both cards + products
                // exist to resolve its Scryfall-id / TCGplayer-id references against.
                if let Err(err) = crate::mtgjson::ingest::refresh(db, client, source).await {
                    tracing::error!(game = game.id, error = %err, "sealed-contents refresh failed");
                }
                // Preconstructed-deck values (`precon_decks.price_cents`), folded from the
                // live card prices. Runs after the sealed-contents sync above (which owns
                // the precon rebuild, and leaves the column NULL) and every tick — even
                // when the version-gated syncs were skipped — because card prices move on
                // ticks the precon tables don't (see `catalog::precon_values`).
                crate::tasks::refresh_precon_values(db).await;
                // Secret Lair per-product release dates, derived from each product's own
                // contents (its `contains` cards' modal release date). Runs after the contents
                // sync so the rows exist to read, and every tick — even when the version-gated
                // syncs above were skipped — so a card-date change or newly-ingested contents
                // re-stamps. TCGCSV files every SLD drop under one group with a single rolling
                // `publishedOn`, so without this SLD products can't be ordered by release date
                // (see `sld_product_dates`).
                match crate::catalog::sld_product_dates::restamp_from_contents(db).await {
                    Ok(changed) if changed > 0 => tracing::info!(
                        game = game.id,
                        changed,
                        "restamped Secret Lair product release dates from contents"
                    ),
                    Ok(_) => {}
                    Err(err) => tracing::error!(
                        game = game.id,
                        error = %err,
                        "Secret Lair product date restamp failed"
                    ),
                }
            }
            other => {
                tracing::warn!(game = other, "no data provider wired for game; skipping");
            }
        }
    }
}

/// Capture today's daily price snapshot for every supported game, then refresh the
/// history tables' planner stats ([`maintain_price_history`]).
///
/// Runs on every sync tick **after** [`refresh_all`], reading the already-committed
/// `cards` rows rather than the streaming import — so the daily series stays
/// continuous even when [`refresh_all`] is version-gated and skips the import (it
/// just records today's date with the last-known prices).
pub async fn snapshot_all(db: &DatabaseConnection) {
    capture_snapshots(db).await;

    // Refresh the price-history tables' planner stats + visibility map now that the
    // capture has appended today's rows, so the collection analytics reads stay fast.
    maintain_price_history(db).await;
}

/// The capture half of [`snapshot_all`]: upsert today's `(game, entity, as_of_date)` price
/// rows from the live price columns, without the follow-up `VACUUM (ANALYZE)`. The sync
/// tick also runs this **before** [`refresh_all`] as insurance — securing today's history
/// row with the last-known prices up front, so a killed, OOM'd, or hung import can no
/// longer cost the day's snapshot (the 2026-08 outage); the history upsert's changed-guard
/// makes the post-refresh re-capture touch only rows whose prices actually moved. A failure
/// for one game is logged and does not abort the others.
pub async fn capture_snapshots(db: &DatabaseConnection) {
    let as_of_date = crate::scryfall::format_date(chrono::Utc::now().date_naive());
    for game in GAMES {
        match game.id {
            crate::scryfall::GAME => {
                // Cards.
                match crate::scryfall::snapshot_prices(db, game.id, &as_of_date).await {
                    Ok(rows) => tracing::info!(
                        game = game.id,
                        rows,
                        as_of = %as_of_date,
                        "captured daily card price snapshot"
                    ),
                    Err(err) => {
                        tracing::error!(game = game.id, error = %err, "card price snapshot failed")
                    }
                }
                // Sealed products.
                match crate::tcgcsv::price_history::snapshot_prices(db, game.id, &as_of_date).await
                {
                    Ok(rows) => tracing::info!(
                        game = game.id,
                        rows,
                        as_of = %as_of_date,
                        "captured daily product price snapshot"
                    ),
                    Err(err) => {
                        tracing::error!(game = game.id, error = %err, "product price snapshot failed")
                    }
                }
            }
            other => {
                tracing::warn!(game = other, "no price snapshot wired for game; skipping");
            }
        }
    }
}

/// Refresh the price-history tables' planner statistics and visibility map after a
/// capture (**Postgres only**), so the collection analytics reads — value-history,
/// movers, and the per-card price chart — keep the per-entity index-seek plan and stay
/// index-only.
///
/// Why this is needed and not left to autovacuum: the daily capture inserts one row per
/// priced entity into a multi-million-row, never-pruned table, and Postgres's default
/// autovacuum triggers are *scale-factor* based, so on a table that large they only fire
/// after ~0.1–0.2·N changes — months of daily captures (`m…053` lowers those thresholds as
/// a backstop). Until an autovacuum runs, two things degrade the analytics reads:
/// stale stats make the planner demote `{card,product}_id IN (…owned…)` from a per-entity
/// index seek to an in-memory filter that scans the whole game's date window (measured on
/// an 18M-row Postgres 16 repro: a 30-day, 956-card value-history read went 6.7 s → 0.16 s
/// after `ANALYZE`), and a stale visibility map turns the covering index's index-only scan
/// into per-row heap-visibility fetches (the churned-VM cliff `m…031` warns about). A
/// `VACUUM (ANALYZE)` right here fixes both.
///
/// It rides the sync tick's leader lock (so only one replica runs it), takes only a
/// `SHARE UPDATE EXCLUSIVE` lock (never blocks readers/writers), and — because `as_of_date`
/// grows monotonically so each day's rows append at the heap's tail — normally scans just
/// that fresh tail. `VACUUM` cannot run inside a transaction; `execute_unprepared` runs it
/// autocommit. A failure is logged and never aborts the tick. A no-op on SQLite (single
/// writer, no MVCC visibility map; its planner stats are a deliberate non-goal).
pub async fn maintain_price_history(db: &DatabaseConnection) {
    if db.get_database_backend() != DatabaseBackend::Postgres {
        return;
    }
    // Both never-pruned history tables the capture just wrote to. Names are fixed
    // literals, so the format! carries no untrusted input.
    for table in ["card_price_history", "product_price_history"] {
        match db
            .execute_unprepared(&format!("VACUUM (ANALYZE) \"{table}\""))
            .await
        {
            Ok(_) => tracing::info!(table, "refreshed price-history stats + visibility map"),
            Err(err) => {
                tracing::warn!(table, error = %err, "post-capture VACUUM (ANALYZE) failed")
            }
        }
    }
}

/// Seed a dummy offline catalog for every supported game. Mirrors [`refresh_all`]
/// but takes no HTTP client — seeding never touches the network — and dispatches per
/// game to its provider's offline seeder. A failure for one game is logged and does
/// not abort the others.
pub async fn seed_all(db: &DatabaseConnection) {
    for game in GAMES {
        let result = match game.id {
            crate::scryfall::GAME => crate::scryfall::seed(db).await,
            other => {
                tracing::warn!(game = other, "no dummy seeder wired for game; skipping");
                continue;
            }
        };
        if let Err(err) = result {
            tracing::error!(game = game.id, error = %err, "dummy catalog seed failed");
        }
    }
}

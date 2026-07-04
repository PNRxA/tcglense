//! Game-agnostic card catalog.
//!
//! Holds the registry of supported trading-card games and the entry points for
//! refreshing their card data ([`refresh_all`]) or seeding a dummy offline catalog
//! ([`seed_all`]). Adding a TCG is two steps: add a [`Game`] entry here and route it
//! to a provider in those dispatchers. Everything downstream (entities, handlers,
//! routes, the SPA) is already generic over `game`.

pub mod images;

use reqwest::Client;
use sea_orm::DatabaseConnection;
use serde::Serialize;

/// Static metadata describing a supported game (serialised to the SPA).
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct Game {
    /// Stable id slug used in URLs and the `game` column, e.g. `"mtg"`.
    pub id: &'static str,
    pub name: &'static str,
    pub publisher: &'static str,
    /// Upstream data source, shown as attribution in the UI.
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
/// descriptive UA TCGCSV requires. A failure for one game/source is logged and does
/// not abort the others.
pub async fn refresh_all(db: &DatabaseConnection, client: &Client, tcgcsv_user_agent: &str) {
    for game in GAMES {
        match game.id {
            crate::scryfall::GAME => {
                if let Err(err) = crate::scryfall::refresh(db, client).await {
                    tracing::error!(game = game.id, error = %err, "card data refresh failed");
                }
                // Sealed products (TCGCSV). Runs after the card sync so cards exist for
                // the later historic price backfill to join against.
                if let Err(err) =
                    crate::tcgcsv::ingest::refresh(db, client, tcgcsv_user_agent).await
                {
                    tracing::error!(game = game.id, error = %err, "product data refresh failed");
                }
                // Sealed-product contents (MTGJSON): which sealed products each card is
                // found in / can be pulled from. Runs last, after both cards + products
                // exist to resolve its Scryfall-id / TCGplayer-id references against.
                if let Err(err) = crate::mtgjson::ingest::refresh(db, client).await {
                    tracing::error!(game = game.id, error = %err, "sealed-contents refresh failed");
                }
            }
            other => {
                tracing::warn!(game = other, "no data provider wired for game; skipping");
            }
        }
    }
}

/// Capture today's daily price snapshot for every supported game.
///
/// Runs on every sync tick **after** [`refresh_all`], reading the already-committed
/// `cards` rows rather than the streaming import — so the daily series stays
/// continuous even when [`refresh_all`] is version-gated and skips the import (it
/// just records today's date with the last-known prices). A failure for one game is
/// logged and does not abort the others.
pub async fn snapshot_all(db: &DatabaseConnection) {
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

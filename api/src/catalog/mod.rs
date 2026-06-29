//! Game-agnostic card catalog.
//!
//! Holds the registry of supported trading-card games and the entry point for
//! refreshing their card data. Adding a TCG is two steps: add a [`Game`] entry
//! here and route it to a provider in [`refresh_all`]. Everything downstream
//! (entities, handlers, routes, the SPA) is already generic over `game`.

pub mod images;

use reqwest::Client;
use sea_orm::DatabaseConnection;
use serde::Serialize;

/// Static metadata describing a supported game (serialised to the SPA).
#[derive(Debug, Clone, Serialize)]
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

/// Refresh card data for every supported game from its provider. A failure for
/// one game is logged and does not abort the others.
pub async fn refresh_all(db: &DatabaseConnection, client: &Client) {
    for game in GAMES {
        let result = match game.id {
            crate::scryfall::GAME => crate::scryfall::refresh(db, client).await,
            other => {
                tracing::warn!(game = other, "no data provider wired for game; skipping");
                continue;
            }
        };
        if let Err(err) = result {
            tracing::error!(game = game.id, error = %err, "card data refresh failed");
        }
    }
}

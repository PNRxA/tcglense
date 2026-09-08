//! The **cheapest-printing** seam: every priced printing of a set of gameplay identities,
//! loaded once so a caller can floor a card's price across *all* its printings — the
//! Secret Lair drops' "cheapest prints" total ([`load_cheapest_by_oracle`]) and the deck
//! pricing breakdown's per-line "cheapest printing of this card" both read from here.
//!
//! Two properties every caller relies on:
//!
//! * **Folded foil-★ variants are excluded** (`folded_onto_id IS NULL`, the same test every
//!   card grid applies through `catalog_cards`). A star row's foil price is already copied
//!   onto its base by `scryfall::enrich_foil_variant_prices`, so leaving it out changes no
//!   minimum — but keeping it in would let the star *win* a tie, and a printing swap that
//!   landed on it would land on a row no grid ever shows.
//! * **Prices are integer cents** through the shared [`valuation`](super::valuation) path,
//!   so a floor computed here can never disagree with a total the valuation folds — same
//!   parse, same rounding, and an unpriced finish is `None`, never zero.
//!
//! The `IN (…)` list is chunked: a deck's row count is caller-controlled, and one
//! unbounded parameter list per request is how a query stops being "one indexed lookup".

use std::collections::{HashMap, HashSet};

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::entities::card;
use crate::entities::prelude::Card;
use crate::error::AppError;

use super::valuation::price_cents;

/// Oracle ids per `WHERE oracle_id IN (…)` lookup — the bound the sealed-product card
/// lookups and the deck token read use.
const CHUNK: usize = 900;

/// One catalog printing's two USD prices, in cents, as the cheapest-printing folds see it.
/// `id` is the **internal** card id — a caller that needs the row on the wire loads it by
/// id afterwards, so the fold itself never carries ~70 columns per printing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PricedPrinting {
    pub id: i32,
    pub usd: Option<i128>,
    pub usd_foil: Option<i128>,
}

impl PricedPrinting {
    /// The cheapest way to own one copy of this printing, either finish — `None` when
    /// neither is priced.
    pub(crate) fn cheapest_single(&self) -> Option<i128> {
        match (self.usd, self.usd_foil) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// Every non-folded printing of each identity in `oracle_ids` (within `game`) that carries
/// **at least one** USD price, grouped by `oracle_id`. An identity none of whose printings
/// is priced is simply absent from the map — "unpriced", which a caller must keep apart
/// from "worth nothing". Within a group the printings come back in ascending internal id,
/// so a fold that breaks ties on order is deterministic.
pub(crate) async fn priced_printings_by_oracle(
    db: &sea_orm::DatabaseConnection,
    game: &str,
    oracle_ids: &HashSet<&str>,
) -> Result<HashMap<String, Vec<PricedPrinting>>, AppError> {
    let mut grouped: HashMap<String, Vec<PricedPrinting>> = HashMap::new();
    if oracle_ids.is_empty() {
        return Ok(grouped);
    }
    let mut ids: Vec<&str> = oracle_ids.iter().copied().collect();
    ids.sort_unstable();
    for chunk in ids.chunks(CHUNK) {
        let rows: Vec<(i32, Option<String>, Option<String>, Option<String>)> = Card::find()
            .select_only()
            .column(card::Column::Id)
            .column(card::Column::OracleId)
            .column(card::Column::PriceUsd)
            .column(card::Column::PriceUsdFoil)
            .filter(card::Column::Game.eq(game))
            .filter(card::Column::OracleId.is_in(chunk.iter().map(|id| id.to_string())))
            .filter(crate::scryfall::not_folded_foil_variant())
            .order_by_asc(card::Column::Id)
            .into_tuple()
            .all(db)
            .await?;
        for (id, oracle_id, usd, usd_foil) in rows {
            let Some(oracle_id) = oracle_id else { continue };
            let printing = PricedPrinting {
                id,
                usd: price_cents(usd.as_deref()),
                usd_foil: price_cents(usd_foil.as_deref()),
            };
            if printing.cheapest_single().is_none() {
                continue;
            }
            grouped.entry(oracle_id).or_default().push(printing);
        }
    }
    Ok(grouped)
}

/// The cheapest single (in integer cents) for each identity in `oracle_ids`: the lowest
/// priced finish of any of its non-folded printings anywhere in the game's catalog, so a
/// card's floor price can come from a cheap reprint in another set rather than the pricier
/// printing on the page. An identity whose printings are all unpriced is absent.
pub(crate) async fn load_cheapest_by_oracle(
    db: &sea_orm::DatabaseConnection,
    game: &str,
    oracle_ids: &HashSet<&str>,
) -> Result<HashMap<String, i128>, AppError> {
    let grouped = priced_printings_by_oracle(db, game, oracle_ids).await?;
    Ok(grouped
        .into_iter()
        .filter_map(|(oracle_id, printings)| {
            printings
                .iter()
                .filter_map(PricedPrinting::cheapest_single)
                .min()
                .map(|cents| (oracle_id, cents))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cheapest_single_takes_the_lower_finish_and_is_none_when_unpriced() {
        let both = PricedPrinting {
            id: 1,
            usd: Some(200),
            usd_foil: Some(150),
        };
        assert_eq!(both.cheapest_single(), Some(150));
        let regular = PricedPrinting {
            id: 2,
            usd: Some(200),
            usd_foil: None,
        };
        assert_eq!(regular.cheapest_single(), Some(200));
        let foil_only = PricedPrinting {
            id: 3,
            usd: None,
            usd_foil: Some(75),
        };
        assert_eq!(foil_only.cheapest_single(), Some(75));
        let none = PricedPrinting {
            id: 4,
            usd: None,
            usd_foil: None,
        };
        assert_eq!(none.cheapest_single(), None);
    }
}

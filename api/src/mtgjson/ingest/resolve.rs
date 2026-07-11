//! Catalog-id resolution helpers shared by the MTGJSON pass and the fallback merge:
//! map external ids (TCGplayer product id, Scryfall id, or `(set, number)`) onto our
//! internal `products.id` / `cards.id`, chunked under SQLite's bind-parameter limit.

use std::collections::HashMap;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};

use super::IN_CHUNK;
use super::super::{GAME, MtgjsonError};
use crate::entities::prelude::{Card, Product};
use crate::entities::{card, product};

/// Collect the distinct owned strings from an iterator of `&String`.
pub(super) fn distinct<'a, I: Iterator<Item = &'a String>>(iter: I) -> Vec<String> {
    let set: std::collections::HashSet<&String> = iter.collect();
    set.into_iter().cloned().collect()
}

/// Resolve TCGplayer product ids -> internal `products.id` for the game, chunked under
/// SQLite's bind limit.
pub(super) async fn resolve_products(
    db: &DatabaseConnection,
    external_ids: &[String],
) -> Result<HashMap<String, i32>, MtgjsonError> {
    let mut map = HashMap::new();
    for chunk in external_ids.chunks(IN_CHUNK) {
        let rows: Vec<(String, i32)> = Product::find()
            .select_only()
            .column(product::Column::ExternalId)
            .column(product::Column::Id)
            .filter(product::Column::Game.eq(GAME))
            .filter(product::Column::ExternalId.is_in(chunk.iter().cloned()))
            .into_tuple()
            .all(db)
            .await?;
        map.extend(rows);
    }
    Ok(map)
}

/// Resolve Scryfall ids -> internal `cards.id` for the game, chunked under SQLite's bind
/// limit.
pub(super) async fn resolve_cards(
    db: &DatabaseConnection,
    external_ids: &[String],
) -> Result<HashMap<String, i32>, MtgjsonError> {
    let mut map = HashMap::new();
    for chunk in external_ids.chunks(IN_CHUNK) {
        let rows: Vec<(String, i32)> = Card::find()
            .select_only()
            .column(card::Column::ExternalId)
            .column(card::Column::Id)
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::ExternalId.is_in(chunk.iter().cloned()))
            .into_tuple()
            .all(db)
            .await?;
        map.extend(rows);
    }
    Ok(map)
}

/// Resolve `(set_code, collector_number)` pairs -> internal `cards.id` for the game (the
/// fallback data keys cards this way rather than by Scryfall id). Fetches by the distinct
/// set codes and indexes in memory — the fallback is a handful of cards, so this is a few
/// small queries. Keys are lowercased set codes; the returned map's keys match.
pub(super) async fn resolve_cards_by_setnum(
    db: &DatabaseConnection,
    keys: &[(String, String)],
) -> Result<HashMap<(String, String), i32>, MtgjsonError> {
    let set_codes: Vec<String> = distinct(keys.iter().map(|(set, _)| set));
    let mut map = HashMap::new();
    for chunk in set_codes.chunks(IN_CHUNK) {
        let rows: Vec<(String, String, i32)> = Card::find()
            .select_only()
            .column(card::Column::SetCode)
            .column(card::Column::CollectorNumber)
            .column(card::Column::Id)
            .filter(card::Column::Game.eq(GAME))
            .filter(card::Column::SetCode.is_in(chunk.iter().cloned()))
            .into_tuple()
            .all(db)
            .await?;
        for (set, number, id) in rows {
            map.insert((set.to_lowercase(), number), id);
        }
    }
    Ok(map)
}

//! Shared card response DTOs: the public card payload (`CardResponse` + its faces
//! and prices) reused by both the catalog and collection endpoints, plus the two
//! small `card::Model` accessors it's built from.
//!
//! The wire DTOs here (and in the other handler modules) carry a test-only
//! `ts_rs::TS` derive: `cargo test` exports each one as a TypeScript type into
//! `web/src/lib/api/generated/` (committed; CI checks for drift), so the SPA's
//! API types are generated from these structs rather than hand-mirrored. The
//! `ts(rename)`s pin the names the web code already uses.

use serde::Serialize;

use crate::entities::card;
use crate::scryfall::model::StoredFace;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "CardPrices"))]
pub(crate) struct PricesResponse {
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
    pub eur: Option<String>,
    pub tix: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "CardFace"))]
pub(crate) struct CardFaceResponse {
    pub name: Option<String>,
    pub mana_cost: Option<String>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
}

/// A single printing of a card, as the SPA sees it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "Card"))]
pub(crate) struct CardResponse {
    pub id: String,
    pub name: String,
    pub set_code: String,
    pub set_name: String,
    pub collector_number: String,
    pub rarity: Option<String>,
    pub lang: String,
    pub released_at: Option<String>,
    pub mana_cost: Option<String>,
    pub cmc: Option<f64>,
    pub type_line: Option<String>,
    pub oracle_text: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub color_identity: Vec<String>,
    pub colors: Vec<String>,
    pub layout: Option<String>,
    pub prices: PricesResponse,
    /// Whether an image is available through the image proxy for this card.
    pub has_image: bool,
    /// The Secret Lair drop this card belongs to (its curated title), for sets
    /// broken into drops; `None` for everything else.
    pub drop_name: Option<String>,
    /// Stable slug of the drop above (anchors/links), paired with `drop_name`.
    pub drop_slug: Option<String>,
    /// Present for multi-faced cards; request face images via `?face=N`.
    pub faces: Vec<CardFaceResponse>,
}

impl From<card::Model> for CardResponse {
    fn from(m: card::Model) -> Self {
        let drop = crate::scryfall::drops::drop_for(&m.game, &m.set_code, &m.collector_number);
        let drop_name = drop.map(|d| d.title.clone());
        let drop_slug = drop.map(|d| d.slug.clone());

        let stored_faces = stored_faces(&m);

        let has_image = m.image_normal.is_some()
            || m.image_small.is_some()
            || m.image_large.is_some()
            || stored_faces
                .iter()
                .any(|f| f.image_normal.is_some() || f.image_small.is_some());

        let faces = stored_faces
            .into_iter()
            .map(|f| CardFaceResponse {
                name: f.name,
                mana_cost: f.mana_cost,
                type_line: f.type_line,
                oracle_text: f.oracle_text,
                power: f.power,
                toughness: f.toughness,
                loyalty: f.loyalty,
            })
            .collect();

        CardResponse {
            id: m.external_id,
            name: m.name,
            set_code: m.set_code,
            set_name: m.set_name,
            collector_number: m.collector_number,
            rarity: m.rarity,
            lang: m.lang,
            released_at: m.released_at,
            mana_cost: m.mana_cost,
            cmc: m.cmc,
            type_line: m.type_line,
            oracle_text: m.oracle_text,
            power: m.power,
            toughness: m.toughness,
            loyalty: m.loyalty,
            color_identity: split_csv(m.color_identity),
            colors: split_csv(m.colors),
            layout: m.layout,
            prices: PricesResponse {
                usd: m.price_usd,
                usd_foil: m.price_usd_foil,
                eur: m.price_eur,
                tix: m.price_tix,
            },
            has_image,
            drop_name,
            drop_slug,
            faces,
        }
    }
}

pub(crate) fn stored_faces(card: &card::Model) -> Vec<StoredFace> {
    card.card_faces
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default()
}

pub(crate) fn split_csv(value: Option<String>) -> Vec<String> {
    value
        .map(|v| {
            v.split(',')
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_csv_handles_empty() {
        assert_eq!(split_csv(None), Vec::<String>::new());
        assert_eq!(split_csv(Some(String::new())), Vec::<String>::new());
        assert_eq!(split_csv(Some("W,U".to_string())), vec!["W", "U"]);
    }
}

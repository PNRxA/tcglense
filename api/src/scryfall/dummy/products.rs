//! Fabricated sealed products for the offline dummy catalog: a handful of booster
//! boxes / bundles / decks across the dummy sets, so the sealed-product routes (list,
//! detail, prices, facets) have data to serve with no network. Pure data — no DB, no
//! clock-derived identities. Prices are deterministic decimal strings.

use chrono::Utc;
use sea_orm::ActiveValue::Set;

use super::super::GAME;
use crate::entities::product;

/// A fabricated sealed product; the constant columns (game, no image, timestamps) are
/// filled in by [`into_active_model`](SeedProduct::into_active_model).
struct SeedProduct {
    /// TCGplayer-style numeric product id, as a string (stable across reboots — the
    /// upsert conflict key). Kept numeric so it parses like a real `productId`.
    external_id: &'static str,
    name: &'static str,
    set_code: &'static str,
    product_type: &'static str,
    released_at: &'static str,
    price_usd: Option<&'static str>,
    price_usd_foil: Option<&'static str>,
}

impl SeedProduct {
    fn into_active_model(self, now: chrono::DateTime<Utc>) -> product::ActiveModel {
        product::ActiveModel {
            game: Set(GAME.to_string()),
            external_id: Set(self.external_id.to_string()),
            name: Set(self.name.to_string()),
            clean_name: Set(Some(self.name.to_string())),
            set_code: Set(self.set_code.to_string()),
            product_type: Set(self.product_type.to_string()),
            url: Set(Some(format!(
                "https://www.tcgplayer.com/product/{}",
                self.external_id
            ))),
            // No image URL keeps the catalog fully offline (has_image resolves false,
            // so the image proxy is never reached).
            image_url: Set(None),
            price_usd: Set(self.price_usd.map(str::to_string)),
            price_usd_foil: Set(self.price_usd_foil.map(str::to_string)),
            released_at: Set(Some(self.released_at.to_string())),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
    }
}

/// The fabricated sealed products across the dummy sets — the single source of truth.
/// Spans a few types (collector display, play pack, bundle, commander deck, draft
/// display) so the facets endpoint has a real vocabulary, and one set (`dmb`) has
/// several so the set filter is exercised.
pub(super) fn dummy_products() -> Vec<product::ActiveModel> {
    let now = Utc::now();
    [
        SeedProduct {
            external_id: "900001",
            name: "Dummy Base Set Collector Booster Box",
            set_code: "dmb",
            product_type: "collector_display",
            released_at: "2024-01-15",
            price_usd: Some("249.99"),
            price_usd_foil: None,
        },
        SeedProduct {
            external_id: "900002",
            name: "Dummy Base Set Play Booster Pack",
            set_code: "dmb",
            product_type: "play_pack",
            released_at: "2024-01-15",
            price_usd: Some("4.49"),
            price_usd_foil: None,
        },
        SeedProduct {
            external_id: "900003",
            name: "Dummy Base Set Bundle",
            set_code: "dmb",
            product_type: "bundle",
            released_at: "2024-01-15",
            price_usd: Some("39.99"),
            price_usd_foil: None,
        },
        SeedProduct {
            external_id: "900004",
            name: "Dummy Universe Commander Deck",
            set_code: "dmu",
            product_type: "commander_deck",
            released_at: "2024-06-20",
            price_usd: Some("44.99"),
            price_usd_foil: None,
        },
        SeedProduct {
            external_id: "900005",
            name: "Dummy Universe Draft Booster Box",
            set_code: "dmu",
            product_type: "draft_display",
            released_at: "2024-06-20",
            // A product with no market price yet — exercises the null-price path.
            price_usd: None,
            price_usd_foil: None,
        },
    ]
    .into_iter()
    .map(|p| p.into_active_model(now))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn products_have_unique_numeric_ids_and_span_types() {
        use std::collections::HashSet;
        let products = dummy_products();
        assert!(products.len() >= 4);
        let mut ids = HashSet::new();
        let mut types = HashSet::new();
        for p in &products {
            let ext = p.external_id.as_ref();
            assert!(ext.parse::<i64>().is_ok(), "id {ext} must be numeric");
            assert!(ids.insert(ext.clone()), "duplicate id {ext}");
            types.insert(p.product_type.as_ref().clone());
        }
        assert!(types.len() >= 3, "products should span several types");
    }
}

//! Serde shapes for TCGCSV's `prices` JSON files and the pure folding of their
//! records into per-product daily prices.
//!
//! Each `{date}/{categoryId}/{groupId}/prices` file inside a daily archive has the
//! same shape as TCGplayer's live prices endpoint:
//! `{"success":true,"errors":[],"results":[{ productId, marketPrice, subTypeName, … }]}`.
//! We only consume `productId`, `marketPrice`, and `subTypeName`; every other field
//! (low/mid/high/directLow) is ignored. Kept provider-generic (no MTG specifics) so
//! part 2 (sealed products) can reuse the same shapes over other categories/groups.

use std::collections::HashMap;

use serde::Deserialize;

// ----- Catalog sweep (part 2: sealed products) -----
//
// The groups + products feeds share TCGplayer's `{"success":…,"results":[…]}`
// envelope. We consume only the handful of fields the products feature needs; every
// other column (sealed flags, pricing tiers we don't ingest, etc.) is ignored.

/// One TCGCSV `/{category}/groups` file: the `results` array of group rows.
#[derive(Debug, Deserialize)]
pub struct GroupsFile {
    #[serde(default)]
    pub results: Vec<Group>,
}

/// A TCGplayer "group" (roughly a set/expansion) within a category.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub group_id: i64,
    #[serde(default)]
    pub name: Option<String>,
    /// Short code that mostly matches our `card_sets.code` for mainline sets. Stored
    /// lowercased as the product's `set_code` so products join to sets like cards do.
    #[serde(default)]
    pub abbreviation: Option<String>,
    /// Release/publish timestamp (e.g. `"2024-02-02T00:00:00"`), when present.
    #[serde(default)]
    pub published_on: Option<String>,
}

/// One TCGCSV `/{category}/{group}/products` file: the `results` array of products.
#[derive(Debug, Deserialize)]
pub struct ProductsFile {
    #[serde(default)]
    pub results: Vec<Product>,
}

/// A TCGplayer product row. Only the fields the sealed-product catalog needs are
/// deserialized; the classification of sealed-vs-card is derived from `extended_data`
/// (see [`super::classify`]).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub product_id: i64,
    pub name: String,
    #[serde(default)]
    pub clean_name: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    /// The tcgplayer.com product page URL (kept for a future buy-links feature).
    #[serde(default)]
    pub url: Option<String>,
    /// Free-form key/value attributes. A `Rarity` or `Number` entry marks the product
    /// as a single card (so sealed = neither) — see [`super::classify::is_sealed`].
    #[serde(default)]
    pub extended_data: Vec<ExtendedData>,
}

/// One `extendedData` attribute. Only `name` is consumed (to tell sealed from cards).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendedData {
    #[serde(default)]
    pub name: String,
}

/// Normalise a group's `publishedOn` into the `"YYYY-MM-DD"` string we store in
/// `released_at` (matching how card/set release dates are stored). A datetime like
/// `"2024-02-02T00:00:00"` keeps only its date part; a blank value is `None`.
pub fn published_on_to_date(published_on: Option<&str>) -> Option<String> {
    let raw = published_on?.trim();
    if raw.is_empty() {
        return None;
    }
    // Split off any time component ("T" for ISO, a space for "date time" forms).
    let date = raw.split(['T', ' ']).next().unwrap_or(raw);
    (!date.is_empty()).then(|| date.to_string())
}

/// One TCGCSV `prices` file: the `results` array of per-product/subtype rows.
#[derive(Debug, Deserialize)]
pub struct PriceFile {
    #[serde(default)]
    pub results: Vec<PriceRecord>,
}

/// One price row: a TCGplayer product in one finish (`subTypeName`) with its
/// `marketPrice`. Other price columns are intentionally not deserialized.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceRecord {
    /// TCGplayer product id — the join key onto `cards.tcgplayer_id`.
    pub product_id: i64,
    /// The finish's market price (dollars), or `null` when TCGplayer has none.
    #[serde(default)]
    pub market_price: Option<f64>,
    /// The finish name, e.g. `"Normal"` or `"Foil"` (others, like etched, ignored).
    #[serde(default)]
    pub sub_type_name: Option<String>,
}

/// A product's captured prices for a single day: the regular (`Normal`) and foil
/// market prices, as decimal strings (matching how `card_price_history` stores them).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DayPrice {
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
}

/// Format a TCGplayer market price as the decimal string we store. A missing,
/// non-finite, or non-positive value is `None` (TCGplayer uses `0`/`null` for
/// "no market price", which must not be stored as a real `$0.00`).
pub fn format_price(value: Option<f64>) -> Option<String> {
    match value {
        Some(price) if price.is_finite() && price > 0.0 => Some(format!("{price:.2}")),
        _ => None,
    }
}

/// Fold a day's price records into per-product `(usd, usd_foil)`, taking
/// `marketPrice` from the `Normal` subtype for `usd` and the `Foil` subtype for
/// `usd_foil`. Products with neither a usable Normal nor Foil market price are
/// absent from the result. The same product id can appear across several files
/// (paginated per group), so callers may fold multiple files into one map.
pub fn aggregate_prices<I>(records: I) -> HashMap<i64, DayPrice>
where
    I: IntoIterator<Item = PriceRecord>,
{
    let mut out: HashMap<i64, DayPrice> = HashMap::new();
    for record in records {
        let Some(price) = format_price(record.market_price) else {
            continue;
        };
        match record.sub_type_name.as_deref() {
            Some("Normal") => out.entry(record.product_id).or_default().usd = Some(price),
            Some("Foil") => out.entry(record.product_id).or_default().usd_foil = Some(price),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_price_rejects_missing_zero_and_negative() {
        assert_eq!(format_price(None), None);
        assert_eq!(format_price(Some(0.0)), None);
        assert_eq!(format_price(Some(-1.5)), None);
        assert_eq!(format_price(Some(f64::NAN)), None);
        assert_eq!(format_price(Some(0.25)), Some("0.25".to_string()));
        assert_eq!(format_price(Some(12.3)), Some("12.30".to_string()));
        assert_eq!(format_price(Some(1234.5)), Some("1234.50".to_string()));
    }

    #[test]
    fn aggregates_normal_and_foil_from_a_prices_file() {
        // A real-shaped fixture: two products, Normal + Foil rows, a null price,
        // and an ignored (etched) subtype.
        let json = r#"{
            "success": true,
            "errors": [],
            "results": [
                {"productId": 100, "lowPrice": 0.1, "midPrice": 0.2, "highPrice": 1.0, "marketPrice": 0.25, "directLowPrice": null, "subTypeName": "Normal"},
                {"productId": 100, "lowPrice": 1.0, "midPrice": 2.0, "highPrice": 5.0, "marketPrice": 1.50, "directLowPrice": null, "subTypeName": "Foil"},
                {"productId": 200, "lowPrice": null, "midPrice": null, "highPrice": null, "marketPrice": null, "directLowPrice": null, "subTypeName": "Normal"},
                {"productId": 200, "lowPrice": 3.0, "midPrice": 4.0, "highPrice": 9.0, "marketPrice": 4.00, "directLowPrice": null, "subTypeName": "Foil"},
                {"productId": 300, "lowPrice": 8.0, "midPrice": 9.0, "highPrice": 20.0, "marketPrice": 9.00, "directLowPrice": null, "subTypeName": "Foil Etched"}
            ]
        }"#;
        let file: PriceFile = serde_json::from_str(json).unwrap();
        let agg = aggregate_prices(file.results);

        // Product 100: both finishes captured.
        assert_eq!(
            agg.get(&100),
            Some(&DayPrice {
                usd: Some("0.25".to_string()),
                usd_foil: Some("1.50".to_string()),
            })
        );
        // Product 200: null Normal market price drops usd; Foil kept.
        assert_eq!(
            agg.get(&200),
            Some(&DayPrice {
                usd: None,
                usd_foil: Some("4.00".to_string()),
            })
        );
        // Product 300: only an ignored etched subtype → absent entirely.
        assert!(!agg.contains_key(&300));
    }

    #[test]
    fn published_on_keeps_only_the_date() {
        assert_eq!(published_on_to_date(None), None);
        assert_eq!(published_on_to_date(Some("")), None);
        assert_eq!(published_on_to_date(Some("   ")), None);
        assert_eq!(
            published_on_to_date(Some("2024-02-02T00:00:00")),
            Some("2024-02-02".to_string())
        );
        assert_eq!(
            published_on_to_date(Some("2024-02-02 12:00:00")),
            Some("2024-02-02".to_string())
        );
        assert_eq!(
            published_on_to_date(Some("2024-02-02")),
            Some("2024-02-02".to_string())
        );
    }

    #[test]
    fn parses_a_groups_file() {
        let json = r#"{
            "success": true,
            "errors": [],
            "results": [
                {"groupId": 2377, "name": "Murders at Karlov Manor", "abbreviation": "MKM", "publishedOn": "2024-02-09T00:00:00"},
                {"groupId": 2378, "name": "No Abbrev Group", "abbreviation": null, "publishedOn": null}
            ]
        }"#;
        let file: GroupsFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.results.len(), 2);
        assert_eq!(file.results[0].group_id, 2377);
        assert_eq!(file.results[0].abbreviation.as_deref(), Some("MKM"));
        assert_eq!(
            file.results[0].published_on.as_deref(),
            Some("2024-02-09T00:00:00")
        );
        assert!(file.results[1].abbreviation.is_none());
    }

    #[test]
    fn parses_a_products_file_with_extended_data() {
        let json = r#"{
            "success": true,
            "errors": [],
            "results": [
                {
                    "productId": 100,
                    "name": "Murders at Karlov Manor Collector Booster Box",
                    "cleanName": "Murders at Karlov Manor Collector Booster Box",
                    "imageUrl": "https://tcgplayer-cdn.tcgplayer.com/product/100_200w.jpg",
                    "categoryId": 1,
                    "groupId": 2377,
                    "url": "https://www.tcgplayer.com/product/100",
                    "extendedData": [{"name": "UPC", "displayName": "UPC", "value": "1234"}]
                },
                {
                    "productId": 200,
                    "name": "Some Single Card",
                    "extendedData": [
                        {"name": "Rarity", "displayName": "Rarity", "value": "Mythic"},
                        {"name": "Number", "displayName": "Number", "value": "123"}
                    ]
                }
            ]
        }"#;
        let file: ProductsFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.results.len(), 2);
        let sealed = &file.results[0];
        assert_eq!(sealed.product_id, 100);
        assert_eq!(sealed.clean_name.as_deref(), Some("Murders at Karlov Manor Collector Booster Box"));
        assert_eq!(sealed.url.as_deref(), Some("https://www.tcgplayer.com/product/100"));
        assert_eq!(sealed.extended_data.len(), 1);
        assert_eq!(sealed.extended_data[0].name, "UPC");
        let card = &file.results[1];
        assert_eq!(card.extended_data.len(), 2);
    }
}

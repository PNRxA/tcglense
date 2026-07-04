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
}

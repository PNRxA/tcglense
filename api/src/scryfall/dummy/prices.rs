//! The fabricated per-card price random walk that gives the offline catalog a year
//! of chart movement. Deterministic: seeded per card, so a reseed reproduces
//! byte-identical values.

use rand::RngExt;
use rand::rngs::StdRng;

/// Per-step volatility of the fabricated price random walk: each day back in time
/// multiplies the running factor by `1 ±` up to this fraction.
const PRICE_STEP: f64 = 0.04;
/// Per-step mean reversion pulling the walk back toward the card's current price, so a
/// year of daily steps wanders realistically without drifting far from day 0's price.
const PRICE_REVERSION: f64 = 0.02;

/// Build a deterministic "random" price series for one currency, indexed by day offset
/// (`series[0]` = today, `series[days - 1]` = a year ago). Day 0 is anchored to the
/// card's current `base` price so the chart's newest point matches the price shown
/// elsewhere; each older day applies a seeded random-walk step (a small shock plus mild
/// mean reversion back toward today's price, so a year of steps wanders without running
/// away). A `None` base (tokens, or a foil-only card's missing regular price) yields an
/// all-`None` series. The walk draws from `rng`, so a reseed with the same per-card seed
/// reproduces byte-identical values; values are clamped to a positive minimum and
/// formatted as 2-decimal strings, matching how real prices are stored.
pub(super) fn price_walk(
    base: &Option<String>,
    rng: &mut StdRng,
    days: usize,
) -> Vec<Option<String>> {
    let Some(base_val) = base.as_deref().and_then(|s| s.parse::<f64>().ok()) else {
        return vec![None; days];
    };
    let mut series = Vec::with_capacity(days);
    let mut factor = 1.0_f64;
    for d in 0..days {
        if d > 0 {
            let shock: f64 = rng.random_range(-PRICE_STEP..PRICE_STEP);
            factor *= 1.0 + shock;
            factor += PRICE_REVERSION * (1.0 - factor);
        }
        let price = (base_val * factor).max(0.01);
        series.push(Some(format!("{price:.2}")));
    }
    series
}

#[cfg(test)]
mod tests {
    use super::super::PRICE_HISTORY_DAYS;
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn price_walk_is_seeded_anchored_and_handles_none() {
        let days = PRICE_HISTORY_DAYS as usize;

        // A `None` base (e.g. a token) yields an all-`None` series of the right length.
        let none = price_walk(&None, &mut StdRng::seed_from_u64(1), days);
        assert_eq!(none.len(), days);
        assert!(none.iter().all(Option::is_none));

        // Day 0 (today) is anchored exactly to the card's current price.
        let series = price_walk(&Some("10.00".into()), &mut StdRng::seed_from_u64(42), days);
        assert_eq!(series.len(), days);
        assert_eq!(series[0].as_deref(), Some("10.00"));

        // Same seed → byte-identical series, so a reseed upserts the same values.
        let a = price_walk(&Some("10.00".into()), &mut StdRng::seed_from_u64(7), days);
        let b = price_walk(&Some("10.00".into()), &mut StdRng::seed_from_u64(7), days);
        assert_eq!(a, b, "same seed must reproduce the same walk");

        // The walk actually moves (not a flat line) and every value is a positive,
        // well-formed 2-decimal string.
        assert!(
            series.iter().flatten().any(|v| v != "10.00"),
            "a year of steps should move off the anchor price"
        );
        for v in series.iter().flatten() {
            let n: f64 = v.parse().expect("price parses as f64");
            assert!(n >= 0.01, "prices stay positive");
            assert_eq!(v.split('.').nth(1).map(str::len), Some(2));
        }
    }
}

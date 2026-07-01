//! Collection valuation: parsing the stored decimal price strings into integer
//! cents, an accumulator that totals a set of holdings (regular + foil) in cents,
//! and rendering the total back to a 2-dp USD string. Kept separate so the summary,
//! the set-scoped summary, and the per-set landing all value holdings identically.

/// Parse a stored decimal price string (e.g. `"12.34"`) to integer USD cents,
/// rounding to the nearest cent. `None`/empty/unparseable yields `None` so an
/// unpriced card simply doesn't contribute to a valuation.
pub(crate) fn price_cents(price: Option<&str>) -> Option<i128> {
    let value: f64 = price?.trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some((value * 100.0).round() as i128)
}

/// Format integer USD cents as a 2-dp decimal string (e.g. `1234` -> `"12.34"`).
pub(crate) fn format_cents(cents: i128) -> String {
    let dollars = cents / 100;
    let rem = (cents % 100).abs();
    format!("{dollars}.{rem:02}")
}

/// Running valuation of a set of holdings in integer cents, tracking whether any
/// holding was priced so an all-unpriced set reports `null` rather than `$0.00`.
#[derive(Debug, Default)]
pub(crate) struct Valuation {
    pub(crate) cents: i128,
    pub(crate) any_priced: bool,
}

impl Valuation {
    /// Add one card's holding: `qty` regular copies at `usd`, `foil_qty` foil copies
    /// at `usd_foil`. An unpriced finish contributes nothing (and doesn't flip
    /// `any_priced`); a priced finish adds `price × copies` and marks the total priced.
    pub(crate) fn add(
        &mut self,
        usd: Option<&str>,
        qty: i32,
        usd_foil: Option<&str>,
        foil_qty: i32,
    ) {
        if let Some(cents) = price_cents(usd) {
            self.cents += cents * i128::from(qty);
            self.any_priced = true;
        }
        if let Some(cents) = price_cents(usd_foil) {
            self.cents += cents * i128::from(foil_qty);
            self.any_priced = true;
        }
    }

    /// The total as a 2-dp USD string, or `None` when nothing was priced.
    pub(crate) fn total_usd(&self) -> Option<String> {
        self.any_priced.then(|| format_cents(self.cents))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_cents_parses_and_rounds() {
        assert_eq!(price_cents(Some("12.34")), Some(1234));
        assert_eq!(price_cents(Some("0.5")), Some(50));
        assert_eq!(price_cents(Some("  1  ")), Some(100));
        assert_eq!(price_cents(Some("0.005")), Some(1)); // rounds to nearest cent
        assert_eq!(price_cents(Some("")), None);
        assert_eq!(price_cents(Some("n/a")), None);
        assert_eq!(price_cents(None), None);
    }

    #[test]
    fn format_cents_renders_two_decimals() {
        assert_eq!(format_cents(1234), "12.34");
        assert_eq!(format_cents(5), "0.05");
        assert_eq!(format_cents(100), "1.00");
        assert_eq!(format_cents(0), "0.00");
    }
}

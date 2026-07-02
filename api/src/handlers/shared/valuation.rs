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

/// Per-unit price (in cents) at or above which a card is *not* bulk. Any single copy
/// priced strictly under $1.00 counts toward the "bulk" value — the low-value
/// commons/uncommons you'd sell by the box rather than one at a time.
pub(crate) const BULK_THRESHOLD_CENTS: i128 = 100;

/// Running valuation of a set of holdings in integer cents. Tracks the full total,
/// the "bulk" subtotal (finishes priced under [`BULK_THRESHOLD_CENTS`]), and whether
/// any holding was priced at all — so an all-unpriced set reports `null` rather than
/// `$0.00` for both figures.
#[derive(Debug, Default)]
pub(crate) struct Valuation {
    pub(crate) cents: i128,
    pub(crate) bulk_cents: i128,
    pub(crate) any_priced: bool,
}

impl Valuation {
    /// Add one card's holding: `qty` regular copies at `usd`, `foil_qty` foil copies
    /// at `usd_foil`. An unpriced finish contributes nothing (and doesn't flip
    /// `any_priced`); a priced finish adds `price × copies` to the total and, when the
    /// per-unit price is under [`BULK_THRESHOLD_CENTS`], to the bulk subtotal too.
    pub(crate) fn add(
        &mut self,
        usd: Option<&str>,
        qty: i32,
        usd_foil: Option<&str>,
        foil_qty: i32,
    ) {
        self.add_finish(usd, qty);
        self.add_finish(usd_foil, foil_qty);
    }

    /// Fold one finish (a price + copy count) into the total, and — if the per-unit
    /// price is under the bulk threshold — the bulk subtotal. Bulk is judged per unit,
    /// so a card whose regular printing is bulk but whose foil isn't (or vice-versa)
    /// contributes only its bulk finish to the bulk subtotal.
    fn add_finish(&mut self, price: Option<&str>, qty: i32) {
        if let Some(cents) = price_cents(price) {
            let amount = cents * i128::from(qty);
            self.cents += amount;
            if cents < BULK_THRESHOLD_CENTS {
                self.bulk_cents += amount;
            }
            self.any_priced = true;
        }
    }

    /// The total as a 2-dp USD string, or `None` when nothing was priced.
    pub(crate) fn total_usd(&self) -> Option<String> {
        self.any_priced.then(|| format_cents(self.cents))
    }

    /// The bulk subtotal (finishes priced under $1) as a 2-dp USD string, or `None`
    /// when nothing was priced. Gated on the same `any_priced` flag as the total, so a
    /// priced collection with no bulk cards reports `"0.00"` (not `null`) — the total is
    /// meaningful, the bulk portion of it is genuinely zero.
    pub(crate) fn bulk_usd(&self) -> Option<String> {
        self.any_priced.then(|| format_cents(self.bulk_cents))
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

    #[test]
    fn valuation_totals_and_bulk_split_at_a_dollar() {
        let mut v = Valuation::default();
        // $0.50 regular ×2 (all bulk) + $5.00 foil ×1 (not bulk).
        v.add(Some("0.50"), 2, Some("5.00"), 1);
        // Exactly $1.00 is NOT bulk (strictly under a dollar); ×3 regular.
        v.add(Some("1.00"), 3, None, 0);
        // A foil-only bulk finish: $0.99 ×1.
        v.add(None, 0, Some("0.99"), 1);
        // Total = 1.00 + 5.00 + 3.00 + 0.99 = 9.99.
        assert_eq!(v.total_usd().as_deref(), Some("9.99"));
        // Bulk = (0.50×2) + 0.99 = 1.99 — the $5.00 foil and $1.00 regulars excluded.
        assert_eq!(v.bulk_usd().as_deref(), Some("1.99"));
    }

    #[test]
    fn bulk_is_zero_when_priced_but_nothing_is_bulk() {
        let mut v = Valuation::default();
        v.add(Some("5.00"), 1, Some("12.00"), 2);
        // Something is priced, so bulk is a meaningful "0.00", not null.
        assert_eq!(v.total_usd().as_deref(), Some("29.00"));
        assert_eq!(v.bulk_usd().as_deref(), Some("0.00"));
    }

    #[test]
    fn total_and_bulk_are_null_when_nothing_is_priced() {
        let mut v = Valuation::default();
        v.add(None, 3, None, 2);
        assert_eq!(v.total_usd(), None);
        assert_eq!(v.bulk_usd(), None);
    }
}

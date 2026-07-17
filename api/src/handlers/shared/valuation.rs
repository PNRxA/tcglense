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

/// The cheapest way to own one copy of a printing: the lower of its regular (`usd`) and foil
/// (`usd_foil`) price, in integer cents, or `None` when neither finish is priced. Unlike a
/// [`Valuation`] (which totals held copies of *both* finishes) this collapses a printing to its
/// single cheapest finish — whichever that is, since a foil-only or oddly-priced printing can
/// have the regular price missing or dearer than the foil. Used to floor a card's price across
/// all its printings (see the Secret Lair drops handler).
pub(crate) fn cheapest_single_cents(usd: Option<&str>, usd_foil: Option<&str>) -> Option<i128> {
    match (price_cents(usd), price_cents(usd_foil)) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Default per-unit price (in cents) at or above which a card is *not* bulk: $1.00. Any
/// single copy priced strictly under the threshold counts toward the "bulk" value — the
/// low-value commons/uncommons you'd sell by the box rather than one at a time. The SPA
/// can override the cutoff per request (a user's chosen bulk threshold, issue #289) via
/// [`resolve_bulk_threshold_cents`]; this is the value used when it doesn't.
pub(crate) const BULK_THRESHOLD_CENTS: i128 = 100;

/// Upper bound on a client-supplied bulk threshold: $10,000 in cents. Well past any real
/// single-card price, so it never constrains a genuine choice, but it bounds the value so
/// a bogus request can't set a nonsensical (or, folded into `i128` arithmetic, overflowing)
/// cutoff.
pub(crate) const MAX_BULK_THRESHOLD_CENTS: i64 = 1_000_000;

/// Resolve an optional client-supplied bulk threshold (a per-unit price cutoff, in USD
/// cents) into the value a [`Valuation`] uses. Absent yields the default
/// [`BULK_THRESHOLD_CENTS`] ($1); a supplied value is clamped to
/// `[0, MAX_BULK_THRESHOLD_CENTS]` — a negative cutoff collapses to 0 (nothing counts as
/// bulk) and an oversized one to the cap — so a hand-edited query can never drive a
/// nonsensical or overflowing threshold.
pub(crate) fn resolve_bulk_threshold_cents(requested: Option<i64>) -> i128 {
    match requested {
        None => BULK_THRESHOLD_CENTS,
        Some(cents) => i128::from(cents.clamp(0, MAX_BULK_THRESHOLD_CENTS)),
    }
}

/// Running valuation of a set of holdings in integer cents. Tracks the full total,
/// the "bulk" subtotal (finishes priced under [`Valuation::bulk_threshold_cents`]), and
/// whether any holding was priced at all — so an all-unpriced set reports `null` rather
/// than `$0.00` for both figures.
#[derive(Debug)]
pub(crate) struct Valuation {
    pub(crate) cents: i128,
    pub(crate) bulk_cents: i128,
    pub(crate) any_priced: bool,
    /// Per-unit price (in cents) at or above which a finish is *not* bulk — the cutoff
    /// this valuation splits `bulk_cents` out at. Fixed at construction (the default $1,
    /// or the request's chosen threshold via [`Valuation::new`]).
    bulk_threshold_cents: i128,
}

/// The default valuation uses the standard $1 bulk cutoff; [`Valuation::new`] overrides it.
impl Default for Valuation {
    fn default() -> Self {
        Self::new(BULK_THRESHOLD_CENTS)
    }
}

impl Valuation {
    /// A fresh, empty valuation that splits its bulk subtotal at `bulk_threshold_cents` — a
    /// per-unit price cutoff in cents (already resolved/clamped; see
    /// [`resolve_bulk_threshold_cents`]).
    pub(crate) fn new(bulk_threshold_cents: i128) -> Self {
        Self {
            cents: 0,
            bulk_cents: 0,
            any_priced: false,
            bulk_threshold_cents,
        }
    }

    /// Add one card's holding: `qty` regular copies at `usd`, `foil_qty` foil copies
    /// at `usd_foil`. An unpriced finish contributes nothing (and doesn't flip
    /// `any_priced`); a priced finish adds `price × copies` to the total and, when the
    /// per-unit price is under this valuation's `bulk_threshold_cents`, to the bulk
    /// subtotal too.
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
            if cents < self.bulk_threshold_cents {
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
    fn cheapest_single_takes_the_lower_priced_finish() {
        // Both finishes priced -> the cheaper one wins, regardless of which is which.
        assert_eq!(
            cheapest_single_cents(Some("2.00"), Some("10.00")),
            Some(200)
        );
        assert_eq!(
            cheapest_single_cents(Some("10.00"), Some("2.00")),
            Some(200)
        );
        // Only one finish priced (a foil-only or regular-only printing) -> that one.
        assert_eq!(cheapest_single_cents(None, Some("3.50")), Some(350));
        assert_eq!(cheapest_single_cents(Some("4.25"), None), Some(425));
        // An empty/unparseable finish is treated as absent, not $0.
        assert_eq!(cheapest_single_cents(Some(""), Some("3.50")), Some(350));
        // Neither finish priced -> nothing to contribute.
        assert_eq!(cheapest_single_cents(None, None), None);
        assert_eq!(cheapest_single_cents(Some(""), Some("n/a")), None);
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

    #[test]
    fn bulk_split_follows_a_custom_threshold() {
        // A $5 cutoff: the $0.50 and $1.00 finishes are now both bulk (strictly under $5),
        // the $5.00 foil is not (exactly at the boundary), so bulk = (0.50×2) + (1.00×3) = 4.00.
        let mut v = Valuation::new(500);
        v.add(Some("0.50"), 2, Some("5.00"), 1);
        v.add(Some("1.00"), 3, None, 0);
        assert_eq!(v.total_usd().as_deref(), Some("9.00")); // total is threshold-independent
        assert_eq!(v.bulk_usd().as_deref(), Some("4.00"));

        // A $0 cutoff means nothing is ever bulk (no price is strictly under zero).
        let mut none = Valuation::new(0);
        none.add(Some("0.01"), 1, None, 0);
        assert_eq!(none.total_usd().as_deref(), Some("0.01"));
        assert_eq!(none.bulk_usd().as_deref(), Some("0.00"));
    }

    #[test]
    fn resolve_bulk_threshold_defaults_and_clamps() {
        // Absent -> the default $1.
        assert_eq!(resolve_bulk_threshold_cents(None), BULK_THRESHOLD_CENTS);
        // A plain value passes through.
        assert_eq!(resolve_bulk_threshold_cents(Some(250)), 250);
        assert_eq!(resolve_bulk_threshold_cents(Some(0)), 0);
        // A negative cutoff clamps to 0 (nothing is bulk), not a negative threshold.
        assert_eq!(resolve_bulk_threshold_cents(Some(-5)), 0);
        // An oversized value clamps to the cap rather than driving a nonsensical cutoff.
        assert_eq!(
            resolve_bulk_threshold_cents(Some(i64::MAX)),
            i128::from(MAX_BULK_THRESHOLD_CENTS)
        );
    }
}

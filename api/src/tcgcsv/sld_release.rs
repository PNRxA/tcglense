//! Secret Lair Drop **release-date derivation** — the date an individual drop first sold.
//!
//! TCGCSV lumps *every* Secret Lair drop under one group ("Secret Lair", abbreviation `SLD`)
//! with a single group-level `publishedOn`. So the group date is the **same** for every drop,
//! and [`super::ingest`] would otherwise stamp that one date on all of them (the bug this
//! module fixes). But each drop released on its own date, and that date lives on the drop's
//! *cards*: Scryfall gives every `sld` printing a per-card `released_at`.
//!
//! So we **derive** each SLD product's release date the same way [`super::sld_msrp`] derives
//! its MSRP: resolve the product to its gallery drop (reusing [`crate::mtgjson::sld`]'s
//! name-matching), then take the modal `released_at` among that drop's cards (ties → earliest),
//! matching the card by-drop view's `handlers::catalog::sets::drop_released_at` so the two
//! surfaces agree. A product that resolves to no drop (a non-drop `SLD` product — commander
//! deck, bundle, single-card promo) keeps the group date, as does a drop whose cards carry no
//! known date yet — no worse than the status quo, never a wrong date.
//!
//! Unlike the MSRP derivation this reads no static file: the per-drop dates come from the
//! already-ingested `cards` table, passed in as a `collector_number -> released_at` map that
//! [`super::ingest`] builds once per sweep (cards are ingested before products in
//! [`crate::catalog::refresh_all`], so the map is populated on the first sweep). This module
//! stays pure over that map so the resolution is unit-testable without a DB.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::mtgjson::sld;

/// Derive the release date (`YYYY-MM-DD`) for a Secret Lair sealed product from its drop's
/// cards, or `None` when it doesn't apply: not the `sld` set, no drop snapshot loaded, the
/// product resolves to no gallery drop, or none of the drop's cards have a known release date.
/// `card_dates` maps an `sld` collector number to that card's `released_at`.
///
/// The **modal** date among the drop's cards is used (ties → earliest): a drop's cards share
/// one street date, so the mode *is* that date and shrugs off a stray reprint carrying a
/// different one — matching `handlers::catalog::sets::drop_released_at`, which derives the same
/// date for the card by-drop view, so the product and card surfaces agree. ISO `YYYY-MM-DD`
/// dates compare lexicographically = chronologically, so the earliest-tie-break needs no parsing.
pub fn derive(
    set_code: &str,
    external_id: &str,
    name: &str,
    card_dates: &HashMap<String, String>,
) -> Option<String> {
    if set_code != sld::SET_CODE {
        return None;
    }
    let table = sld::table()?;
    let pd = sld::resolve_product_drop(&table, external_id, name)?;
    // Tally the drop's cards' dates and take the mode (ties → the earlier ISO date), mirroring
    // `handlers::catalog::sets::drop_released_at` so this agrees with the card by-drop view.
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for cn in pd.collector_numbers() {
        if let Some(date) = card_dates.get(cn) {
            *counts.entry(date.as_str()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        // Highest count wins; on a tie prefer the earlier date (smaller ISO-8601 string).
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(date, _)| date.to_string())
}

/// A stable content hash (64 bits of SHA-256, hex) of the per-drop date inputs — the
/// `collector_number -> released_at` map [`derive`] reads. [`super::ingest`] folds it into the
/// products sync version gate alongside [`super::msrp::version`] and [`super::sld_msrp::version`],
/// so a change to the `sld` cards' release dates re-runs the sweep on the next tick **even when
/// TCGCSV and the drop snapshot are byte-identical** — closing, for the release-date derivation,
/// the same TCGCSV-independence gap `sld_msrp::version` closes for the drop snapshot.
///
/// Order-independent: the pairs are sorted before hashing so the map's iteration order can't
/// change the hash. Delimiters are `=`/`;`, which can't occur in a collector number (alphanumeric)
/// or an ISO date, so distinct data can't collide.
pub fn version(card_dates: &HashMap<String, String>) -> String {
    let mut pairs: Vec<(&str, &str)> = card_dates
        .iter()
        .map(|(cn, date)| (cn.as_str(), date.as_str()))
        .collect();
    pairs.sort_unstable();
    let mut hasher = Sha256::new();
    for (cn, date) in pairs {
        hasher.update(cn.as_bytes());
        hasher.update(b"=");
        hasher.update(date.as_bytes());
        hasher.update(b";");
    }
    hex::encode(&hasher.finalize()[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dates(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(cn, d)| (cn.to_string(), d.to_string()))
            .collect()
    }

    #[test]
    fn derives_modal_date_shrugging_off_a_stray_outlier() {
        // "Cats of Chaos" (in the shipped snapshot) is collector numbers 2690–2694. Most of the
        // drop's cards share one street date; a single reprint carries an earlier outlier date,
        // and a card in no drop must be ignored. The modal date wins — `min` would wrongly pick
        // the earlier outlier.
        let card_dates = dates(&[
            ("2690", "2024-05-03"),
            ("2691", "2024-05-03"),
            ("2692", "2024-05-03"),
            ("2694", "2019-01-01"), // a stray reprint with a different, earlier date
            ("999999", "2000-01-01"), // not one of the drop's cards: ignored
        ]);
        assert_eq!(
            derive(
                "sld",
                "700795",
                "Secret Lair Drop: Cats of Chaos - Non-Foil Edition",
                &card_dates,
            )
            .as_deref(),
            Some("2024-05-03"),
        );
    }

    #[test]
    fn gates_non_sld_and_unresolved_products() {
        let card_dates = dates(&[("2690", "2024-05-03")]);
        // Wrong set: never derived (the group date stands).
        assert!(
            derive(
                "mkm",
                "700795",
                "Secret Lair Drop: Cats of Chaos - Non-Foil Edition",
                &card_dates,
            )
            .is_none()
        );
        // A non-drop SLD product (a single-card promo) resolves to no gallery drop: None.
        assert!(
            derive(
                "sld",
                "554987",
                "Secret Lair Drop: Secret Lair Promo: Seedborn Muse - Rainbow Foil Edition",
                &card_dates,
            )
            .is_none()
        );
    }

    #[test]
    fn version_is_stable_order_independent_and_tracks_changes() {
        let a = dates(&[("2690", "2024-05-06"), ("2691", "2024-05-03")]);
        // Same pairs inserted in the other order hash identically (order-independent).
        let b = dates(&[("2691", "2024-05-03"), ("2690", "2024-05-06")]);
        assert_eq!(version(&a), version(&b));
        assert_eq!(version(&a).len(), 16); // 8 bytes hex-encoded
        // A changed date bumps the hash; so does an added/removed entry.
        let changed = dates(&[("2690", "2024-05-07"), ("2691", "2024-05-03")]);
        assert_ne!(version(&a), version(&changed));
        let added = dates(&[
            ("2690", "2024-05-06"),
            ("2691", "2024-05-03"),
            ("2692", "2024-05-03"),
        ]);
        assert_ne!(version(&a), version(&added));
        // The empty map is stable and distinct from any populated one.
        assert_eq!(version(&HashMap::new()), version(&HashMap::new()));
        assert_ne!(version(&HashMap::new()), version(&a));
    }

    #[test]
    fn none_when_no_drop_card_carries_a_date() {
        // The product resolves to its drop, but none of the drop's collector numbers have a
        // known date — so there's nothing to derive and the group date is kept.
        let card_dates = dates(&[("111111", "2024-01-01")]);
        assert!(
            derive(
                "sld",
                "700795",
                "Secret Lair Drop: Cats of Chaos - Non-Foil Edition",
                &card_dates,
            )
            .is_none()
        );
    }
}

//! The expected-value maths: pure, over the [`ResolvedPack`] list the loader built and the
//! prices the [`CardIndex`] parsed. No database, no request — hand it a configuration and a
//! price and it tells you what an average pack is worth, which is what makes it testable
//! against numbers computed on paper.
//!
//! The model, in one place:
//!
//! * A pack rolls **one variant** with probability `weight / Σ weights`, then draws `count`
//!   cards from each sheet that variant's slots name.
//! * A sheet's card `c` is pulled with probability `w_c / T`, where `T` is the sheet's
//!   `total_weight` — which **includes** the weight of cards our catalog doesn't hold. The
//!   missing share is reported (`priced_share`, and a caveat) rather than re-normalised
//!   away, because re-normalising would quietly inflate every remaining card's odds.
//! * So a sheet contributes `picks = Σ_v P(v) × count_v(sheet)` cards to an average pack,
//!   and `picks × Σ_c (w_c / T) × price_c` dollars.
//! * A **fixed** sheet isn't drawn at all: a slot takes its first `count` cards in stored
//!   order, so the card at position `i` appears in exactly the variants whose count exceeds
//!   `i` — `Σ_{v: count_v > i} P(v)` copies per pack, which is the same expectation
//!   arithmetic with the randomness taken out.
//!
//! Each pick is treated as an independent draw even where the real sheet is drawn without
//! replacement. On a sheet of any size the difference is invisible; on a tiny one it isn't,
//! which is why every response says so. (The [`super::open`] simulator, which deals actual
//! cards, does draw without replacement — a repeated card there would be a visible lie.)

use std::collections::HashSet;

use crate::entities::booster_sheet::UNRESOLVED_CARD_ID;

use super::{
    CardIndex, CardResponses, PackCardOdds, PackEv, ProductEv, ResolvedConfig, ResolvedPack,
    ResolvedSheet, SlotEv, any_balance_colors, percent, unaccounted_sheets, usd,
};

/// Biggest contributors kept per slot before the pack- and product-level lists are cut down
/// to their own limits. Scaling a pack's contributions by its `quantity` is monotone, so the
/// copy's top *N* can only come from each pack's top *N* — keeping a bounded head per slot
/// is enough, and it stops a 300-card common sheet from materialising 300 payloads to
/// display three.
const MAX_ODDS_KEPT: usize = 12;

/// Top contributors on one slot.
const SLOT_TOP: usize = 3;
/// Top contributors on one pack.
const PACK_TOP: usize = 10;
/// Top contributors across one copy of the product.
const PRODUCT_TOP: usize = 12;

/// One card's expected contribution, before it is dressed as a [`PackCardOdds`].
struct Contender {
    card_id: i32,
    /// Expected copies per pack.
    expected: f64,
    price_cents: Option<i128>,
    /// `expected × price`, in cents. Zero for an unpriced card.
    contribution: f64,
}

/// A [`Contender`] plus the sheet it came off, pooled across a pack's slots.
struct Candidate {
    sheet: String,
    foil: bool,
    contender: Contender,
}

/// Sort the biggest contributor first, with total tie-breaks so two runs of the same data
/// produce the same list.
fn rank(a: &Contender, b: &Contender) -> std::cmp::Ordering {
    b.contribution
        .partial_cmp(&a.contribution)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| {
            b.expected
                .partial_cmp(&a.expected)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| a.card_id.cmp(&b.card_id))
}

/// One line of a `top` list, computed but not yet dressed: everything [`PackCardOdds`]
/// carries except the card itself, which is a whole `cards` row and is fetched only for the
/// handful of ids a finished plan names.
struct PlannedOdds {
    card_id: i32,
    foil: bool,
    sheet: String,
    expected_per_pack: f64,
    one_in: f64,
    price_usd: Option<String>,
    contribution_usd: String,
}

impl PlannedOdds {
    /// Attach the catalog payload. `None` when the card row has gone between the loader's
    /// two passes — the line is then dropped, as it was when the row went missing before.
    fn render(self, responses: &CardResponses) -> Option<PackCardOdds> {
        let card = responses.get(&self.card_id)?.clone();
        Some(PackCardOdds {
            card,
            foil: self.foil,
            sheet: self.sheet,
            expected_per_pack: self.expected_per_pack,
            one_in: self.one_in,
            price_usd: self.price_usd,
            contribution_usd: self.contribution_usd,
        })
    }
}

/// Plan a contender's line. `scale` multiplies the *money* only — the product-level list
/// reports a whole copy's contribution while still quoting per-pack odds, which is the unit
/// "one in N packs" is meaningful in.
fn planned(sheet: &str, foil: bool, c: &Contender, scale: f64) -> PlannedOdds {
    PlannedOdds {
        card_id: c.card_id,
        foil,
        sheet: sheet.to_string(),
        expected_per_pack: c.expected,
        // Only ever built for `expected > 0`, so this is finite — the reciprocal of a
        // positive expectation. "One in N packs, on average."
        one_in: 1.0 / c.expected,
        price_usd: c.price_cents.map(|cents| usd(cents as f64)),
        contribution_usd: usd(c.contribution * scale),
    }
}

/// One sheet's line of a planned pack — [`SlotEv`] with its `top` not yet dressed.
struct SlotPlan {
    sheet: String,
    foil: bool,
    picks: f64,
    ev_usd: String,
    card_count: u32,
    priced_share: f64,
    top: Vec<PlannedOdds>,
}

/// One booster's line of a planned copy — [`PackEv`], undressed.
struct PackPlan {
    set_code: String,
    booster_code: String,
    name: Option<String>,
    quantity: u32,
    cards_per_pack: f64,
    ev_usd: String,
    priced_share: f64,
    slots: Vec<SlotPlan>,
    top: Vec<PlannedOdds>,
}

/// A finished expected value that still names its cards by internal id. The handler asks it
/// which cards it needs ([`EvPlan::card_ids`]), fetches exactly those rows, and renders.
pub(super) struct EvPlan {
    ev_usd: String,
    packs: Vec<PackPlan>,
    top: Vec<PlannedOdds>,
    caveats: Vec<String>,
}

impl EvPlan {
    /// Every card this plan will put on the wire, deduplicated, in the order it was
    /// planned. A few dozen ids at most: the `top` lists are capped at [`SLOT_TOP`] per
    /// slot, [`PACK_TOP`] per pack and [`PRODUCT_TOP`] per copy, whatever the sheets
    /// underneath hold.
    pub(super) fn card_ids(&self) -> Vec<i32> {
        let mut seen: HashSet<i32> = HashSet::new();
        let mut ids: Vec<i32> = Vec::new();
        let lines = self.top.iter().chain(self.packs.iter().flat_map(|pack| {
            pack.top
                .iter()
                .chain(pack.slots.iter().flat_map(|slot| slot.top.iter()))
        }));
        for line in lines {
            if seen.insert(line.card_id) {
                ids.push(line.card_id);
            }
        }
        ids
    }

    /// Dress the plan with the catalog rows the second pass loaded.
    pub(super) fn render(self, responses: &CardResponses) -> ProductEv {
        ProductEv {
            ev_usd: self.ev_usd,
            packs: self
                .packs
                .into_iter()
                .map(|pack| PackEv {
                    set_code: pack.set_code,
                    booster_code: pack.booster_code,
                    name: pack.name,
                    quantity: pack.quantity,
                    cards_per_pack: pack.cards_per_pack,
                    ev_usd: pack.ev_usd,
                    priced_share: pack.priced_share,
                    slots: pack
                        .slots
                        .into_iter()
                        .map(|slot| SlotEv {
                            sheet: slot.sheet,
                            foil: slot.foil,
                            picks: slot.picks,
                            ev_usd: slot.ev_usd,
                            card_count: slot.card_count,
                            priced_share: slot.priced_share,
                            top: slot
                                .top
                                .into_iter()
                                .filter_map(|line| line.render(responses))
                                .collect(),
                        })
                        .collect(),
                    top: pack
                        .top
                        .into_iter()
                        .filter_map(|line| line.render(responses))
                        .collect(),
                })
                .collect(),
            top: self
                .top
                .into_iter()
                .filter_map(|line| line.render(responses))
                .collect(),
            caveats: self.caveats,
        }
    }
}

/// What one sheet contributes to an average pack of this configuration.
struct SlotTotals {
    /// Cards this sheet puts in an average pack.
    picks: f64,
    /// Cents this sheet puts in an average pack.
    ev_cents: f64,
    /// `0..1` — the share of `picks` landing on a priced card.
    priced_share: f64,
    /// Distinct catalog cards on the sheet.
    card_count: u32,
    /// Whether any of this sheet's picks really can land on something we can't price.
    ///
    /// Tracked as a **flag** rather than read back off `priced_share < 1.0`: that share is
    /// accumulated card by card in floating point, so a sheet of N equal-weight priced
    /// cards lands on `0.9999999999999984` and a `< 1` test would report a fully-priced
    /// pack as partly unpriced. The number on the wire is snapped ([`snap_share`]) so a
    /// client's own `< 1` test agrees with this flag.
    any_unpriced: bool,
    /// The biggest contributors, already ranked and cut to [`MAX_ODDS_KEPT`].
    contenders: Vec<Contender>,
}

/// How close a computed share has to be to an end of its range to be reported as exactly
/// that end. Far above the drift a few hundred additions accumulate, far below any share a
/// real sheet could express.
const SHARE_EPSILON: f64 = 1e-9;

/// Snap a computed `0..1` share to the ends of its range, and clamp it into them.
///
/// Whether anything is unpriced is a *fact* (the `any_unpriced` flags); this share is a
/// float summed over the cards, and without snapping a fully-priced sheet reports
/// `0.9999999999999984`. Clients test `priced_share < 1` to decide whether to qualify a
/// number, so the two have to agree.
fn snap_share(share: f64) -> f64 {
    if !share.is_finite() {
        return 0.0;
    }
    let clamped = share.clamp(0.0, 1.0);
    if (clamped - 1.0).abs() < SHARE_EPSILON {
        1.0
    } else if clamped.abs() < SHARE_EPSILON {
        0.0
    } else {
        clamped
    }
}

/// Evaluate one sheet against the `(probability, count)` pairs of the variants that name it.
fn slot_totals(sheet: &ResolvedSheet, index: &CardIndex, draws: &[(f64, u32)]) -> SlotTotals {
    let picks: f64 = draws.iter().map(|(p, count)| p * f64::from(*count)).sum();

    // A random sheet's unaccounted weight is picks that land on nothing we can price. On a
    // fixed sheet it is `UNRESOLVED_CARD_ID` positions instead, which only cost a pick when
    // a variant actually reaches them — so that case is judged card by card below, not from
    // the sheet's totals.
    let mut any_unpriced = !sheet.fixed && picks > 0.0 && sheet.unaccounted_share() > 0.0;

    // Expected copies per card, in the sheet's own order. A random sheet spreads `picks`
    // across the weights; a fixed sheet hands out its first `count` cards, so a card's
    // expectation is the probability of a variant deep enough to reach its position. A
    // placeholder position is kept in that count: it is a real card of the pack, just one
    // we can't name, so it consumes its pick and is worth nothing.
    let mut expected: Vec<(i32, f64)> = Vec::new();
    if sheet.fixed {
        for (position, &(card_id, _)) in sheet.cards.iter().enumerate() {
            let reach: f64 = draws
                .iter()
                .filter(|(_, count)| (*count as usize) > position)
                .map(|(p, _)| p)
                .sum();
            if reach > 0.0 {
                accumulate(&mut expected, card_id, reach);
            }
        }
    } else {
        let total = sheet.denominator();
        if total > 0 && picks > 0.0 {
            for &(card_id, weight) in &sheet.cards {
                let share = f64::from(weight) / total as f64;
                if share > 0.0 {
                    accumulate(&mut expected, card_id, picks * share);
                }
            }
        }
    }

    let mut ev_cents = 0.0;
    let mut priced_expected = 0.0;
    let mut contenders: Vec<Contender> = Vec::new();
    for &(card_id, copies) in &expected {
        let price_cents = index.price_cents(card_id, sheet.foil);
        let contribution = copies * price_cents.unwrap_or(0) as f64;
        ev_cents += contribution;
        if price_cents.is_some() {
            priced_expected += copies;
        } else if copies > 0.0 {
            // Either a card with no market price, or a fixed sheet's unnameable position.
            any_unpriced = true;
        }
        // A card that cannot be pulled has no odds to quote, and one worth nothing is
        // never a "top contributor" — both would only be noise in a list of the biggest.
        if copies > 0.0 && contribution > 0.0 {
            contenders.push(Contender {
                card_id,
                expected: copies,
                price_cents,
                contribution,
            });
        }
    }
    contenders.sort_by(rank);
    contenders.truncate(MAX_ODDS_KEPT);

    // The share of the picks that fall on a priced card. For a random sheet that is the
    // priced weight over the *stated* total, so weight sitting on cards the catalog doesn't
    // hold counts against it, exactly as an unpriced card does.
    let priced_share = if picks > 0.0 {
        snap_share(priced_expected / picks)
    } else {
        0.0
    };

    // Cards, so a fixed sheet's placeholder positions don't count as any.
    let mut distinct: Vec<i32> = sheet
        .cards
        .iter()
        .map(|&(id, _)| id)
        .filter(|&id| id != UNRESOLVED_CARD_ID)
        .collect();
    distinct.sort_unstable();
    distinct.dedup();

    SlotTotals {
        picks,
        ev_cents,
        priced_share,
        card_count: distinct.len().min(u32::MAX as usize) as u32,
        any_unpriced,
        contenders,
    }
}

/// Add `copies` to a card's running expectation, keeping first-appearance order — a sheet
/// may list one printing twice (a fixed sheet legitimately does), and it is one card.
fn accumulate(expected: &mut Vec<(i32, f64)>, card_id: i32, copies: f64) {
    if let Some(entry) = expected.iter_mut().find(|(id, _)| *id == card_id) {
        entry.1 += copies;
    } else {
        expected.push((card_id, copies));
    }
}

/// What one booster configuration works out to: its slots, what an average pack of it is
/// worth, and the candidates its `top` lists are drawn from.
struct ConfigTotals {
    slots: Vec<SlotPlan>,
    candidates: Vec<Candidate>,
    cards_per_pack: f64,
    ev_cents: f64,
    priced_share: f64,
    /// Whether any of this pack's picks can land on something we can't price — see
    /// [`SlotTotals::any_unpriced`].
    any_unpriced: bool,
}

/// Evaluate one booster configuration.
fn evaluate_config(config: &ResolvedConfig, index: &CardIndex) -> ConfigTotals {
    let denominator = config.variant_denominator();
    let probability: Vec<f64> = config
        .variants
        .iter()
        .map(|v| {
            if denominator == 0 {
                0.0
            } else {
                v.weight as f64 / denominator as f64
            }
        })
        .collect();

    // Sheets in the order they first appear in the variants. That is *not* the order the
    // pack reads in: the ingest sorts each variant's slots by sheet name, so this is
    // alphabetical within a variant, with each later variant appending whatever sheets it
    // adds. It is deterministic, which is what a stable wire order needs; the SPA labels
    // the slots, so it never depended on them arriving in physical order. A slot naming a
    // sheet the ingest didn't store is skipped.
    let mut order: Vec<&str> = Vec::new();
    for variant in &config.variants {
        for (name, _) in &variant.slots {
            if !order.contains(&name.as_str()) {
                order.push(name.as_str());
            }
        }
    }

    let mut slots: Vec<SlotPlan> = Vec::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut cards_per_pack = 0.0;
    let mut ev_cents = 0.0;
    let mut priced_picks = 0.0;
    let mut any_unpriced = false;

    for name in order {
        let Some(sheet) = config.sheet(name) else {
            continue;
        };
        // How many cards each variant takes off this sheet (a variant naming it twice takes
        // the sum), paired with that variant's probability.
        let draws: Vec<(f64, u32)> = config
            .variants
            .iter()
            .zip(&probability)
            .filter_map(|(variant, p)| {
                let count: u32 = variant
                    .slots
                    .iter()
                    .filter(|(slot, _)| slot == name)
                    .map(|(_, count)| *count)
                    .sum();
                (count > 0).then_some((*p, count))
            })
            .collect();
        if draws.is_empty() {
            continue;
        }

        let totals = slot_totals(sheet, index, &draws);
        cards_per_pack += totals.picks;
        ev_cents += totals.ev_cents;
        priced_picks += totals.picks * totals.priced_share;
        any_unpriced |= totals.any_unpriced;

        slots.push(SlotPlan {
            sheet: sheet.name.clone(),
            foil: sheet.foil,
            picks: totals.picks,
            ev_usd: usd(totals.ev_cents),
            card_count: totals.card_count,
            priced_share: totals.priced_share,
            top: totals
                .contenders
                .iter()
                .take(SLOT_TOP)
                .map(|c| planned(&sheet.name, sheet.foil, c, 1.0))
                .collect(),
        });

        for contender in totals.contenders {
            candidates.push(Candidate {
                sheet: sheet.name.clone(),
                foil: sheet.foil,
                contender,
            });
        }
    }

    let priced_share = if cards_per_pack > 0.0 {
        snap_share(priced_picks / cards_per_pack)
    } else {
        0.0
    };
    ConfigTotals {
        slots,
        candidates,
        cards_per_pack,
        ev_cents,
        priced_share,
        any_unpriced,
    }
}

/// What one copy of the product is worth on average, as a plan that still names its cards
/// by internal id — the whole computation runs on prices alone, and only the handful of
/// cards it decides to quote need a catalog row. The caller has already established that
/// there *are* packs; a product with none has no expected value and answers `null`.
pub(super) fn evaluate(packs: &[ResolvedPack], index: &CardIndex) -> EvPlan {
    let mut wire_packs: Vec<PackPlan> = Vec::with_capacity(packs.len());
    let mut copy_candidates: Vec<(f64, Candidate)> = Vec::new();
    let mut total_cents = 0.0;
    let mut copy_picks = 0.0;
    let mut copy_priced_picks = 0.0;
    let mut any_unpriced = false;

    for pack in packs {
        let ConfigTotals {
            slots,
            candidates,
            cards_per_pack,
            ev_cents,
            priced_share,
            any_unpriced: pack_unpriced,
        } = evaluate_config(&pack.config, index);
        let quantity = f64::from(pack.quantity);
        total_cents += quantity * ev_cents;
        copy_picks += quantity * cards_per_pack;
        copy_priced_picks += quantity * cards_per_pack * priced_share;
        // The *fact* that something is unpriced, not `priced_share < 1.0` — which a
        // fully-priced sheet fails by a rounding error's width.
        any_unpriced |= pack_unpriced;

        let mut ranked: Vec<&Candidate> = candidates.iter().collect();
        ranked.sort_by(|a, b| rank(&a.contender, &b.contender));
        let top = ranked
            .iter()
            .take(PACK_TOP)
            .map(|c| planned(&c.sheet, c.foil, &c.contender, 1.0))
            .collect();

        wire_packs.push(PackPlan {
            set_code: pack.config.set_code.clone(),
            booster_code: pack.config.code.clone(),
            name: pack.config.name.clone(),
            quantity: pack.quantity,
            cards_per_pack,
            ev_usd: usd(ev_cents),
            priced_share,
            slots,
            top,
        });

        for candidate in candidates {
            copy_candidates.push((quantity, candidate));
        }
    }

    // Across the whole copy the money is per copy (× the pack's quantity) while the odds
    // stay per pack — see `ProductEv::top`.
    copy_candidates.sort_by(|(qa, a), (qb, b)| {
        (b.contender.contribution * qb)
            .partial_cmp(&(a.contender.contribution * qa))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| rank(&a.contender, &b.contender))
    });
    let top = copy_candidates
        .iter()
        .take(PRODUCT_TOP)
        .map(|(quantity, c)| planned(&c.sheet, c.foil, &c.contender, *quantity))
        .collect();

    let copy_priced_share = if copy_picks > 0.0 {
        snap_share(copy_priced_picks / copy_picks)
    } else {
        0.0
    };

    EvPlan {
        ev_usd: usd(total_cents),
        packs: wire_packs,
        top,
        caveats: caveats(packs, any_unpriced, copy_priced_share),
    }
}

/// What qualifies the numbers above, in a fixed order — the honest half of the response, and
/// the reason a client may show an EV at all. Each one is emitted only when it applies, so a
/// caveat that *is* there always means something.
fn caveats(packs: &[ResolvedPack], any_unpriced: bool, copy_priced_share: f64) -> Vec<String> {
    let mut out = vec![
        "Expected value is an average over many packs at today's market prices — no single \
         pack is worth this."
            .to_string(),
    ];

    // The with-replacement approximation only needs saying where a sheet is actually drawn
    // without replacement: a fixed sheet isn't drawn at all, and one that allows duplicates
    // really is independent.
    let approximated = packs
        .iter()
        .flat_map(|p| p.config.sheets.iter())
        .any(|s| !s.fixed && !s.allow_duplicates);
    if approximated {
        out.push(
            "Each pick is treated as an independent weighted draw from its sheet; a real \
             sheet isn't drawn with replacement, which only matters on very small sheets."
                .to_string(),
        );
    }

    if any_balance_colors(packs) {
        out.push("Colour balancing of common slots isn't simulated.".to_string());
    }

    if any_unpriced {
        out.push(format!(
            "Cards without a market price count as $0 — {} of the picks fall on priced cards.",
            percent(copy_priced_share)
        ));
    }

    if let Some(sheets) = unaccounted_sheets(packs) {
        out.push(format!(
            "Some cards on these sheets aren't in the catalog and count as $0: {sheets}."
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    /// Compute *and* dress, the way the handler does: the plan names its cards by id, the
    /// second pass turns those ids into payloads. Every assertion below is on the wire
    /// shape, so it pins what a client actually receives.
    fn evaluate_wire(packs: &[ResolvedPack], index: &CardIndex) -> ProductEv {
        let plan = evaluate(packs, index);
        let responses = responses(&plan.card_ids());
        plan.render(&responses)
    }

    /// A configuration small enough to evaluate on paper, and broad enough to exercise every
    /// branch: two variants of different weights, a weighted rare sheet, an unpriced common,
    /// a foil sheet valued at foil prices, and a fixed land sheet only the rarer variant has.
    ///
    /// ```text
    /// W = 4:  v0 (weight 3) = common x2, rare x1, foil x1
    ///         v1 (weight 1) = common x2, rare x1, foil x1, land x2
    /// common  T=4  [card 1 w3 @ $1.00, card 2 w1 unpriced]
    /// rare    T=10 [card 3 w8 @ $2.00, card 4 w2 @ $20.00]
    /// foil    T=4  [card 7 w3 @ $5.00 foil, card 8 w1 foil-unpriced]   (allow_duplicates)
    /// land    fixed [card 5 @ $0.40, card 6 @ $3.00]
    /// ```
    fn fixture() -> (Vec<ResolvedPack>, CardIndex) {
        let prices = [
            (1, Some("1.00"), None),
            (2, None, None),
            (3, Some("2.00"), None),
            (4, Some("20.00"), None),
            (5, Some("0.40"), None),
            (6, Some("3.00"), None),
            // A foil sheet must read the foil price and never fall back to the regular one:
            // card 8 is priced non-foil and unpriced foil.
            (7, Some("1.00"), Some("5.00")),
            (8, Some("10.00"), None),
        ];
        let mut foil_sheet = sheet("foil", true, 4, &[(7, 3), (8, 1)]);
        foil_sheet.allow_duplicates = true;
        let mut land_sheet = sheet("land", false, 2, &[(5, 1), (6, 1)]);
        land_sheet.fixed = true;

        let config = config(
            vec![
                variant(3, &[("common", 2), ("rare", 1), ("foil", 1)]),
                variant(1, &[("common", 2), ("rare", 1), ("foil", 1), ("land", 2)]),
            ],
            vec![
                sheet("common", false, 4, &[(1, 3), (2, 1)]),
                sheet("rare", false, 10, &[(3, 8), (4, 2)]),
                foil_sheet,
                land_sheet,
            ],
        );
        (vec![pack(3, config)], index(&prices))
    }

    #[test]
    fn a_pack_is_worth_the_sum_of_its_slots_to_the_cent() {
        let (packs, index) = fixture();
        let ev = evaluate_wire(&packs, &index);
        let pack = &ev.packs[0];

        // p(v0) = 3/4, p(v1) = 1/4.
        // common: picks 2, $/pick = 3/4 x 100c = 75c  -> 150c
        // rare:   picks 1, $/pick = .8x200 + .2x2000  -> 560c
        // foil:   picks 1, $/pick = 3/4 x 500c        -> 375c
        // land:   fixed, only v1 (p .25) reaches both -> .25x40 + .25x300 = 85c
        assert_eq!(pack.ev_usd, "11.70");
        assert_eq!(
            pack.slots
                .iter()
                .map(|s| (s.sheet.as_str(), s.ev_usd.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("common", "1.50"),
                ("rare", "5.60"),
                ("foil", "3.75"),
                ("land", "0.85"),
            ],
            "slots are listed in the order the sheets first appear in the variants"
        );
        // One copy opens three packs.
        assert_eq!(ev.ev_usd, "35.10");
    }

    #[test]
    fn cards_per_pack_is_an_expectation_over_the_variants() {
        let (packs, index) = fixture();
        let pack = &evaluate_wire(&packs, &index).packs[0];
        // 3/4 x 4 cards + 1/4 x 6 cards.
        assert!((pack.cards_per_pack - 4.5).abs() < 1e-9);
        let picks: Vec<(&str, f64)> = pack
            .slots
            .iter()
            .map(|s| (s.sheet.as_str(), s.picks))
            .collect();
        assert_eq!(picks[0].0, "common");
        assert!((picks[0].1 - 2.0).abs() < 1e-9);
        assert!((picks[3].1 - 0.5).abs() < 1e-9, "land: 1/4 x 2 = 0.5 picks");
        assert!(
            (picks.iter().map(|(_, p)| p).sum::<f64>() - pack.cards_per_pack).abs() < 1e-9,
            "cards per pack is exactly the picks it is made of"
        );
    }

    #[test]
    fn an_unpriced_card_counts_as_zero_and_shows_up_in_the_priced_share() {
        let (packs, index) = fixture();
        let pack = &evaluate_wire(&packs, &index).packs[0];
        let common = &pack.slots[0];
        // Card 2 holds 1 of the sheet's 4 weight and has no price.
        assert!((common.priced_share - 0.75).abs() < 1e-9);
        assert_eq!(common.card_count, 2);
        // The foil sheet's card 8 is priced non-foil but unpriced foil: on a foil sheet
        // that is an unpriced pull, not a $10 one.
        let foil = &pack.slots[2];
        assert!((foil.priced_share - 0.75).abs() < 1e-9);
        assert_eq!(foil.ev_usd, "3.75");
        // 3.75 of 4.5 expected picks land on a priced card.
        assert!((pack.priced_share - 3.75 / 4.5).abs() < 1e-9);
    }

    #[test]
    fn odds_are_one_in_n_packs_and_never_infinite() {
        let (packs, index) = fixture();
        let pack = &evaluate_wire(&packs, &index).packs[0];
        let rare = &pack.slots[1];
        let best = &rare.top[0];
        assert_eq!(best.card.id, "ext-4");
        assert!((best.expected_per_pack - 0.2).abs() < 1e-9);
        assert!((best.one_in - 5.0).abs() < 1e-9, "1 in 5 packs");
        assert_eq!(best.price_usd.as_deref(), Some("20.00"));
        assert_eq!(best.contribution_usd, "4.00");
        assert!(!best.foil);

        for entry in pack.top.iter().chain(rare.top.iter()) {
            assert!(
                entry.one_in.is_finite() && entry.one_in > 0.0,
                "a card that can't be pulled has no odds to quote: {}",
                entry.card.id
            );
        }
        assert!(rare.top.len() <= 3, "a slot lists at most three");
        assert!(pack.top.len() <= 10, "a pack lists at most ten");
    }

    #[test]
    fn the_copys_top_scales_money_by_quantity_but_leaves_the_odds_per_pack() {
        let (packs, index) = fixture();
        let ev = evaluate_wire(&packs, &index);
        let best = &ev.top[0];
        assert_eq!(best.card.id, "ext-4");
        // Per pack: 0.2 x $20 = $4. Per copy (three packs): $12.
        assert!((best.expected_per_pack - 0.2).abs() < 1e-9);
        assert_eq!(best.contribution_usd, "12.00");
        assert!(ev.top.len() <= 12);
        assert_eq!(
            ev.top
                .iter()
                .map(|c| c.card.id.as_str())
                .collect::<Vec<_>>(),
            vec!["ext-4", "ext-7", "ext-3", "ext-1", "ext-6", "ext-5"],
            "biggest contributor first; the two unpriced pulls contribute nothing and are \
             not 'top contributors'"
        );
    }

    #[test]
    fn a_fixed_sheet_hands_out_its_first_cards_in_order() {
        // Two cards, one card taken: only the first is ever dealt, so only it has odds.
        let mut land = sheet("land", false, 2, &[(5, 1), (6, 1)]);
        land.fixed = true;
        let packs = vec![pack(
            1,
            config(vec![variant(1, &[("land", 1)])], vec![land]),
        )];
        let index = index(&[(5, Some("0.40"), None), (6, Some("3.00"), None)]);
        let pack_ev = &evaluate_wire(&packs, &index).packs[0];
        assert_eq!(pack_ev.ev_usd, "0.40", "the second card is never reached");
        assert_eq!(pack_ev.slots[0].top.len(), 1);
        assert_eq!(pack_ev.slots[0].top[0].card.id, "ext-5");
        assert!((pack_ev.slots[0].top[0].expected_per_pack - 1.0).abs() < 1e-9);
        assert!((pack_ev.slots[0].top[0].one_in - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_caveats_are_only_the_ones_that_apply() {
        let (packs, index) = fixture();
        let ev = evaluate_wire(&packs, &index);
        assert_eq!(ev.caveats.len(), 3, "{:?}", ev.caveats);
        assert!(ev.caveats[0].contains("no single pack is worth this"));
        assert!(ev.caveats[1].contains("independent weighted draw"));
        assert!(
            ev.caveats[2].contains("83% of the picks fall on priced cards"),
            "{:?}",
            ev.caveats[2]
        );
        assert!(
            !ev.caveats.iter().any(|c| c.contains("Colour balancing")),
            "nothing here is colour-balanced"
        );
        assert!(
            !ev.caveats
                .iter()
                .any(|c| c.contains("aren't in the catalog")),
            "every sheet is fully accounted for"
        );
    }

    #[test]
    fn colour_balancing_and_missing_weight_each_add_their_own_caveat() {
        let mut common = sheet("common", false, 5, &[(1, 3), (2, 1)]);
        common.balance_colors = true;
        let packs = vec![pack(
            1,
            config(vec![variant(1, &[("common", 1)])], vec![common]),
        )];
        let index = index(&[(1, Some("1.00"), None), (2, Some("1.00"), None)]);
        let ev = evaluate_wire(&packs, &index);
        assert!(ev.caveats.iter().any(|c| c.contains("Colour balancing")));
        assert!(
            ev.caveats
                .iter()
                .any(|c| c.contains("play/common (20% of its weight)")),
            "a fifth of the sheet's weight is on cards we don't hold: {:?}",
            ev.caveats
        );
        // Every stored card is priced, but a fifth of the weight isn't in the catalog, so
        // the pack's priced share is 80% — the two failure modes read the same way.
        assert!((ev.packs[0].priced_share - 0.8).abs() < 1e-9);
    }

    #[test]
    fn a_configuration_with_nothing_to_roll_is_worth_nothing_rather_than_nan() {
        // No variants at all, and a slot naming a sheet that isn't stored.
        let empty = vec![pack(1, config(vec![], vec![]))];
        let ev = evaluate_wire(&empty, &index(&[]));
        assert_eq!(ev.ev_usd, "0.00");
        assert_eq!(ev.packs[0].cards_per_pack, 0.0);
        assert_eq!(ev.packs[0].priced_share, 0.0);
        assert!(ev.packs[0].slots.is_empty());

        let dangling = vec![pack(1, config(vec![variant(1, &[("nope", 3)])], vec![]))];
        let ev = evaluate_wire(&dangling, &index(&[]));
        assert!(
            ev.packs[0].slots.is_empty(),
            "a slot naming a sheet the ingest didn't store is skipped, not guessed at"
        );
        assert_eq!(ev.ev_usd, "0.00");
    }

    #[test]
    fn a_zero_weight_sheet_contributes_nothing_but_still_counts_its_picks() {
        // Every card weighs 0 (a degenerate sheet) — no card can be pulled, and nothing
        // divides by zero.
        let packs = vec![pack(
            1,
            config(
                vec![variant(1, &[("common", 2)])],
                vec![sheet("common", false, 0, &[(1, 0), (2, 0)])],
            ),
        )];
        let index = index(&[(1, Some("1.00"), None), (2, Some("1.00"), None)]);
        let ev = evaluate_wire(&packs, &index);
        assert_eq!(ev.ev_usd, "0.00");
        assert!((ev.packs[0].cards_per_pack - 2.0).abs() < 1e-9);
        assert_eq!(ev.packs[0].slots[0].priced_share, 0.0);
        assert!(ev.packs[0].slots[0].top.is_empty());
    }
}

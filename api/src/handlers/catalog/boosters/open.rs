//! The pack opener: pure, seeded, and stateless. Hand it a product's boosters, a seed and a
//! copy count and it deals the cards — no session table, no expiry, nothing to clean up,
//! because the run is a function of `(product, seed, copies)` and every one of those rides
//! in the URL. That is what lets a surprising box be handed to someone else as a link.
//!
//! It shares its generator with the deck goldfish ([`crate::handlers::shared::rng`]) rather
//! than keeping a second copy, and for the same reason that read has one: a seed is part of
//! the wire contract, and `rand`'s generators explicitly do not promise a stable stream
//! across versions, so a dependency bump would silently invalidate every shared opening.
//!
//! **Pack *n* of an opening is the same pack whatever `copies` was.** Each pack derives its
//! own generator state from `(seed, its ordinal in the whole opening)` — not from a single
//! stream walked pack after pack — so opening six copies replays the one-copy run and then
//! keeps going. Bumping `copies` in the SPA therefore extends a box rather than rerolling
//! it, and the `?pack=&copies=` pair in a shared URL means one thing.
//!
//! Unlike the [`super::ev`] side, the draw is genuinely **without replacement** within a
//! sheet unless upstream marked it `allowDuplicates`: this deals actual cards, where the
//! same printing appearing twice in one slot would be a visible lie rather than a rounding
//! error. Weights are taken over the cards our catalog holds — the sheet's unaccounted
//! weight can't be dealt, only reported — which is the one place a simulated pack differs
//! from a real one, and it is a caveat on every response.
//!
//! The modulo below biases the pick by at most one part in `2^64 / total`, far below
//! anything a print sheet could express, and keeping it makes the algorithm short enough to
//! reimplement in a client that wants to predict a pull.

use crate::error::AppError;
use crate::handlers::shared::rng::split_mix64;
use crate::handlers::shared::valuation::{format_cents, price_cents};

use std::collections::HashSet;

use super::{
    CardIndex, CardResponses, MAX_CARDS_PER_OPENING, MAX_PACKS_PER_OPENING, OpenedCard, OpenedPack,
    PackOpening, ResolvedConfig, ResolvedPack, any_balance_colors, unaccounted_sheets,
};

/// One dealt card, before its catalog payload is fetched: everything [`OpenedCard`] carries
/// except the card itself. An opening deals at most [`MAX_CARDS_PER_OPENING`] of these and a
/// fresh seed on every click, so fetching whole `cards` rows for the sheets it drew *from*
/// — thousands of them — to render a few dozen would be the cost of every roll.
struct DealtCard {
    card_id: i32,
    foil: bool,
    sheet: String,
    price_usd: Option<String>,
}

impl DealtCard {
    /// Attach the catalog payload. `None` when the card row has gone between the loader's
    /// two passes; the card then simply isn't in the pack, as before.
    fn render(self, responses: &CardResponses) -> Option<OpenedCard> {
        let card = responses.get(&self.card_id)?.clone();
        Some(OpenedCard {
            card,
            foil: self.foil,
            sheet: self.sheet,
            price_usd: self.price_usd,
        })
    }
}

/// One dealt pack — [`OpenedPack`], undressed.
struct PlannedPack {
    set_code: String,
    booster_code: String,
    name: Option<String>,
    variant: u32,
    cards: Vec<DealtCard>,
    value_usd: String,
}

/// A finished opening that still names its cards by internal id. The handler asks it which
/// cards it dealt ([`OpeningPlan::card_ids`]), fetches exactly those rows, and renders.
pub(super) struct OpeningPlan {
    seed: u32,
    copies: u32,
    packs: Vec<PlannedPack>,
    value_usd: String,
    priced_count: u32,
    unpriced_count: u32,
    caveats: Vec<String>,
}

impl OpeningPlan {
    /// Every distinct card this opening dealt, in the order it was dealt — bounded by
    /// [`MAX_CARDS_PER_OPENING`], and usually far below it once the duplicates a box deals
    /// across its packs fold together.
    pub(super) fn card_ids(&self) -> Vec<i32> {
        let mut seen: HashSet<i32> = HashSet::new();
        let mut ids: Vec<i32> = Vec::new();
        for card in self.packs.iter().flat_map(|pack| pack.cards.iter()) {
            if seen.insert(card.card_id) {
                ids.push(card.card_id);
            }
        }
        ids
    }

    /// Dress the opening with the catalog rows the second pass loaded.
    pub(super) fn render(self, responses: &CardResponses) -> PackOpening {
        PackOpening {
            seed: self.seed,
            copies: self.copies,
            packs: self
                .packs
                .into_iter()
                .map(|pack| OpenedPack {
                    set_code: pack.set_code,
                    booster_code: pack.booster_code,
                    name: pack.name,
                    variant: pack.variant,
                    cards: pack
                        .cards
                        .into_iter()
                        .filter_map(|card| card.render(responses))
                        .collect(),
                    value_usd: pack.value_usd,
                })
                .collect(),
            value_usd: self.value_usd,
            priced_count: self.priced_count,
            unpriced_count: self.unpriced_count,
            caveats: self.caveats,
        }
    }
}

/// The generator state for the pack at ordinal `i` of the whole opening. Deriving it per
/// pack rather than walking one stream is what makes an opening prefix-stable across
/// `copies` — see the module docs.
fn pack_state(seed: u32, ordinal: u32) -> u64 {
    (u64::from(seed) << 32) | u64::from(ordinal)
}

/// Walk a cumulative weight table for `roll % total` and return the index it lands on.
/// Falls back to the last entry carrying weight, so a `total` larger than the entries sum to
/// (a stated total the stored rows don't reach) still lands somewhere real instead of
/// dealing nothing.
fn cumulative_pick<I: Iterator<Item = u64>>(weights: I, total: u64, roll: u64) -> Option<usize> {
    if total == 0 {
        return None;
    }
    let target = roll % total;
    let mut acc = 0u64;
    let mut last = None;
    for (index, weight) in weights.enumerate() {
        if weight > 0 {
            last = Some(index);
        }
        acc = acc.saturating_add(weight);
        if target < acc {
            return Some(index);
        }
    }
    last
}

/// The most cards a single pack of this configuration could ever hold — its fattest variant.
/// Used to bound an opening *before* anything is drawn, since both a slot count and a
/// product's `quantity` come from ingested data.
fn largest_variant(config: &ResolvedConfig) -> u64 {
    config
        .variants
        .iter()
        .map(|v| {
            v.slots
                .iter()
                .map(|(_, count)| u64::from(*count))
                .sum::<u64>()
        })
        .max()
        .unwrap_or(0)
}

/// Deal one pack, appending its cards. Returns the index of the variant it rolled.
fn open_one(config: &ResolvedConfig, index: &CardIndex, state: &mut u64) -> (u32, Vec<DealtCard>) {
    let mut cards = Vec::new();
    let denominator = config.variant_denominator();
    if config.variants.is_empty() || denominator == 0 {
        // Nothing to roll: the pack opens empty rather than the read failing. A
        // configuration in this state is a bug upstream, not a bad request.
        return (0, cards);
    }

    let roll = split_mix64(state);
    let variant_index =
        cumulative_pick(config.variants.iter().map(|v| v.weight), denominator, roll).unwrap_or(0);
    let Some(variant) = config.variants.get(variant_index) else {
        return (0, cards);
    };

    for (name, count) in &variant.slots {
        let Some(sheet) = config.sheet(name) else {
            // A slot naming a sheet the ingest didn't store deals nothing — the same
            // tolerance the EV takes, rather than failing a public read on stale data.
            continue;
        };
        if sheet.fixed {
            // A fixed sheet is a list, not a pool: take its first `count` cards in order.
            for &(card_id, _) in sheet.cards.iter().take(*count as usize) {
                push_card(&mut cards, index, card_id, sheet.foil, &sheet.name);
            }
            continue;
        }

        // The pool this instance can actually deal: the stored cards and their weights. The
        // sheet's unaccounted weight is not dealable, only reportable.
        let mut pool: Vec<(i32, u32)> = sheet.cards.clone();
        let mut total: u64 = pool.iter().map(|&(_, w)| u64::from(w)).sum();
        for _ in 0..*count {
            if pool.is_empty() || total == 0 {
                break;
            }
            let roll = split_mix64(state);
            let Some(at) = cumulative_pick(pool.iter().map(|&(_, w)| u64::from(w)), total, roll)
            else {
                break;
            };
            let (card_id, weight) = pool[at];
            push_card(&mut cards, index, card_id, sheet.foil, &sheet.name);
            if !sheet.allow_duplicates {
                total = total.saturating_sub(u64::from(weight));
                pool.swap_remove(at);
            }
        }
    }

    (variant_index.min(u32::MAX as usize) as u32, cards)
}

/// Append one dealt card, priced off the sheet's finish. Its catalog payload is fetched
/// later, for the ids the finished plan names.
fn push_card(cards: &mut Vec<DealtCard>, index: &CardIndex, card_id: i32, foil: bool, sheet: &str) {
    cards.push(DealtCard {
        card_id,
        foil,
        sheet: sheet.to_string(),
        price_usd: index.price_cents(card_id, foil).map(format_cents),
    });
}

/// Open `copies` of a product's boosters with `seed`.
///
/// Refuses, as a `422`, before drawing anything: a product with no booster data, a request
/// for no copies at all, an opening of more than [`MAX_PACKS_PER_OPENING`] packs, and one
/// that would deal more than [`MAX_CARDS_PER_OPENING`] cards (measured off each pack's
/// fattest variant). The pack count alone isn't a bound — a `quantity` and a slot count are
/// both ingested data — so the card bound is the one that actually caps the response.
pub(super) fn open_packs(
    packs: &[ResolvedPack],
    index: &CardIndex,
    seed: u32,
    copies: u32,
) -> Result<OpeningPlan, AppError> {
    if packs.is_empty() {
        return Err(AppError::Validation(
            "this product has no booster data to open".to_string(),
        ));
    }
    if copies == 0 {
        return Err(AppError::Validation(
            "copies must be at least 1".to_string(),
        ));
    }

    let per_copy: u64 = packs.iter().map(|p| u64::from(p.quantity)).sum();
    let total_packs = per_copy.saturating_mul(u64::from(copies));
    if total_packs == 0 {
        return Err(AppError::Validation(
            "this product has no booster data to open".to_string(),
        ));
    }
    if total_packs > u64::from(MAX_PACKS_PER_OPENING) {
        return Err(AppError::Validation(format!(
            "opening {total_packs} packs at once is too many; the limit is {MAX_PACKS_PER_OPENING}"
        )));
    }

    let per_copy_cards: u64 = packs
        .iter()
        .map(|p| largest_variant(&p.config).saturating_mul(u64::from(p.quantity)))
        .sum();
    let total_cards = per_copy_cards.saturating_mul(u64::from(copies));
    if total_cards > MAX_CARDS_PER_OPENING {
        return Err(AppError::Validation(format!(
            "opening {copies} of this product would deal up to {total_cards} cards; \
             the limit is {MAX_CARDS_PER_OPENING}"
        )));
    }

    let mut opened: Vec<PlannedPack> = Vec::new();
    let mut ordinal: u32 = 0;
    let mut value_cents: i128 = 0;
    let mut priced_count: u32 = 0;
    let mut unpriced_count: u32 = 0;

    // Copies outermost, and within a copy the product's own pack order (`sealed_packs`,
    // configuration id ascending): that is what makes the first `per_copy` packs of a
    // six-copy opening identical to the whole of a one-copy opening.
    for _ in 0..copies {
        for pack in packs {
            for _ in 0..pack.quantity {
                let mut state = pack_state(seed, ordinal);
                // Warm the state once so a low seed doesn't start from a near-zero mix —
                // the goldfish's rule, and part of this read's wire contract too.
                let _ = split_mix64(&mut state);
                let (variant, cards) = open_one(&pack.config, index, &mut state);

                let mut pack_cents: i128 = 0;
                for card in &cards {
                    match price_cents(card.price_usd.as_deref()) {
                        Some(cents) => {
                            pack_cents += cents;
                            priced_count = priced_count.saturating_add(1);
                        }
                        None => unpriced_count = unpriced_count.saturating_add(1),
                    }
                }
                value_cents += pack_cents;

                opened.push(PlannedPack {
                    set_code: pack.config.set_code.clone(),
                    booster_code: pack.config.code.clone(),
                    name: pack.config.name.clone(),
                    variant,
                    cards,
                    value_usd: format_cents(pack_cents),
                });
                ordinal = ordinal.saturating_add(1);
            }
        }
    }

    Ok(OpeningPlan {
        seed,
        copies,
        packs: opened,
        value_usd: format_cents(value_cents),
        priced_count,
        unpriced_count,
        caveats: caveats(packs, unpriced_count),
    })
}

/// What qualifies this run. The first one is the whole point: an opening is a sample, and
/// the number at the bottom of it is what *this* roll dealt.
fn caveats(packs: &[ResolvedPack], unpriced_count: u32) -> Vec<String> {
    let mut out = vec![
        "This is one simulated opening at today's prices — a roll of the dice, not what \
         these packs are worth on average."
            .to_string(),
    ];
    if any_balance_colors(packs) {
        out.push("Colour balancing of common slots isn't simulated.".to_string());
    }
    if let Some(sheets) = unaccounted_sheets(packs) {
        out.push(format!(
            "Some cards on these sheets aren't in the catalog and can't be dealt here: {sheets}."
        ));
    }
    if unpriced_count > 0 {
        out.push(format!(
            "{unpriced_count} pulled card(s) have no market price and count as $0."
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    /// Eight priced cards, ids 1..=8 — prices only, which is all the draw reads.
    fn priced() -> Vec<(i32, Option<&'static str>, Option<&'static str>)> {
        (1..=8).map(|id| (id, Some("1.00"), Some("5.00"))).collect()
    }

    fn cards() -> CardIndex {
        index(&priced())
    }

    /// Deal *and* dress, the way the handler does: the plan names the cards it dealt by id,
    /// the second pass turns those ids into payloads. Every assertion below is on the wire
    /// shape, so it pins what a client actually receives.
    fn open_wire(
        packs: &[ResolvedPack],
        index: &CardIndex,
        seed: u32,
        copies: u32,
    ) -> Result<PackOpening, AppError> {
        let plan = open_packs(packs, index, seed, copies)?;
        let responses = responses(&plan.card_ids());
        Ok(plan.render(&responses))
    }

    /// One booster: a single variant taking three commons off a six-card sheet plus one
    /// fixed land, so the draw, the order and the duplicate rule are all observable.
    fn one_booster(quantity: u32) -> Vec<ResolvedPack> {
        let mut land = sheet("land", false, 2, &[(7, 1), (8, 1)]);
        land.fixed = true;
        vec![pack(
            quantity,
            config(
                vec![variant(1, &[("common", 3), ("land", 1)])],
                vec![
                    sheet(
                        "common",
                        false,
                        6,
                        &[(1, 1), (2, 1), (3, 1), (4, 1), (5, 1), (6, 1)],
                    ),
                    land,
                ],
            ),
        )]
    }

    fn dealt(opening: &PackOpening) -> Vec<Vec<&str>> {
        opening
            .packs
            .iter()
            .map(|p| p.cards.iter().map(|c| c.card.id.as_str()).collect())
            .collect()
    }

    #[test]
    fn the_same_seed_deals_the_same_cards() {
        let packs = one_booster(4);
        let index = cards();
        let a = open_wire(&packs, &index, 7, 1).expect("opens");
        let b = open_wire(&packs, &index, 7, 1).expect("opens");
        assert_eq!(dealt(&a), dealt(&b));
        assert_eq!(a.value_usd, b.value_usd);
        assert_eq!(a.seed, 7);
    }

    #[test]
    fn different_seeds_deal_different_cards() {
        let packs = one_booster(1);
        let index = cards();
        let runs: std::collections::HashSet<Vec<Vec<String>>> = (1..=20u32)
            .map(|seed| {
                open_wire(&packs, &index, seed, 1)
                    .expect("opens")
                    .packs
                    .iter()
                    .map(|p| p.cards.iter().map(|c| c.card.id.clone()).collect())
                    .collect()
            })
            .collect();
        assert!(
            runs.len() > 1,
            "twenty seeds must not all deal the same pack"
        );
    }

    #[test]
    fn opening_more_copies_extends_the_same_run() {
        let packs = one_booster(3);
        let index = cards();
        let one = open_wire(&packs, &index, 42, 1).expect("opens");
        let four = open_wire(&packs, &index, 42, 4).expect("opens");
        assert_eq!(one.packs.len(), 3);
        assert_eq!(four.packs.len(), 12);
        assert_eq!(
            dealt(&one),
            dealt(&four)[..3].to_vec(),
            "pack n is the same pack whatever `copies` was"
        );
    }

    #[test]
    fn a_slot_never_repeats_a_card_unless_the_sheet_allows_it() {
        let packs = one_booster(1);
        let index = cards();
        for seed in 0..50u32 {
            let opening = open_wire(&packs, &index, seed, 1).expect("opens");
            let commons: Vec<&str> = opening.packs[0]
                .cards
                .iter()
                .filter(|c| c.sheet == "common")
                .map(|c| c.card.id.as_str())
                .collect();
            let mut unique = commons.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(
                commons.len(),
                3,
                "the slot deals its full count (seed {seed})"
            );
            assert_eq!(unique.len(), 3, "without replacement (seed {seed})");
        }
    }

    #[test]
    fn a_duplicates_sheet_may_repeat_and_still_deals_its_count() {
        // A two-card sheet asked for four cards: only possible with replacement.
        let mut foil = sheet("foil", true, 2, &[(1, 1), (2, 1)]);
        foil.allow_duplicates = true;
        let packs = vec![pack(
            1,
            config(vec![variant(1, &[("foil", 4)])], vec![foil]),
        )];
        let index = cards();
        let opening = open_wire(&packs, &index, 3, 1).expect("opens");
        assert_eq!(opening.packs[0].cards.len(), 4);
        assert!(
            opening.packs[0].cards.iter().all(|c| c.foil),
            "a foil sheet deals foils, priced at the foil price"
        );
        assert_eq!(opening.packs[0].cards[0].price_usd.as_deref(), Some("5.00"));

        // Without replacement the same sheet runs dry after its two cards.
        let mut once = sheet("foil", true, 2, &[(1, 1), (2, 1)]);
        once.fixed = false;
        let packs = vec![pack(
            1,
            config(vec![variant(1, &[("foil", 4)])], vec![once]),
        )];
        let opening = open_wire(&packs, &index, 3, 1).expect("opens");
        assert_eq!(
            opening.packs[0].cards.len(),
            2,
            "a sheet without duplicates deals what it has and stops"
        );
    }

    #[test]
    fn a_fixed_sheet_deals_its_first_cards_in_stored_order() {
        let mut land = sheet("land", false, 3, &[(5, 1), (6, 1), (7, 1)]);
        land.fixed = true;
        let packs = vec![pack(
            1,
            config(vec![variant(1, &[("land", 2)])], vec![land]),
        )];
        let index = cards();
        for seed in [0u32, 1, 99, 12345] {
            let opening = open_wire(&packs, &index, seed, 1).expect("opens");
            assert_eq!(
                opening.packs[0]
                    .cards
                    .iter()
                    .map(|c| c.card.id.as_str())
                    .collect::<Vec<_>>(),
                vec!["ext-5", "ext-6"],
                "a fixed sheet is a list, not a pool (seed {seed})"
            );
        }
    }

    #[test]
    fn the_rolled_variant_is_reported_and_follows_the_weights() {
        // A configuration that is 99-to-1 the first variant: over a handful of seeds the
        // reported index must be a real variant index, and the heavy one must dominate.
        let packs = vec![pack(
            1,
            config(
                vec![variant(99, &[("common", 1)]), variant(1, &[("land", 1)])],
                vec![
                    sheet("common", false, 2, &[(1, 1), (2, 1)]),
                    sheet("land", false, 1, &[(7, 1)]),
                ],
            ),
        )];
        let index = cards();
        let mut heavy = 0;
        for seed in 0..40u32 {
            let opening = open_wire(&packs, &index, seed, 1).expect("opens");
            let variant = opening.packs[0].variant;
            assert!(variant < 2, "a real variant index");
            if variant == 0 {
                heavy += 1;
            }
        }
        assert!(
            heavy > 30,
            "the 99-weight variant should dominate: {heavy}/40"
        );
    }

    #[test]
    fn the_value_is_what_this_run_dealt_and_unpriced_pulls_count_as_zero() {
        let mut prices = priced();
        // Card 3 has no price at all.
        prices[2] = (3, None, None);
        let packs = one_booster(1);
        let index = index(&prices);
        let opening = open_wire(&packs, &index, 11, 1).expect("opens");
        let priced: i128 = opening.packs[0]
            .cards
            .iter()
            .filter_map(|c| c.price_usd.as_deref())
            .map(|p| price_cents(Some(p)).unwrap_or(0))
            .sum();
        assert_eq!(opening.value_usd, format_cents(priced));
        assert_eq!(
            opening.priced_count + opening.unpriced_count,
            opening.packs[0].cards.len() as u32
        );
        if opening.unpriced_count > 0 {
            assert!(
                opening
                    .caveats
                    .iter()
                    .any(|c| c.contains("no market price and count as $0")),
                "{:?}",
                opening.caveats
            );
        }
    }

    #[test]
    fn a_configuration_with_nothing_to_roll_opens_empty_rather_than_failing() {
        let packs = vec![pack(1, config(vec![], vec![]))];
        let opening = open_wire(&packs, &cards(), 5, 1).expect("opens");
        assert_eq!(opening.packs.len(), 1);
        assert_eq!(opening.packs[0].variant, 0);
        assert!(opening.packs[0].cards.is_empty());
        assert_eq!(opening.value_usd, "0.00");

        // A slot naming a sheet the ingest didn't store, and a sheet whose every card
        // weighs nothing: both deal nothing, neither divides by zero.
        let packs = vec![pack(
            1,
            config(
                vec![variant(1, &[("nope", 2), ("zero", 2)])],
                vec![sheet("zero", false, 0, &[(1, 0), (2, 0)])],
            ),
        )];
        let opening = open_wire(&packs, &cards(), 5, 1).expect("opens");
        assert!(opening.packs[0].cards.is_empty());
    }

    #[test]
    fn the_bounds_are_refused_before_anything_is_drawn() {
        let index = cards();

        // No booster data at all.
        let err = open_wire(&[], &index, 1, 1).expect_err("no data");
        assert!(matches!(&err, AppError::Validation(m) if m.contains("no booster data")));

        // Zero copies.
        let packs = one_booster(1);
        let err = open_wire(&packs, &index, 1, 0).expect_err("zero copies");
        assert!(matches!(&err, AppError::Validation(m) if m.contains("at least 1")));

        // A booster box is exactly the pack limit; a case of two isn't.
        let box_of_36 = one_booster(MAX_PACKS_PER_OPENING);
        assert!(open_wire(&box_of_36, &index, 1, 1).is_ok());
        let err = open_wire(&box_of_36, &index, 1, 2).expect_err("too many packs");
        assert!(
            matches!(&err, AppError::Validation(m) if m.contains("packs at once is too many")),
            "{err:?}"
        );

        // Under the pack limit, but each pack claims 400 cards: the card bound catches it.
        let fat = vec![pack(
            4,
            config(
                vec![variant(1, &[("common", 400)])],
                vec![sheet("common", false, 6, &[(1, 1), (2, 1)])],
            ),
        )];
        let err = open_wire(&fat, &index, 1, 1).expect_err("too many cards");
        assert!(
            matches!(&err, AppError::Validation(m) if m.contains("the limit is 1200")),
            "{err:?}"
        );
    }
}

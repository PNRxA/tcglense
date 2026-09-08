//! **Where the money is** — the deck's value broken down per card, and what each card
//! would cost as its cheapest printing (issue #672).
//!
//! A deck has had exactly one money number until now: `summary.total_value_usd`, the
//! shared valuation folded over the deck proper. This read keeps that number — the total
//! here *is* that fold, run over the same rows through the same [`Valuation`], so the two
//! can never disagree — and adds the two questions a player asks once they've seen it:
//! *which cards are the $80*, and *how much of it is the printing rather than the card*.
//!
//! **The cheapest printing is judged at the line's own finish split.** A deck row holds
//! `quantity` regular copies and `foil_quantity` foil ones, and the printing swap
//! (`PUT …/cards/{id}/printing`) preserves exactly that split. So "cheapest" means the
//! printing that costs least *held the way this row is held* — its regular price times the
//! regular copies plus its foil price times the foil ones — never the cheapest single copy
//! of any finish, which is the drops handler's question (`load_cheapest_by_oracle`) and a
//! different one: a printing whose foil is cheap but whose nonfoil is dear would be named
//! "cheapest" for a nonfoil row, and the swap would then make the deck *dearer*. A printing
//! unpriced in a finish the row holds is not a candidate at all — nothing may be called
//! cheaper on a price that isn't known.
//!
//! Three more rules, each keeping the numbers honest:
//!
//! * **A saving is stated only when both sides are known.** `saving_usd` is the held cost
//!   minus the cheapest cost, and it exists only when the held printing is itself priced in
//!   every finish the row holds — otherwise the held side is a floor, and a saving computed
//!   from it could be wrong in either direction. A line's `cheapest` can still be reported
//!   then (it *is* the cheapest priced printing), it just carries no saving.
//! * **`null` is "unpriced", never `$0.00`.** A line with nothing priced has a null price;
//!   a deck with nothing priced has null totals — the valuation's own line. A saving of
//!   `"0.00"` is real (the row already holds the cheapest printing) and is stated.
//! * **The totals are coherent by construction.** `cheapest_total_usd` is `total_usd` minus
//!   `saving_usd`: what the deck would be worth after every line with a known saving were
//!   swapped, so the three numbers always add up, and a line whose saving is unknown moves
//!   none of them.
//!
//! Folded foil-★ variants are never candidates (the shared seam excludes them): the star's
//! price is already copied onto its base, so it could only ever *tie* — and a swap that won
//! the tie would land on a printing no card grid shows. Scoped to the **deck proper**
//! (issue #570): a card under consideration in a maybeboard isn't money in the deck.

use std::collections::{HashMap, HashSet};

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Serialize;

use crate::entities::card;
use crate::entities::prelude::Card;
use crate::error::AppError;
use crate::handlers::shared::valuation::{Valuation, format_cents, price_cents};
use crate::handlers::shared::{CardResponse, PricedPrinting, priced_printings_by_oracle};
use crate::state::AppState;

use super::DeckAnalysisInput;

/// Card ids per `WHERE id IN (…)` lookup when the winning printings are loaded for the
/// wire — the same bound the token read and the cheapest seam chunk at.
const RESOLVE_CHUNK: usize = 900;

// ---------- Wire types ----------

/// The cheapest printing of one line's card, held the way the line is held.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckCheapestPrinting {
    /// The printing — the full card payload, so a client can show it and hand its `id` to
    /// the printing swap. It is the line's own printing when nothing cheaper is priced.
    pub card: CardResponse,
    /// What the line would cost as this printing: its regular price × the regular copies
    /// plus its foil price × the foil copies, 2-dp USD. Always priced — a printing unpriced
    /// in a finish the line holds is never called cheapest.
    pub price_usd: String,
}

/// One deck row — a printing in a section — priced.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckPricingLine {
    /// The printing the deck holds.
    pub card: CardResponse,
    /// The section it sits in — what the printing swap needs, since the same printing in two
    /// sections is two rows.
    pub section_id: i32,
    pub quantity: i32,
    pub foil_quantity: i32,
    /// What the row is worth as held: regular copies at the card's `usd`, foil copies at its
    /// `usd_foil` — the same arithmetic as `summary.total_value_usd`, so the lines sum to it.
    /// `null` when no finish the row holds copies of is priced (a nonfoil copy of a
    /// foil-only printing is unpriced, never `"0.00"`).
    pub price_usd: Option<String>,
    /// The cheapest priced printing of this card at this row's finish split, or `null` when
    /// no printing of it is priced in every finish the row holds.
    pub cheapest: Option<DeckCheapestPrinting>,
    /// `price_usd` minus `cheapest.price_usd`, 2-dp USD — `"0.00"` when the row already
    /// holds the cheapest printing. `null` when either side is unknown: no priced cheapest,
    /// or a held printing unpriced in a finish the row holds.
    pub saving_usd: Option<String>,
}

/// The deck's money, line by line.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckPricing {
    /// Every row of the deck proper, **most expensive first** (unpriced rows last, then by
    /// name), so the top-N is a prefix. Maybeboard rows are not listed.
    pub lines: Vec<DeckPricingLine>,
    /// The deck's value as held — identical to `summary.total_value_usd`. `null` when
    /// nothing in the deck is priced.
    pub total_usd: Option<String>,
    /// `total_usd` minus `saving_usd`: the deck's value if every line with a known saving
    /// were swapped to its cheapest printing. `null` whenever `total_usd` is.
    pub cheapest_total_usd: Option<String>,
    /// The sum of every line's known saving — `"0.00"` when every priced row already holds
    /// its cheapest printing. `null` whenever `total_usd` is.
    pub saving_usd: Option<String>,
    /// Rows whose held printing has no price in either finish. While non-zero, `total_usd`
    /// is a floor.
    pub unpriced_count: i64,
    /// Rows a swap would save money on (`saving_usd` > 0) — what "swap all" would touch.
    pub swappable_count: i64,
}

// ---------- The fold ----------

/// One line's answer before the winning printing is loaded for the wire: which printing is
/// cheapest (by internal id) and at what cost, plus the held cost and the saving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LineQuote {
    held_cents: Option<i128>,
    /// `(internal card id, cost in cents)` of the cheapest candidate at the row's split.
    cheapest: Option<(i32, i128)>,
    saving_cents: Option<i128>,
}

/// What a printing costs held as `quantity` regular + `foil_quantity` foil copies, or
/// `None` when it is unpriced in a finish the row actually holds.
fn cost_at(printing: &PricedPrinting, quantity: i32, foil_quantity: i32) -> Option<i128> {
    let mut total: i128 = 0;
    if quantity > 0 {
        total += printing.usd? * i128::from(quantity);
    }
    if foil_quantity > 0 {
        total += printing.usd_foil? * i128::from(foil_quantity);
    }
    Some(total)
}

/// What the row is worth as held, over the finishes it **actually holds**: each held finish
/// that is priced contributes `price × copies`, and the result is `None` only when no held
/// finish is priced. Deliberately not the shared [`Valuation`]'s `any_priced`, which flips
/// on a parseable price string even at zero copies — right for a deck total (a row worth
/// nothing contributes nothing either way), wrong for a per-row price, where a nonfoil copy
/// of a foil-only printing would be published as `"0.00"` rather than unpriced. The
/// deck-wide total still goes through the shared fold, so it stays the summary's number.
fn held_cost(held: &PricedPrinting, quantity: i32, foil_quantity: i32) -> Option<i128> {
    let mut total: i128 = 0;
    let mut priced = false;
    if quantity > 0
        && let Some(cents) = held.usd
    {
        total += cents * i128::from(quantity);
        priced = true;
    }
    if foil_quantity > 0
        && let Some(cents) = held.usd_foil
    {
        total += cents * i128::from(foil_quantity);
        priced = true;
    }
    priced.then_some(total)
}

/// Quote one row. `held` is the row's own printing; `held_cents` is [`held_cost`]'s answer
/// (priced held finishes only — `None` when none is); `candidates` are
/// every priced, non-folded printing of the card (the held one among them when it
/// qualifies). Ties go to the held printing, then to the lowest internal id, so the answer
/// is stable across requests and a row already holding the cheapest is never told to swap.
fn quote_line(
    held: &PricedPrinting,
    held_cents: Option<i128>,
    quantity: i32,
    foil_quantity: i32,
    candidates: &[PricedPrinting],
) -> LineQuote {
    let held_full = cost_at(held, quantity, foil_quantity);
    let cheapest = candidates
        .iter()
        .filter_map(|candidate| {
            cost_at(candidate, quantity, foil_quantity).map(|cost| (candidate.id, cost))
        })
        .min_by_key(|&(id, cost)| (cost, id != held.id, id));
    // A saving needs both sides known: a held printing priced in every finish the row
    // holds is itself a candidate, so the cheapest can't cost more than it.
    let saving_cents = match (held_full, cheapest) {
        (Some(held_cost), Some((_, cheapest_cost))) => Some((held_cost - cheapest_cost).max(0)),
        _ => None,
    };
    LineQuote {
        held_cents,
        cheapest,
        saving_cents,
    }
}

/// The deck-wide numbers off the quoted lines: the valuation's own total (passed in, so it
/// is the exact `summary` fold), the summed known savings, and the two counts.
struct Totals {
    total: Option<i128>,
    saving: Option<i128>,
    unpriced_count: i64,
    swappable_count: i64,
}

fn fold_totals(total: Option<i128>, quotes: &[LineQuote]) -> Totals {
    let saving_sum: i128 = quotes.iter().filter_map(|q| q.saving_cents).sum();
    let unpriced_count = quotes.iter().filter(|q| q.held_cents.is_none()).count() as i64;
    let swappable_count = quotes
        .iter()
        .filter(|q| q.saving_cents.is_some_and(|cents| cents > 0))
        .count() as i64;
    Totals {
        total,
        // Gated on the total the way `bulk_usd` is: a priced deck with nothing to save
        // reports `"0.00"`, an unpriced deck reports nothing at all.
        saving: total.map(|_| saving_sum),
        unpriced_count,
        swappable_count,
    }
}

// ---------- The read ----------

/// Price a deck's rows and find each one's cheapest printing.
///
/// `input` and `models` are the parallel pair `load_analysis_with_cards` returns — the
/// entries for the section split and the counts, the catalog rows for the prices and the
/// wire payloads. Three bounded queries: the priced printings of every identity in the deck
/// (one chunked lookup through the shared seam), then the winning printings that aren't the
/// held ones, loaded by id for the wire. Nothing here goes per copy.
pub(crate) async fn analyse_pricing(
    state: &AppState,
    game: &str,
    input: &DeckAnalysisInput,
    models: &[card::Model],
) -> Result<DeckPricing, AppError> {
    let maybeboard: HashSet<i32> = input
        .sections
        .iter()
        .filter(|s| s.is_maybeboard)
        .map(|s| s.id)
        .collect();
    let rows: Vec<(&super::AnalysisEntry, &card::Model)> = input
        .entries
        .iter()
        .zip(models)
        .filter(|(entry, _)| !maybeboard.contains(&entry.section_id))
        .collect();

    // Every identity in the deck, looked up once. A card with no `oracle_id` has no key to
    // find siblings by (the `/prints` rule), so it can only be its own cheapest printing.
    let oracle_ids: HashSet<&str> = rows
        .iter()
        .filter_map(|(_, model)| model.oracle_id.as_deref().filter(|id| !id.is_empty()))
        .collect();
    let priced = priced_printings_by_oracle(&state.db, game, &oracle_ids).await?;

    // The deck's own total, through the very fold `summary.total_value_usd` uses.
    let mut valuation = Valuation::default();
    let mut quotes: Vec<LineQuote> = Vec::with_capacity(rows.len());
    for (entry, model) in &rows {
        let usd = model.price_usd.as_deref();
        let usd_foil = model.price_usd_foil.as_deref();
        valuation.add(usd, entry.quantity, usd_foil, entry.foil_quantity);

        let held = PricedPrinting {
            id: model.id,
            usd: price_cents(usd),
            usd_foil: price_cents(usd_foil),
        };
        let held_cents = held_cost(&held, entry.quantity, entry.foil_quantity);

        // The card's priced siblings, plus the held printing itself whenever the seam
        // didn't return it — a card with no `oracle_id` (no key to find siblings by), or a
        // held printing that is a folded foil-★ (excluded there so it can never *win*, but
        // still the row's own cost to compare against).
        let mut candidates: Vec<PricedPrinting> = model
            .oracle_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .and_then(|id| priced.get(id))
            .cloned()
            .unwrap_or_default();
        if !candidates.iter().any(|c| c.id == held.id) {
            candidates.push(held);
        }
        quotes.push(quote_line(
            &held,
            held_cents,
            entry.quantity,
            entry.foil_quantity,
            &candidates,
        ));
    }

    // The winning printings that aren't the held row itself, loaded once for the wire.
    let mut wanted: Vec<i32> = quotes
        .iter()
        .zip(&rows)
        .filter_map(|(quote, (_, model))| {
            quote
                .cheapest
                .map(|(id, _)| id)
                .filter(|id| *id != model.id)
        })
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    let mut winners: HashMap<i32, card::Model> = HashMap::new();
    for chunk in wanted.chunks(RESOLVE_CHUNK) {
        let found = Card::find()
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .all(&state.db)
            .await?;
        for model in found {
            winners.insert(model.id, model);
        }
    }

    let mut lines: Vec<DeckPricingLine> = Vec::with_capacity(rows.len());
    for (quote, (entry, model)) in quotes.iter().zip(&rows) {
        let cheapest = quote.cheapest.and_then(|(id, cost)| {
            let card = if id == model.id {
                Some(CardResponse::from((*model).clone()))
            } else {
                winners.get(&id).cloned().map(CardResponse::from)
            };
            card.map(|card| DeckCheapestPrinting {
                card,
                price_usd: format_cents(cost),
            })
        });
        // A winner whose row vanished between the two queries (a re-import mid-request)
        // can't be named, and a saving against a printing we can't name is no saving.
        let saving_usd = cheapest.as_ref().and(quote.saving_cents).map(format_cents);
        lines.push(DeckPricingLine {
            card: CardResponse::from((*model).clone()),
            section_id: entry.section_id,
            quantity: entry.quantity,
            foil_quantity: entry.foil_quantity,
            price_usd: quote.held_cents.map(format_cents),
            cheapest,
            saving_usd,
        });
    }

    // Most expensive first, unpriced last, then by name and the row's own identity so the
    // order is stable across requests.
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by(|&a, &b| {
        let (qa, qb) = (&quotes[a], &quotes[b]);
        qb.held_cents
            .is_some()
            .cmp(&qa.held_cents.is_some())
            .then_with(|| qb.held_cents.cmp(&qa.held_cents))
            .then_with(|| lines[a].card.name.cmp(&lines[b].card.name))
            .then_with(|| lines[a].card.id.cmp(&lines[b].card.id))
            .then_with(|| lines[a].section_id.cmp(&lines[b].section_id))
    });
    let ordered_quotes: Vec<LineQuote> = order.iter().map(|&i| quotes[i]).collect();
    let mut slots: Vec<Option<DeckPricingLine>> = lines.into_iter().map(Some).collect();
    let lines: Vec<DeckPricingLine> = order.iter().filter_map(|&i| slots[i].take()).collect();

    let totals = fold_totals(
        valuation.any_priced.then_some(valuation.cents),
        &ordered_quotes,
    );
    Ok(DeckPricing {
        lines,
        total_usd: totals.total.map(format_cents),
        cheapest_total_usd: totals
            .total
            .zip(totals.saving)
            .map(|(total, saving)| format_cents(total - saving)),
        saving_usd: totals.saving.map(format_cents),
        unpriced_count: totals.unpriced_count,
        swappable_count: totals.swappable_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printing(id: i32, usd: Option<i128>, usd_foil: Option<i128>) -> PricedPrinting {
        PricedPrinting { id, usd, usd_foil }
    }

    #[test]
    fn a_nonfoil_row_is_judged_on_regular_prices_only() {
        // The held printing's regular copies cost 2 × 500; a reprint's regular is 100 even
        // though its foil is dearer than anything — the foil price is irrelevant to a
        // nonfoil row, because the swap keeps the row nonfoil.
        let held = printing(1, Some(500), Some(600));
        let reprint = printing(2, Some(100), Some(9_000));
        let quote = quote_line(&held, Some(1_000), 2, 0, &[held, reprint]);
        assert_eq!(quote.cheapest, Some((2, 200)));
        assert_eq!(quote.saving_cents, Some(800));
    }

    #[test]
    fn a_printing_unpriced_in_a_held_finish_is_never_called_cheapest() {
        // The row holds one foil. The reprint's regular price is a bargain, but it has no
        // foil price, so it cannot be the cheapest way to hold a foil copy.
        let held = printing(1, Some(500), Some(700));
        let foilless = printing(2, Some(50), None);
        let quote = quote_line(&held, Some(700), 0, 1, &[held, foilless]);
        assert_eq!(quote.cheapest, Some((1, 700)));
        assert_eq!(quote.saving_cents, Some(0));
    }

    #[test]
    fn a_mixed_row_costs_each_finish_at_its_own_price() {
        let held = printing(1, Some(500), Some(700));
        let reprint = printing(2, Some(100), Some(300));
        // 3 regular + 1 foil: held = 1500 + 700 = 2200; reprint = 300 + 300 = 600.
        let quote = quote_line(&held, Some(2_200), 3, 1, &[held, reprint]);
        assert_eq!(quote.cheapest, Some((2, 600)));
        assert_eq!(quote.saving_cents, Some(1_600));
    }

    #[test]
    fn ties_stay_on_the_held_printing() {
        let held = printing(7, Some(100), None);
        let twin = printing(2, Some(100), None);
        let quote = quote_line(&held, Some(100), 1, 0, &[twin, held]);
        assert_eq!(quote.cheapest, Some((7, 100)));
        assert_eq!(quote.saving_cents, Some(0));
    }

    #[test]
    fn a_held_printing_unpriced_in_a_held_finish_reports_a_cheapest_but_no_saving() {
        // 1 regular + 1 foil held; the held printing has no foil price, so the valuation
        // prices only its regular copy (500) — a floor. The reprint is fully priced at 400.
        // Naming it as the cheapest is true; claiming a 100 saving against a floor is not.
        let held = printing(1, Some(500), None);
        let reprint = printing(2, Some(200), Some(200));
        let quote = quote_line(&held, Some(500), 1, 1, &[held, reprint]);
        assert_eq!(quote.cheapest, Some((2, 400)));
        assert_eq!(quote.saving_cents, None);
    }

    #[test]
    fn an_unpriced_row_has_no_price_and_no_saving_but_may_have_a_cheapest() {
        let held = printing(1, None, None);
        let reprint = printing(2, Some(150), None);
        let quote = quote_line(&held, None, 1, 0, &[reprint]);
        assert_eq!(quote.held_cents, None);
        assert_eq!(quote.cheapest, Some((2, 150)));
        assert_eq!(quote.saving_cents, None);

        // …and with no priced printing anywhere, nothing at all.
        let quote = quote_line(&held, None, 1, 0, &[held]);
        assert_eq!(
            quote,
            LineQuote {
                held_cents: None,
                cheapest: None,
                saving_cents: None,
            }
        );
    }

    #[test]
    fn a_held_cost_reads_only_the_finishes_the_row_holds() {
        // One nonfoil copy of a foil-only printing: the foil price is real, but the row
        // holds none — unpriced, not "$0.00".
        let foil_only = printing(1, None, Some(2_000));
        assert_eq!(held_cost(&foil_only, 1, 0), None);
        assert_eq!(held_cost(&foil_only, 0, 1), Some(2_000));
        assert_eq!(
            held_cost(&foil_only, 1, 1),
            Some(2_000),
            "a priced held finish is a floor"
        );
        let both = printing(2, Some(100), Some(300));
        assert_eq!(held_cost(&both, 2, 1), Some(500));
        assert_eq!(held_cost(&printing(3, None, None), 3, 0), None);
    }

    #[test]
    fn totals_sum_known_savings_and_stay_null_for_an_unpriced_deck() {
        let quotes = [
            LineQuote {
                held_cents: Some(1_000),
                cheapest: Some((2, 200)),
                saving_cents: Some(800),
            },
            LineQuote {
                held_cents: Some(300),
                cheapest: Some((3, 300)),
                saving_cents: Some(0),
            },
            LineQuote {
                held_cents: None,
                cheapest: Some((4, 50)),
                saving_cents: None,
            },
        ];
        let totals = fold_totals(Some(1_300), &quotes);
        assert_eq!(totals.total, Some(1_300));
        assert_eq!(totals.saving, Some(800));
        assert_eq!(totals.unpriced_count, 1);
        assert_eq!(totals.swappable_count, 1);

        let totals = fold_totals(None, &quotes[2..]);
        assert_eq!(totals.total, None);
        assert_eq!(
            totals.saving, None,
            "an unpriced deck saves nothing knowable"
        );
        assert_eq!(totals.unpriced_count, 1);
    }

    #[test]
    fn a_priced_deck_with_nothing_to_save_reports_a_zero_saving_not_null() {
        let quotes = [LineQuote {
            held_cents: Some(300),
            cheapest: Some((3, 300)),
            saving_cents: Some(0),
        }];
        let totals = fold_totals(Some(300), &quotes);
        assert_eq!(totals.saving, Some(0));
        assert_eq!(totals.swappable_count, 0);
    }
}

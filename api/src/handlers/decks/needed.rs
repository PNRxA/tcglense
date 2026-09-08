//! The deck "cards needed" endpoint (issue #499): across all of a user's decks for a game,
//! the cards their decks collectively want more copies of than their collection holds — a
//! shopping list for building every deck at once — or, scoped with `?deck_id=` (issue
//! #675), the shopping list for *one* deck: what's still missing to finish it, and what
//! that costs.
//!
//! Two matching modes (see [`NeedMode`]): `card` aggregates by gameplay identity so any
//! printing you own covers any printing a deck wants (the default — "two decks want a
//! Command Tower, you own one, you need one more"); `printing` matches a deck's exact
//! printing against that same printing in the collection, naming the precise printing
//! that's short. Either way, each result carries which of the caller's decks want it.
//!
//! **Scoping never changes the supply.** The demand scan always covers every deck the
//! caller owns and the collection is always compared against that whole demand; a deck
//! filter only decides which shortfalls are *reported* and how much of each is this deck's:
//! `min(what this deck wants, the shortfall across every deck)`. Comparing one deck's demand
//! against the collection on its own would let two decks that share one owned Sol Ring both
//! claim it — and the point of a per-deck list is to be the one you can buy from.
//!
//! **Money is a citation of the catalog's prices, folded two ways.** `held_usd` prices the
//! shortfall as the decks hold the card — regular copies at `usd`, foil at `usd_foil`, over
//! every printing they run, charged per copy; `cheapest_usd` prices it at the card's
//! cheapest printing anywhere, through the shared cheapest-printing seam
//! (`handlers::shared::cheapest`, which excludes folded foil-★ variants). Both go through
//! the valuation module's cents path, and both are `null` — never `"0.00"` — when nothing
//! relevant is priced, with the totals counting how many entries that is, so a total is
//! always known to be a floor when it is one.
//!
//! Reads only (`AuthUser`), in the no-store private group. A deck card has no `user_id`, so
//! the demand scan is scoped to the deck ids the caller owns for the game (never queried by
//! user directly), and a `deck_id` that isn't one of them is a **404** through the same
//! [`load_deck`] gate every deck route uses; catalog rows gone after a re-import are skipped,
//! exactly as the deck detail read does, and so are maybeboard sections (issue #570) — a
//! card a deck is only considering is not one you need to buy.

use std::collections::{BTreeSet, HashMap, HashSet};

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

use crate::auth::extractor::AuthUser;
use crate::entities::prelude::{Card, CollectionItem, Deck, DeckCard};
use crate::entities::{card, collection_item, deck, deck_card};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::valuation::{cheapest_single_cents, format_cents, price_cents};
use crate::handlers::shared::{CardResponse, load_cheapest_by_oracle, require_game};
use crate::state::AppState;

use super::{
    NeedMode, NeededCard, NeededCardDeck, NeededCards, NeededParams, NeededTotals, load_deck,
};

/// List cards needed across a game's decks, or for one deck
///
/// `GET /api/decks/{game}/needed` -> every card the caller's decks collectively want more
/// copies of than their collection holds, sorted by name, with what the shortfall costs at
/// the printings the decks hold and at each card's cheapest printing. `mode=card` (default)
/// aggregates across any printing of a gameplay card; `mode=printing` reports the exact
/// missing printing. `deck_id=` scopes the list to one of the caller's decks — its share of
/// the shortfall across every deck, so a copy two decks share is never counted as owned by
/// both. Each entry lists the decks that want the card. Returns
/// `{ data: NeededCard[], deck, totals }`.
#[utoipa::path(
    get,
    path = "/api/decks/{game}/needed",
    tag = "Decks",
    security(("api_key" = [])),
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("mode" = Option<String>, Query, description = "`card` (default) counts any printing of a gameplay card; `printing` reports the exact missing printing."),
        ("deck_id" = Option<i32>, Query, description = "Scope the list to one of the caller's decks: what *it* still needs, as its share of the shortfall across every deck. A deck that isn't the caller's is a 404."),
    ),
    responses(
        (status = 200, description = "Cards the caller's decks need beyond their collection, by name, with priced totals.", body = NeededCards),
        (status = 401, description = "Missing or invalid API key."),
        (status = 404, description = "Unknown game, or a `deck_id` that isn't one of the caller's decks."),
    ),
)]
pub async fn needed_cards(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<NeededParams>,
) -> Result<Json<NeededCards>, AppError> {
    require_game(&game)?;

    // The scope, proved first: a deck that isn't the caller's (or is for another game) is
    // a 404 before any work — and before the "no decks" early return below, so a foreign
    // id on an empty account answers exactly as it would on a full one.
    let scope = match params.deck_id {
        Some(deck_id) => Some(load_deck(&state, user.id, &game, deck_id).await?),
        None => None,
    };
    let scope_ref = scope.as_ref().map(|d| NeededCardDeck {
        id: d.id,
        name: d.name.clone(),
    });

    // The caller's decks for this game — id + name only (id scopes the card scan below;
    // name labels each result's affected decks). A deck card has no user_id, so its
    // demand is only ever reached through these owned deck ids.
    let decks: Vec<(i32, String)> = Deck::find()
        .select_only()
        .column(deck::Column::Id)
        .column(deck::Column::Name)
        .filter(deck::Column::UserId.eq(user.id))
        .filter(deck::Column::Game.eq(&game))
        .into_tuple()
        .all(&state.db)
        .await?;
    if decks.is_empty() {
        return Ok(Json(NeededCards {
            data: Vec::new(),
            deck: scope_ref,
            totals: fold_totals(&[]),
        }));
    }
    let deck_names: HashMap<i32, String> = decks.iter().cloned().collect();
    let deck_ids: Vec<i32> = decks.iter().map(|(id, _)| *id).collect();

    // Every deck card joined to its catalog row (a card gone after a re-import LEFT-joins to
    // None and is skipped, matching the deck detail read). Maybeboard sections are excluded
    // (issue #570) — a shopping list should name what the decks actually run, not everything
    // their owners are still considering. Always every deck, scoped or not: the supply is
    // compared against the whole demand (see the module docs).
    let deck_rows: Vec<(deck_card::Model, Option<card::Model>)> = DeckCard::find()
        .find_also_related(Card)
        .filter(deck_card::Column::DeckId.is_in(deck_ids.iter().copied()))
        .filter(
            deck_card::Column::SectionId
                .not_in_subquery(super::maybeboard_section_ids(deck_ids.clone())),
        )
        .all(&state.db)
        .await?;

    // The caller's collection for the game, summed two ways so either mode has its supply:
    // per printing (internal card id) and per gameplay identity (oracle id, else name).
    let owned_rows: Vec<(i32, Option<String>, String, i32, i32)> = CollectionItem::find()
        .select_only()
        .column(collection_item::Column::CardId)
        .column(card::Column::OracleId)
        .column(card::Column::Name)
        .column(collection_item::Column::Quantity)
        .column(collection_item::Column::FoilQuantity)
        .inner_join(Card)
        .filter(collection_item::Column::UserId.eq(user.id))
        .filter(collection_item::Column::Game.eq(&game))
        .into_tuple()
        .all(&state.db)
        .await?;

    let mut owned_by_printing: HashMap<i32, i64> = HashMap::new();
    let mut owned_by_identity: HashMap<String, i64> = HashMap::new();
    for (card_id, oracle_id, name, quantity, foil_quantity) in owned_rows {
        let copies = i64::from(quantity) + i64::from(foil_quantity);
        *owned_by_printing.entry(card_id).or_default() += copies;
        *owned_by_identity
            .entry(identity_key(oracle_id.as_deref(), &name))
            .or_default() += copies;
    }

    // Fold deck demand into groups keyed by the mode's grouping, tracking the total copies
    // wanted (by every deck, and by the scoped one), which decks want them, and each
    // contributing printing's demand by finish — the per-finish split is what prices the
    // shortfall as the decks hold it, and the copies pick the representative card the
    // `card` mode shows.
    let scope_id = scope.as_ref().map(|d| d.id);
    let mut groups: HashMap<String, Group> = HashMap::new();
    for (entry, card) in deck_rows {
        let Some(card) = card else { continue };
        let demand = Demand {
            regular: i64::from(entry.quantity),
            foil: i64::from(entry.foil_quantity),
        };
        let in_scope = scope_id == Some(entry.deck_id);
        let key = match params.mode {
            NeedMode::Card => identity_key(card.oracle_id.as_deref(), &card.name),
            NeedMode::Printing => format!("p:{}", card.id),
        };
        let group = groups.entry(key).or_default();
        group.required += demand.copies();
        group.deck_ids.insert(entry.deck_id);
        let printing = group
            .printings
            .entry(card.id)
            .or_insert_with(|| PrintingDemand::new(card));
        printing.all.add(demand);
        if in_scope {
            group.scoped_required += demand.copies();
            printing.scoped.add(demand);
        }
    }

    // The cheapest printing of every identity the decks want, one lookup — the floor a
    // shopper can buy at, whichever printing the deck happens to list.
    let oracle_ids: HashSet<&str> = groups
        .values()
        .flat_map(|group| group.printings.values())
        .filter_map(|p| p.card.oracle_id.as_deref().filter(|id| !id.is_empty()))
        .collect();
    let cheapest_by_oracle = load_cheapest_by_oracle(&state.db, &game, &oracle_ids).await?;

    // Emit the shortfalls (demand beyond supply), sorted by card name.
    let mut data: Vec<NeededCard> = Vec::new();
    for (key, group) in groups {
        // Scoped, the entry describes what *this* deck wants; unscoped, what every deck does.
        let required = if scope_id.is_some() {
            group.scoped_required
        } else {
            group.required
        };
        if required <= 0 {
            continue; // scoped: a card only the other decks want
        }
        let demand_of = |p: &PrintingDemand| if scope_id.is_some() { p.scoped } else { p.all };

        // The representative printing is the one the decks (or the scoped deck) want most
        // (ties: lowest catalog id), so `card` mode shows a printing they actually
        // reference. In `printing` mode the group is a single printing, so this just
        // returns it.
        let Some(rep_id) = group
            .printings
            .iter()
            .max_by(|(a_id, a), (b_id, b)| {
                demand_of(a)
                    .copies()
                    .cmp(&demand_of(b).copies())
                    .then_with(|| b_id.cmp(a_id))
            })
            .map(|(id, _)| *id)
        else {
            continue; // unreachable: a demand group always has at least one printing
        };
        let owned = match params.mode {
            NeedMode::Card => owned_by_identity.get(&key).copied().unwrap_or(0),
            NeedMode::Printing => owned_by_printing.get(&rep_id).copied().unwrap_or(0),
        };
        // This deck's share of the shortfall across every deck — the whole shortfall when
        // there is no scope, since then `required` is every deck's demand.
        let needed = required.min(group.required - owned);
        if needed <= 0 {
            continue;
        }

        // Priced as held: each contributing printing's demand at its own prices, per finish.
        let held_lines: Vec<(Option<i128>, i64)> = group
            .printings
            .values()
            .flat_map(|p| {
                let demand = demand_of(p);
                [
                    (price_cents(p.card.price_usd.as_deref()), demand.regular),
                    (price_cents(p.card.price_usd_foil.as_deref()), demand.foil),
                ]
            })
            .collect();
        let held_cents = held_cost_cents(needed, &held_lines);
        // Priced at the cheapest printing: the catalog-wide floor for the identity, or — for
        // a card with no identity to find siblings by — the cheapest of the printings the
        // decks themselves run.
        let cheapest_unit = group
            .printings
            .values()
            .filter_map(|p| p.card.oracle_id.as_deref().filter(|id| !id.is_empty()))
            .next()
            .and_then(|oracle_id| cheapest_by_oracle.get(oracle_id).copied())
            .or_else(|| {
                group
                    .printings
                    .values()
                    .filter_map(|p| {
                        cheapest_single_cents(
                            p.card.price_usd.as_deref(),
                            p.card.price_usd_foil.as_deref(),
                        )
                    })
                    .min()
            });
        let cheapest_cents = cheapest_unit.map(|unit| unit * i128::from(needed));

        let mut decks: Vec<NeededCardDeck> = group
            .deck_ids
            .iter()
            .filter_map(|id| {
                deck_names.get(id).map(|name| NeededCardDeck {
                    id: *id,
                    name: name.clone(),
                })
            })
            .collect();
        decks.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        let mut printings = group.printings;
        let Some(rep) = printings.remove(&rep_id) else {
            continue; // unreachable: `rep_id` came from this map
        };
        data.push(NeededCard {
            card: CardResponse::from(rep.card),
            needed,
            required,
            owned,
            decks,
            held_usd: held_cents.map(format_cents),
            cheapest_usd: cheapest_cents.map(format_cents),
        });
    }
    data.sort_by(|a, b| {
        a.card
            .name
            .cmp(&b.card.name)
            .then_with(|| a.card.id.cmp(&b.card.id))
    });

    let totals = fold_totals(&data);
    Ok(Json(NeededCards {
        data,
        deck: scope_ref,
        totals,
    }))
}

/// Copies wanted of one printing, by finish.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Demand {
    regular: i64,
    foil: i64,
}

impl Demand {
    fn copies(self) -> i64 {
        self.regular + self.foil
    }

    fn add(&mut self, other: Demand) {
        self.regular += other.regular;
        self.foil += other.foil;
    }
}

/// One printing a demand group draws on: its catalog row, and the copies wanted of it by
/// every deck and by the scoped deck alone.
struct PrintingDemand {
    card: card::Model,
    all: Demand,
    scoped: Demand,
}

impl PrintingDemand {
    fn new(card: card::Model) -> Self {
        Self {
            card,
            all: Demand::default(),
            scoped: Demand::default(),
        }
    }
}

/// One demand group — a gameplay identity in `card` mode, a printing in `printing` mode.
#[derive(Default)]
struct Group {
    /// Copies wanted by every deck.
    required: i64,
    /// Copies wanted by the scoped deck (zero without a scope).
    scoped_required: i64,
    deck_ids: BTreeSet<i32>,
    /// `card_id` -> that printing's demand + catalog row.
    printings: HashMap<i32, PrintingDemand>,
}

/// What `needed` copies cost at the price the decks hold the card at, in cents. `lines` is
/// each held `(finish price, copies wanted in that finish)` pair over every contributing
/// printing; the priced pairs give a per-copy price — the decks' own demand valued as held,
/// divided by the copies that valuation covers — and the shortfall is charged at it,
/// rounded to the nearest cent. `None` when no held finish with copies is priced: which of
/// the wanted copies are the missing ones is unknowable, so the honest price of the
/// shortfall is the average the deck pays, not a guess at a finish, and it can only be
/// stated when there is a priced copy to average.
fn held_cost_cents(needed: i64, lines: &[(Option<i128>, i64)]) -> Option<i128> {
    let mut priced_cents: i128 = 0;
    let mut priced_copies: i128 = 0;
    for &(price, copies) in lines {
        if copies <= 0 {
            continue;
        }
        if let Some(cents) = price {
            priced_cents += cents * i128::from(copies);
            priced_copies += i128::from(copies);
        }
    }
    if priced_copies == 0 {
        return None;
    }
    // `needed × (priced_cents / priced_copies)`, rounded half-up in integer arithmetic.
    let numerator = i128::from(needed) * priced_cents * 2 + priced_copies;
    Some(numerator / (priced_copies * 2))
}

/// The list's totals: sizes, and each money column summed over the entries that carry it
/// (`null` when none does), with the count of entries that don't so a client can say when
/// a total is a floor.
fn fold_totals(data: &[NeededCard]) -> NeededTotals {
    fn sum(prices: impl Iterator<Item = Option<i128>>) -> (Option<i128>, i64) {
        let mut total: Option<i128> = None;
        let mut unpriced = 0;
        for price in prices {
            match price {
                Some(cents) => total = Some(total.unwrap_or(0) + cents),
                None => unpriced += 1,
            }
        }
        (total, unpriced)
    }
    let (held, held_unpriced) = sum(data.iter().map(|e| price_cents(e.held_usd.as_deref())));
    let (cheapest, cheapest_unpriced) =
        sum(data.iter().map(|e| price_cents(e.cheapest_usd.as_deref())));
    NeededTotals {
        cards: data.len() as i64,
        copies: data.iter().map(|e| e.needed).sum(),
        held_usd: held.map(format_cents),
        held_unpriced_cards: held_unpriced,
        cheapest_usd: cheapest.map(format_cents),
        cheapest_unpriced_cards: cheapest_unpriced,
    }
}

/// The gameplay identity of a card across printings: its `oracle_id`, or its name when the
/// catalog has none — the same rule the deck printing-swap uses to decide two rows are the
/// same card. Namespaced (`o:` / `n:`) so an oracle id can never collide with a name.
fn identity_key(oracle_id: Option<&str>, name: &str) -> String {
    match oracle_id {
        Some(oracle) => format!("o:{oracle}"),
        None => format!("n:{name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_cost_charges_the_shortfall_at_the_decks_own_per_copy_price() {
        // One printing, regular only: 3 needed at $1.50 = $4.50.
        assert_eq!(
            held_cost_cents(3, &[(Some(150), 4), (Some(500), 0)]),
            Some(450)
        );
        // Held two ways — 2 regular at $1.00 and 2 foil at $5.00 — is $3.00 a copy, and
        // three missing copies are charged at that: $9.00, never "the cheap ones are the
        // ones you own" nor the dear ones.
        assert_eq!(
            held_cost_cents(3, &[(Some(100), 2), (Some(500), 2)]),
            Some(900)
        );
        // Rounds to the nearest cent: 1 needed at (0.01×1 + 0.02×2)/3 = 1.666… cents -> 2.
        assert_eq!(held_cost_cents(1, &[(Some(1), 1), (Some(2), 2)]), Some(2));
    }

    #[test]
    fn held_cost_averages_only_over_priced_copies_and_is_none_when_none_are() {
        // The unpriced foil copies don't drag the average down to a fraction of the regular
        // price: 2 needed at the $2.00 the priced copies cost = $4.00.
        assert_eq!(held_cost_cents(2, &[(Some(200), 1), (None, 3)]), Some(400));
        // A finish with a price but no wanted copies is not "held", so it prices nothing.
        assert_eq!(held_cost_cents(2, &[(Some(200), 0), (None, 3)]), None);
        // Nothing priced: unpriced, never $0.00.
        assert_eq!(held_cost_cents(2, &[(None, 1), (None, 1)]), None);
        assert_eq!(held_cost_cents(2, &[]), None);
    }

    #[test]
    fn identity_key_namespaces_oracle_and_name() {
        assert_eq!(identity_key(Some("abc"), "Sol Ring"), "o:abc");
        assert_eq!(identity_key(None, "Sol Ring"), "n:Sol Ring");
    }
}

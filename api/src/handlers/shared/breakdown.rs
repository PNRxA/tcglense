//! The holdings **breakdown** (issue #680): where a collection's — or a wish list's —
//! money is. Value and copies by rarity, by colour-identity bucket, by card type and by
//! finish, plus the top holdings by *held* value (price × copies, never a single copy's
//! price), all folded from the same narrow holdings ⋈ cards rows the summary reads.
//!
//! One engine, two twins: the fold here is generic over [`BreakdownRow`] — the
//! [`SummaryRow`] the summary/sets folds already aggregate, widened by the four facet
//! columns — so `handlers::collection::breakdown` and `handlers::wishlist::breakdown`
//! differ only in the SeaORM query that names their entity. The response's `summary` is
//! [`summarize_holdings`] over the very same rows, so the breakdown's total can never
//! disagree with the landing header beside it.
//!
//! It is the third whole-holdings scan per user (after value history and movers), so
//! every handler wraps its computation in the analytics response cache under the
//! surface's own holdings version ([`breakdown_cache_key`]) and rides the per-user
//! `analytics` rate bucket.

use std::collections::HashMap;

use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, QueryFilter, QuerySelect,
    Select, SelectModel, Selector,
};
use serde::{Deserialize, Serialize};

use crate::analytics_cache::HoldingsSurface;
use crate::entities::card;
use crate::entities::prelude::Card;
use crate::error::AppError;
use crate::state::AppState;

use super::dto::CardResponse;
use super::holdings::{
    CardSummaryFacts, CollectionSummary, HoldingCounts, HoldingSummaryRow, SummaryRow,
    select_summary_columns, summarize_holdings,
};
use super::type_line::primary_type;
use super::valuation::{Valuation, format_cents, price_cents, resolve_bulk_threshold_cents};

/// How many top holdings the breakdown names. Enough to answer "where is my money" at
/// a glance; the full ranking is the list endpoint sorted by price.
pub(crate) const TOP_HOLDINGS: usize = 10;

/// The wire token every breakdown handler passes as the analytics-cache endpoint segment.
const CACHE_ENDPOINT: &str = "breakdown";

// ---------- Wire DTOs ----------

/// Query params for a breakdown read: the bulk threshold the embedded `summary` splits
/// its bulk slice at — the same `bulk_max_cents` the summary takes, so the two agree.
#[derive(Debug, Default, Deserialize)]
pub struct BreakdownParams {
    /// Per-unit price cutoff (USD cents) under which a finish counts as bulk. Absent =
    /// the server default ($1); clamped like the summary's.
    pub bulk_max_cents: Option<i64>,
}

impl BreakdownParams {
    pub(crate) fn bulk_threshold_cents(&self) -> i128 {
        resolve_bulk_threshold_cents(self.bulk_max_cents)
    }
}

/// One bucket of a breakdown facet: how many distinct held cards and copies file under
/// it, and what they are worth as held (regular copies at `usd`, foil at `usd_foil`).
#[derive(Debug, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct BreakdownBucket {
    /// The bucket's stable key. Rarity: Scryfall's own (`common`/`uncommon`/`rare`/
    /// `mythic`/`special`/`bonus`, `unknown` for a card with none). Colour:
    /// `white`/`blue`/`black`/`red`/`green` for a mono-coloured identity, `multicolor`
    /// for two or more colours, `colorless` for none. Type: the type line's first card
    /// type past the supertypes, lower-cased (`creature`, `land`, …; an artifact creature
    /// files under `artifact`), `other` when the line names none. Finish: `regular` /
    /// `foil`.
    pub key: String,
    /// Distinct held cards in the bucket (one per holdings row).
    pub cards: i64,
    /// Held copies in the bucket (regular + foil — or, for a finish bucket, that finish).
    pub copies: i64,
    /// Estimated USD value of the bucket's held copies, a 2-dp decimal string; `null` when
    /// none of them is priced.
    pub value_usd: Option<String>,
}

/// One of the most valuable holdings, ranked by **held** value — `usd × quantity +
/// usd_foil × foil_quantity` — never by a single copy's price (that ranking is the list's
/// `sort=price`). A finish the row holds but the catalog doesn't price contributes
/// nothing, so `value_usd` can be a floor for a partly-priced holding.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct TopHolding {
    pub card: CardResponse,
    pub quantity: i32,
    pub foil_quantity: i32,
    /// The held value, a 2-dp decimal string (always priced — an unpriced holding never
    /// ranks).
    pub value_usd: String,
}

/// The breakdown of a user's per-game holdings (the collection or the wish list; the
/// wish-list twin reads "wanted" for "held"): value and copies by rarity, colour
/// identity, card type and finish, and the top holdings by held value.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct HoldingBreakdown {
    /// The same summary `GET …/summary` answers, folded from the same rows (so the
    /// bucket values below are slices of exactly this total).
    pub summary: CollectionSummary,
    /// By rarity, in rarity order (common → mythic → special → bonus, then anything
    /// else alphabetically, `unknown` last). Only non-empty buckets are listed.
    pub rarity: Vec<BreakdownBucket>,
    /// By colour identity, in WUBRG order then `multicolor` then `colorless`. Only
    /// non-empty buckets are listed.
    pub color: Vec<BreakdownBucket>,
    /// By the type line's first card type, most valuable bucket first (then most copies,
    /// then key). Only non-empty buckets are listed.
    pub card_type: Vec<BreakdownBucket>,
    /// By finish: `regular` then `foil`, each counting only that finish's copies. Only
    /// non-empty buckets are listed.
    pub finish: Vec<BreakdownBucket>,
    /// The most valuable holdings by held value, highest first (at most ten).
    pub top: Vec<TopHolding>,
    /// Distinct held cards that contribute nothing to any value here because no finish
    /// they're held in is priced — so a reader can tell a small total from an unpriced one.
    pub unpriced_cards: i64,
}

// ---------- Rows ----------

/// The card-side facets the breakdown buckets by, borrowed from whichever row
/// representation the caller fetched. `card_id` is the internal id, so the top holdings
/// can be dressed with their full card rows afterwards.
pub(crate) struct CardBreakdownFacts<'a> {
    pub card_id: i32,
    pub rarity: Option<&'a str>,
    /// Comma-joined colour letters as stored (`"W,U"`), `None`/empty for colourless.
    pub color_identity: Option<&'a str>,
    pub type_line: Option<&'a str>,
}

/// A row the breakdown can aggregate: a [`SummaryRow`] (counts + the priced card facts
/// the summary folds) that also carries the breakdown's facet columns. `None` = the card
/// row vanished in a catalog re-import — skipped everywhere, matching the summary.
pub(crate) trait BreakdownRow: SummaryRow {
    fn breakdown_facts(&self) -> Option<CardBreakdownFacts<'_>>;
}

impl<H: HoldingCounts> BreakdownRow for (H, Option<card::Model>) {
    fn breakdown_facts(&self) -> Option<CardBreakdownFacts<'_>> {
        self.1.as_ref().map(|card| CardBreakdownFacts {
            card_id: card.id,
            rarity: card.rarity.as_deref(),
            color_identity: card.color_identity.as_deref(),
            type_line: card.type_line.as_deref(),
        })
    }
}

/// The breakdown's narrow projection: the summary's [`HoldingSummaryRow`] columns
/// (nested, so the two projections share one column list) plus the four facet columns.
/// Every facet is `Option` because the LEFT JOIN yields NULLs when the card row is gone;
/// the nested row's `set_code` stays the row-present sentinel.
#[derive(FromQueryResult)]
pub(crate) struct HoldingBreakdownRow {
    #[sea_orm(nested)]
    summary: HoldingSummaryRow,
    card_id: Option<i32>,
    rarity: Option<String>,
    color_identity: Option<String>,
    type_line: Option<String>,
}

impl SummaryRow for HoldingBreakdownRow {
    fn quantity(&self) -> i32 {
        self.summary.quantity()
    }

    fn foil_quantity(&self) -> i32 {
        self.summary.foil_quantity()
    }

    fn card_facts(&self) -> Option<CardSummaryFacts<'_>> {
        self.summary.card_facts()
    }
}

impl BreakdownRow for HoldingBreakdownRow {
    fn breakdown_facts(&self) -> Option<CardBreakdownFacts<'_>> {
        // The summary row's presence rule (a NULL `set_code` is a vanished card) decides
        // for the facets too, so a row is either in every fold or in none.
        self.summary.card_facts()?;
        Some(CardBreakdownFacts {
            card_id: self.card_id?,
            rarity: self.rarity.as_deref(),
            color_identity: self.color_identity.as_deref(),
            type_line: self.type_line.as_deref(),
        })
    }
}

/// Apply the breakdown projection to a holdings ⋈ cards join: the summary's column list
/// plus the facets. Shared by the collection/wish-list twins (which pass their own count
/// columns); the join + per-user filters stay with each entity's query.
pub(crate) fn narrow_breakdown_rows<E: EntityTrait, C: ColumnTrait>(
    query: Select<E>,
    quantity: C,
    foil_quantity: C,
) -> Selector<SelectModel<HoldingBreakdownRow>> {
    select_summary_columns(query, quantity, foil_quantity)
        .column_as(card::Column::Id, "card_id")
        .column_as(card::Column::Rarity, "rarity")
        .column_as(card::Column::ColorIdentity, "color_identity")
        .column_as(card::Column::TypeLine, "type_line")
        .into_model::<HoldingBreakdownRow>()
}

// ---------- The fold ----------

/// Rarity keys in display order; anything else sorts after them alphabetically, and
/// `unknown` last.
const RARITY_ORDER: &[&str] = &["common", "uncommon", "rare", "mythic", "special", "bonus"];

/// Colour buckets in display order: the five colours (WUBRG), then multicolour, then
/// colourless.
const COLOR_ORDER: &[&str] = &[
    "white",
    "blue",
    "black",
    "red",
    "green",
    "multicolor",
    "colorless",
];

/// The rarity bucket key for a stored rarity: Scryfall's lower-case word as is, `unknown`
/// for a missing/blank one.
fn rarity_key(rarity: Option<&str>) -> String {
    match rarity.map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => r.to_ascii_lowercase(),
        None => "unknown".to_string(),
    }
}

/// The colour bucket for a stored comma-joined colour identity: one colour → its name,
/// two or more → `multicolor`, none → `colorless`. A letter that isn't one of the five
/// (never stored, but defensively) counts as a colour without a name of its own, so a
/// lone unknown letter is `multicolor` rather than a made-up bucket.
fn color_key(color_identity: Option<&str>) -> &'static str {
    let letters: Vec<&str> = color_identity
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    match letters.as_slice() {
        [] => "colorless",
        [one] => match one.to_ascii_uppercase().as_str() {
            "W" => "white",
            "U" => "blue",
            "B" => "black",
            "R" => "red",
            "G" => "green",
            _ => "multicolor",
        },
        _ => "multicolor",
    }
}

/// The type bucket for a type line: the primary type lower-cased, `other` when the line
/// names no card type.
fn type_key(type_line: Option<&str>) -> String {
    primary_type(type_line).map_or_else(|| "other".to_string(), str::to_ascii_lowercase)
}

/// Running totals for one bucket.
#[derive(Default)]
struct BucketAgg {
    cards: i64,
    copies: i64,
    valuation: Valuation,
}

impl BucketAgg {
    fn add(&mut self, usd: Option<&str>, qty: i32, usd_foil: Option<&str>, foil_qty: i32) {
        self.cards += 1;
        self.copies += i64::from(qty) + i64::from(foil_qty);
        self.valuation.add(usd, qty, usd_foil, foil_qty);
    }

    fn into_bucket(self, key: String) -> BreakdownBucket {
        BreakdownBucket {
            key,
            cards: self.cards,
            copies: self.copies,
            value_usd: self.valuation.total_usd(),
        }
    }
}

/// A fold's intermediate result: everything on the wire except the top holdings, which
/// still need their card rows loaded ([`finish_breakdown`]).
pub(crate) struct FoldedBreakdown {
    pub summary: CollectionSummary,
    pub rarity: Vec<BreakdownBucket>,
    pub color: Vec<BreakdownBucket>,
    pub card_type: Vec<BreakdownBucket>,
    pub finish: Vec<BreakdownBucket>,
    /// `(internal card id, quantity, foil_quantity, held cents)`, highest value first.
    pub top: Vec<(i32, i32, i32, i128)>,
    pub unpriced_cards: i64,
}

/// Fold already-fetched holdings rows into the breakdown's buckets and top-holdings
/// ranking. Pure, entity-agnostic, and skips a row whose card is gone for **every**
/// figure — the same rule as [`summarize_holdings`], which computes the embedded summary
/// over the same rows. `top_n` bounds the ranking (the handlers pass [`TOP_HOLDINGS`]).
pub(crate) fn fold_breakdown<R: BreakdownRow>(
    rows: &[R],
    bulk_threshold_cents: i128,
    top_n: usize,
) -> FoldedBreakdown {
    let summary = summarize_holdings(rows, bulk_threshold_cents);

    let mut rarity: HashMap<String, BucketAgg> = HashMap::new();
    let mut color: HashMap<&'static str, BucketAgg> = HashMap::new();
    let mut card_type: HashMap<String, BucketAgg> = HashMap::new();
    let mut regular = BucketAgg::default();
    let mut foil = BucketAgg::default();
    let mut ranked: Vec<(i32, i32, i32, i128)> = Vec::new();
    let mut unpriced_cards = 0;

    for row in rows {
        let (Some(card), Some(facts)) = (row.card_facts(), row.breakdown_facts()) else {
            continue;
        };
        let (qty, foil_qty) = (row.quantity(), row.foil_quantity());
        // A price only counts for a finish that is actually held: `Valuation` flips its
        // "anything priced" flag on a priced finish even at zero copies (so the summary
        // reads a foil-only holding with only a regular price as `"0.00"`), but a bucket
        // and the ranking must not call such a card priced — it contributes nothing.
        let usd = (qty > 0).then_some(card.price_usd).flatten();
        let usd_foil = (foil_qty > 0).then_some(card.price_usd_foil).flatten();

        rarity
            .entry(rarity_key(facts.rarity))
            .or_default()
            .add(usd, qty, usd_foil, foil_qty);
        color
            .entry(color_key(facts.color_identity))
            .or_default()
            .add(usd, qty, usd_foil, foil_qty);
        card_type
            .entry(type_key(facts.type_line))
            .or_default()
            .add(usd, qty, usd_foil, foil_qty);
        // A finish bucket counts a card only if some copies are held in that finish, and
        // values only those copies.
        if qty > 0 {
            regular.add(usd, qty, None, 0);
        }
        if foil_qty > 0 {
            foil.add(None, 0, usd_foil, foil_qty);
        }

        // Held value: each finish's price × its copies, over the priced finishes only.
        let mut held = Valuation::default();
        held.add(usd, qty, usd_foil, foil_qty);
        if held.any_priced {
            ranked.push((facts.card_id, qty, foil_qty, held.cents));
        } else {
            unpriced_cards += 1;
        }
    }

    // Highest held value first; the internal id breaks ties so the ranking is stable.
    ranked.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(top_n);

    let mut rarity: Vec<BreakdownBucket> = rarity
        .into_iter()
        .map(|(key, agg)| agg.into_bucket(key))
        .collect();
    rarity.sort_by(|a, b| rarity_rank(&a.key).cmp(&rarity_rank(&b.key)));

    let color: Vec<BreakdownBucket> = COLOR_ORDER
        .iter()
        .filter_map(|key| {
            color
                .remove(key)
                .map(|agg| agg.into_bucket((*key).to_string()))
        })
        .collect();

    let mut card_type: Vec<BreakdownBucket> = card_type
        .into_iter()
        .map(|(key, agg)| agg.into_bucket(key))
        .collect();
    card_type.sort_by(|a, b| {
        bucket_cents(b)
            .cmp(&bucket_cents(a))
            .then_with(|| b.copies.cmp(&a.copies))
            .then_with(|| a.key.cmp(&b.key))
    });

    let finish: Vec<BreakdownBucket> = [("regular", regular), ("foil", foil)]
        .into_iter()
        .filter(|(_, agg)| agg.cards > 0)
        .map(|(key, agg)| agg.into_bucket(key.to_string()))
        .collect();

    FoldedBreakdown {
        summary,
        rarity,
        color,
        card_type,
        finish,
        top: ranked,
        unpriced_cards,
    }
}

/// Sort rank for a rarity key: its position in [`RARITY_ORDER`], then the rest
/// alphabetically, `unknown` last.
fn rarity_rank(key: &str) -> (usize, usize, &str) {
    match RARITY_ORDER.iter().position(|k| *k == key) {
        Some(i) => (0, i, key),
        None if key == "unknown" => (2, 0, key),
        None => (1, 0, key),
    }
}

/// A bucket's value in cents for sorting (unpriced sorts as zero — after every priced
/// bucket, since a priced one is at least `"0.00"` by explicit value and typically more).
fn bucket_cents(bucket: &BreakdownBucket) -> i128 {
    price_cents(bucket.value_usd.as_deref()).unwrap_or(-1)
}

// ---------- Dressing + caching ----------

/// Load the top holdings' card rows and assemble the wire response. One bounded query
/// (`TOP_HOLDINGS` ids at most); a card whose row has gone since the fold is dropped.
pub(crate) async fn finish_breakdown(
    db: &DatabaseConnection,
    folded: FoldedBreakdown,
) -> Result<HoldingBreakdown, AppError> {
    let ids: Vec<i32> = folded.top.iter().map(|(id, ..)| *id).collect();
    let mut cards: HashMap<i32, card::Model> = if ids.is_empty() {
        HashMap::new()
    } else {
        Card::find()
            .filter(card::Column::Id.is_in(ids))
            .all(db)
            .await?
            .into_iter()
            .map(|c| (c.id, c))
            .collect()
    };
    let top = folded
        .top
        .into_iter()
        .filter_map(|(id, quantity, foil_quantity, cents)| {
            cards.remove(&id).map(|c| TopHolding {
                card: CardResponse::from(c),
                quantity,
                foil_quantity,
                value_usd: format_cents(cents),
            })
        })
        .collect();
    Ok(HoldingBreakdown {
        summary: folded.summary,
        rarity: folded.rarity,
        color: folded.color,
        card_type: folded.card_type,
        finish: folded.finish,
        top,
        unpriced_cards: folded.unpriced_cards,
    })
}

/// The analytics-cache body key for a breakdown read on `surface`: the surface's own
/// holdings version + the price epoch + the UTC date, with the bulk threshold as the
/// params segment (it changes the embedded summary's bulk slice). `None` = cache degraded.
pub(crate) async fn breakdown_cache_key(
    state: &AppState,
    surface: HoldingsSurface,
    user_id: i32,
    game: &str,
    params: &BreakdownParams,
) -> Option<String> {
    state
        .analytics_cache
        .surface_body_key(
            surface,
            user_id,
            game,
            CACHE_ENDPOINT,
            &format!("bulk{}", params.bulk_threshold_cents()),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::collection_item;

    fn holding(card_id: i32, quantity: i32, foil_quantity: i32) -> collection_item::Model {
        let now = chrono::Utc::now();
        collection_item::Model {
            id: card_id,
            user_id: 1,
            game: "mtg".into(),
            card_id,
            quantity,
            foil_quantity,
            created_at: now,
            updated_at: now,
        }
    }

    fn card(
        id: i32,
        rarity: Option<&str>,
        colors: Option<&str>,
        type_line: &str,
        usd: Option<&str>,
        usd_foil: Option<&str>,
    ) -> card::Model {
        card::Model {
            rarity: rarity.map(str::to_string),
            color_identity: colors.map(str::to_string),
            type_line: Some(type_line.to_string()),
            price_usd: usd.map(str::to_string),
            price_usd_foil: usd_foil.map(str::to_string),
            ..crate::test_support::card_model(id)
        }
    }

    fn bucket<'a>(list: &'a [BreakdownBucket], key: &str) -> &'a BreakdownBucket {
        list.iter()
            .find(|b| b.key == key)
            .unwrap_or_else(|| panic!("bucket {key} missing from {list:?}"))
    }

    #[test]
    fn keys_bucket_rarity_colour_and_type() {
        assert_eq!(rarity_key(Some("Mythic")), "mythic");
        assert_eq!(rarity_key(Some("  ")), "unknown");
        assert_eq!(rarity_key(None), "unknown");

        assert_eq!(color_key(None), "colorless");
        assert_eq!(color_key(Some("")), "colorless");
        assert_eq!(color_key(Some("W")), "white");
        assert_eq!(color_key(Some("u")), "blue");
        assert_eq!(color_key(Some("B")), "black");
        assert_eq!(color_key(Some("R")), "red");
        assert_eq!(color_key(Some("G")), "green");
        assert_eq!(color_key(Some("W,U")), "multicolor");
        assert_eq!(color_key(Some("X")), "multicolor");

        assert_eq!(type_key(Some("Legendary Creature — Elf")), "creature");
        assert_eq!(type_key(Some("Artifact Creature — Golem")), "artifact");
        assert_eq!(type_key(Some("Basic Land — Forest")), "land");
        assert_eq!(type_key(Some("")), "other");
        assert_eq!(type_key(None), "other");
    }

    #[test]
    fn fold_slices_the_summary_total_and_ranks_by_held_value() {
        let rows = vec![
            // $2 regular ×3 + $10 foil ×1 = $16 — the most valuable holding, though its
            // single-copy price is not the highest.
            (
                holding(1, 3, 1),
                Some(card(
                    1,
                    Some("rare"),
                    Some("W,U"),
                    "Creature — Human",
                    Some("2.00"),
                    Some("10.00"),
                )),
            ),
            // $12 regular ×1 — the dearest single copy, second by held value.
            (
                holding(2, 1, 0),
                Some(card(
                    2,
                    Some("mythic"),
                    Some("R"),
                    "Legendary Creature — Dragon",
                    Some("12.00"),
                    None,
                )),
            ),
            // $0.50 ×4 — bulk land.
            (
                holding(3, 4, 0),
                Some(card(
                    3,
                    Some("common"),
                    None,
                    "Basic Land — Forest",
                    Some("0.50"),
                    None,
                )),
            ),
            // Unpriced in the one finish it's held in: counted, never valued or ranked.
            (
                holding(4, 0, 2),
                Some(card(
                    4,
                    None,
                    Some("G"),
                    "Artifact Creature — Golem",
                    Some("3.00"),
                    None,
                )),
            ),
            // A vanished card row: in no figure at all.
            (holding(5, 9, 9), None),
        ];

        let folded = fold_breakdown(&rows, 100, 10);

        // The embedded summary is the shared fold over the same rows.
        assert_eq!(folded.summary.unique_cards, 4);
        assert_eq!(folded.summary.total_cards, 11);
        assert_eq!(folded.summary.total_value_usd.as_deref(), Some("30.00"));
        assert_eq!(folded.summary.bulk_value_usd.as_deref(), Some("2.00"));

        // Rarity, in rarity order with `unknown` last, each a slice of the total.
        let keys: Vec<&str> = folded.rarity.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(keys, ["common", "rare", "mythic", "unknown"]);
        assert_eq!(
            bucket(&folded.rarity, "rare"),
            &BreakdownBucket {
                key: "rare".into(),
                cards: 1,
                copies: 4,
                value_usd: Some("16.00".into()),
            }
        );
        assert_eq!(bucket(&folded.rarity, "unknown").value_usd, None);
        assert_eq!(bucket(&folded.rarity, "unknown").copies, 2);

        // Colour, WUBRG then multicolor then colorless; only non-empty buckets.
        let keys: Vec<&str> = folded.color.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(keys, ["red", "green", "multicolor", "colorless"]);
        assert_eq!(
            bucket(&folded.color, "colorless").value_usd.as_deref(),
            Some("2.00")
        );

        // Type, most valuable first: creature ($28) > land ($2) > artifact (unpriced).
        let keys: Vec<&str> = folded.card_type.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(keys, ["creature", "land", "artifact"]);
        assert_eq!(bucket(&folded.card_type, "creature").cards, 2);

        // Finish: regular counts only regular copies, foil only foil copies.
        assert_eq!(
            folded.finish,
            vec![
                BreakdownBucket {
                    key: "regular".into(),
                    cards: 3,
                    copies: 8,
                    value_usd: Some("20.00".into()),
                },
                BreakdownBucket {
                    key: "foil".into(),
                    cards: 2,
                    copies: 3,
                    value_usd: Some("10.00".into()),
                },
            ]
        );

        // Top holdings by held value, not single-copy price; the unpriced one never ranks.
        let top: Vec<(i32, i128)> = folded.top.iter().map(|t| (t.0, t.3)).collect();
        assert_eq!(top, [(1, 1600), (2, 1200), (3, 200)]);
        assert_eq!(folded.unpriced_cards, 1);
    }

    #[test]
    fn fold_bounds_the_ranking_and_breaks_ties_by_id() {
        let rows: Vec<(collection_item::Model, Option<card::Model>)> = (1..=5)
            .map(|id| {
                (
                    holding(id, 1, 0),
                    Some(card(
                        id,
                        Some("common"),
                        None,
                        "Instant",
                        Some("1.00"),
                        None,
                    )),
                )
            })
            .collect();
        let folded = fold_breakdown(&rows, 100, 2);
        let top: Vec<i32> = folded.top.iter().map(|t| t.0).collect();
        assert_eq!(top, [1, 2]);
        // The buckets are still folded over every row, not the truncated ranking.
        assert_eq!(bucket(&folded.rarity, "common").cards, 5);
        assert_eq!(folded.finish[0].copies, 5);
    }

    #[test]
    fn fold_of_nothing_is_empty_everywhere() {
        let rows: Vec<(collection_item::Model, Option<card::Model>)> = Vec::new();
        let folded = fold_breakdown(&rows, 100, 10);
        assert_eq!(folded.summary.unique_cards, 0);
        assert_eq!(folded.summary.total_value_usd, None);
        assert!(folded.rarity.is_empty());
        assert!(folded.color.is_empty());
        assert!(folded.card_type.is_empty());
        assert!(folded.finish.is_empty());
        assert!(folded.top.is_empty());
        assert_eq!(folded.unpriced_cards, 0);
    }

    #[test]
    fn rarity_order_is_canonical_then_alphabetical_then_unknown() {
        let mut keys = vec!["unknown", "zeta", "mythic", "alpha", "common", "bonus"];
        keys.sort_by(|a, b| rarity_rank(a).cmp(&rarity_rank(b)));
        assert_eq!(
            keys,
            ["common", "mythic", "bonus", "alpha", "zeta", "unknown"]
        );
    }
}

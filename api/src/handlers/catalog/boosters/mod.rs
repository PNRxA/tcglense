//! Booster **expected value** and a simulated **pack opening** (issue #682): the two public
//! reads over the sheet/slot tables the sealed sync writes — `sealed_packs` (which boosters
//! one copy of a product opens, and how many), `booster_configs` (a booster's pack
//! variants) and `booster_sheets` (each variant slot's weighted card pool).
//!
//! ## What an expected value is, and what it is not
//!
//! An EV is an **average over many packs at today's market prices**: `Σ P(pull) × price`,
//! summed over every card on every sheet a pack draws from. It is not a valuation of the
//! pack in your hand — the whole point of a booster is that no single one is worth the
//! average — it is not a prediction, and it is not a *count of anything physical*. The
//! rest of a sealed product's page is bound by that last rule too (see `AGENTS.md`), and
//! these two reads are the only places a per-pack number is legitimate at all, precisely
//! because every one of them is worded as an expectation or as one simulated roll:
//!
//! * [`PackEv::cards_per_pack`] is an expectation over the pack's variants. A pack that is
//!   three-in-four a fifteen-card configuration and one-in-four a sixteen-card one holds
//!   15.25 cards on average and never that many in the hand.
//! * [`SlotEv::picks`] is how many cards a sheet contributes to an average pack, not how
//!   many it holds.
//! * [`PackCardOdds::one_in`] is "one in N packs, on average", never "your Nth pack".
//! * [`PackOpening`] is the other kind of number entirely: **one seeded roll of the dice**.
//!   Its `value_usd` is what this particular simulated run happened to deal, and saying
//!   anything else about it would be a lie about a random variable.
//!
//! Every response carries `caveats`, generated server-side and meant to be shown, so a
//! client cannot present these numbers stripped of what qualifies them.
//!
//! ## The approximations, all of them named on the wire
//!
//! * Each pick is treated as an **independent weighted draw** from its sheet. A real sheet
//!   without `allowDuplicates` is drawn without replacement, which shifts the odds only on
//!   a very small sheet — but the EV says so rather than pretending otherwise. (The
//!   *opener* does draw without replacement; it deals actual cards, where a repeat would be
//!   visible.)
//! * Upstream's colour balancing of common slots is recorded (`booster_sheets.balance_colors`)
//!   and **not** simulated.
//! * A card on a sheet that our catalog doesn't hold was dropped at ingest **but its weight
//!   stayed in the sheet's `total_weight`**, so the unaccounted share is knowable — and it
//!   is reported as a caveat instead of being silently re-normalised away. An unpriced card
//!   counts as $0 and is reported the same way, through `priced_share`.
//!
//! ## The two handlers
//!
//! * [`product_ev`] — `GET /api/games/{game}/products/{id}/ev`. `{ "data": null }` for a
//!   product with no `sealed_packs` rows (MTGJSON doesn't describe it, or its packs are a
//!   randomised `variable` choice with no defined EV), so the SPA can hide the panel
//!   without a second request. Cacheable like the rest of the catalog: prices move daily.
//! * [`open_product`] — `GET /api/games/{game}/products/{id}/open?seed=&copies=`. Stateless
//!   and seeded, exactly like the deck goldfish: the opening is a pure function of the URL,
//!   so there is no session table and a URL reproduces a run. It shares that read's
//!   generator ([`crate::handlers::shared::rng`]) rather than keeping a second copy, and it
//!   takes that read's `no-store` rule with it — **a seedless request mints a random seed,
//!   which makes the response not a function of its URL, so a shared cache must never pin
//!   one visitor's roll as everyone's.** A seeded request is ordinary cacheable catalog.
//!
//! The engines are pure and live beside this loader: [`ev`] (the maths) and [`open`] (the
//! draw). Both read the [`ResolvedPack`] list this module builds, so the odds a pack quotes
//! and the cards it deals can only ever come from the same rows.

use std::collections::{HashMap, HashSet};

use axum::{
    Json,
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};

use crate::entities::booster_config::Variant;
use crate::entities::booster_sheet::UNRESOLVED_CARD_ID;
use crate::entities::prelude::{BoosterConfig, BoosterSheet, Card, SealedPack};
use crate::entities::{booster_config, booster_sheet, card, sealed_pack};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::shared::valuation::price_cents;
use crate::handlers::shared::{CardResponse, DataBody, load_product, require_game};
use crate::state::AppState;

mod ev;
mod open;

/// Card ids per `IN (...)` chunk when loading a product's sheet cards — the same bound
/// [`super::products`] loads a product's cards under, well inside every backend's bind
/// limit.
const SHEET_CARDS_IN_CHUNK: usize = 900;

/// Most packs one `open` request may deal. A sealed booster box is 36 packs, so one copy of
/// the biggest thing anyone opens sits exactly at the limit; a case (six boxes) doesn't, on
/// purpose — 216 packs is a payload, not a read.
pub(crate) const MAX_PACKS_PER_OPENING: u32 = 36;

/// Most cards one `open` request may deal, checked **before** anything is drawn from the
/// largest variant of each pack. The pack count alone doesn't bound the work: a `quantity`
/// and a slot count both come from ingested data, so this is the bound that actually caps
/// the response. A 36-pack box of fifteen-card packs deals 540.
pub(crate) const MAX_CARDS_PER_OPENING: u64 = 1_200;

// ---------- Wire DTOs ----------

/// The expected value of **one copy** of a sealed product at today's prices — an average
/// over many openings, never a valuation of the copy in front of you (see the module docs).
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "ProductEv"))]
pub(crate) struct ProductEv {
    /// 2-dp USD: `Σ pack.quantity × pack.ev_usd` over every booster one copy opens, summed
    /// before rounding (so it can differ by a cent from adding up the rendered per-pack
    /// figures — the unrounded sum is the honest one).
    pub ev_usd: String,
    /// Every distinct booster one copy of the product opens, with how many of each.
    pub packs: Vec<PackEv>,
    /// The biggest expected contributors across the whole copy (at most 12), by
    /// `contribution_usd` descending — which for this list is **per copy**
    /// (`expected_per_pack × price × quantity`), while `expected_per_pack` and `one_in`
    /// stay per pack, because that's the unit odds are quoted in.
    pub top: Vec<PackCardOdds>,
    /// What qualifies these numbers, generated server-side. Show them.
    pub caveats: Vec<String>,
}

/// The expected value of **one pack** of one booster configuration.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PackEv"))]
pub(crate) struct PackEv {
    /// The set the booster belongs to (lowercased code, e.g. `blb`).
    pub set_code: String,
    /// MTGJSON's booster key (`play`, `collector`, `draft`, …).
    pub booster_code: String,
    /// The booster's display name (`Play Booster`) when upstream states one.
    pub name: Option<String>,
    /// How many of this pack **one copy** of the product opens.
    pub quantity: u32,
    /// Cards in an average pack: `Σ_variants P(variant) × Σ slot counts`. An expectation —
    /// a pack that is sometimes fifteen cards and sometimes sixteen reports a fraction.
    pub cards_per_pack: f64,
    /// 2-dp USD for **one** pack.
    pub ev_usd: String,
    /// `0..1` — the share of the pack's expected picks that fall on a card with a market
    /// price. Everything else counts as $0.
    pub priced_share: f64,
    /// One entry per sheet the pack draws from, in the order the sheets first appear in the
    /// configuration's variants.
    pub slots: Vec<SlotEv>,
    /// The pack's biggest expected contributors (at most 10), by `contribution_usd` desc.
    pub top: Vec<PackCardOdds>,
}

/// What one **sheet** contributes to an average pack.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "SlotEv"))]
pub(crate) struct SlotEv {
    /// The sheet's name (`common`, `rareMythic`, `foil`, …) — upstream's, not a label.
    pub sheet: String,
    /// Whether the sheet is foil (its cards are valued at their foil price).
    pub foil: bool,
    /// Cards this sheet contributes to an average pack: `Σ_variants P(variant) × count`.
    pub picks: f64,
    /// 2-dp USD this sheet contributes to an average pack.
    pub ev_usd: String,
    /// Distinct cards our catalog holds for this sheet.
    pub card_count: u32,
    /// `0..1` — the share of this sheet's picks that fall on a priced card. Below 1 either
    /// because some cards have no market price or because part of the sheet's weight is on
    /// cards the catalog doesn't hold.
    pub priced_share: f64,
    /// The sheet's biggest expected contributors (at most 3).
    pub top: Vec<PackCardOdds>,
}

/// One card's pull odds and what those odds are worth — the unit both `top` lists are made
/// of. Never emitted for a card that can't be pulled, so `one_in` is always finite.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PackCardOdds"))]
pub(crate) struct PackCardOdds {
    pub card: CardResponse,
    /// Whether it is pulled foil (it sits on a foil sheet), which is the price used.
    pub foil: bool,
    /// The sheet it is pulled from.
    pub sheet: String,
    /// Expected copies of this card in one pack — a with-replacement approximation.
    pub expected_per_pack: f64,
    /// `1 / expected_per_pack`: "one in N packs, on average". Finite by construction.
    pub one_in: f64,
    /// The price used, 2-dp USD; `null` when the card has no market price (it then counts
    /// as $0).
    pub price_usd: Option<String>,
    /// `expected_per_pack × price`, 2-dp USD — per pack, except in
    /// [`ProductEv::top`], where it is per copy (× the pack's `quantity`).
    pub contribution_usd: String,
}

/// One simulated opening: stateless, seeded, and a pure function of its URL. What this run
/// of the dice dealt — not what the product is worth.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PackOpening"))]
pub(crate) struct PackOpening {
    /// The seed this opening was rolled with — echoed so a random one can be replayed or
    /// shared as a URL.
    pub seed: u32,
    /// How many copies of the product were opened.
    pub copies: u32,
    /// Every pack, in opening order: copy by copy, and within a copy in the product's own
    /// pack order. Pack *n* of an opening is the same pack whatever `copies` was, so
    /// opening more is opening the same run for longer.
    pub packs: Vec<OpenedPack>,
    /// 2-dp USD of everything pulled (unpriced cards count as $0).
    pub value_usd: String,
    /// Pulled cards that had a market price.
    pub priced_count: u32,
    /// Pulled cards that had none.
    pub unpriced_count: u32,
    /// What qualifies this run, generated server-side. Show them.
    pub caveats: Vec<String>,
}

/// One pack of a simulated opening.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "OpenedPack"))]
pub(crate) struct OpenedPack {
    pub set_code: String,
    pub booster_code: String,
    pub name: Option<String>,
    /// Index of the rolled variant within the configuration's variant list.
    pub variant: u32,
    /// The cards dealt, slot by slot in the rolled variant's order.
    pub cards: Vec<OpenedCard>,
    /// 2-dp USD of this pack's pulls.
    pub value_usd: String,
}

/// One card dealt by a simulated opening.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "OpenedCard"))]
pub(crate) struct OpenedCard {
    pub card: CardResponse,
    /// Whether it was dealt off a foil sheet (which is the price used).
    pub foil: bool,
    /// The sheet it came off.
    pub sheet: String,
    /// 2-dp USD, `null` when the card has no market price.
    pub price_usd: Option<String>,
}

/// Query string of the pack opener. Everything the run depends on is here, so the same URL
/// always deals the same cards.
#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
pub struct OpenParams {
    /// Roll seed. Omit for a fresh random one — the response echoes it back, and the
    /// response is then `no-store` because it isn't a function of its URL.
    pub seed: Option<u32>,
    /// How many copies of the product to open (default 1).
    pub copies: Option<u32>,
}

// ---------- The in-memory shape both engines read ----------

/// One sheet of a booster configuration, resolved against the catalog: the flags, the
/// denominator, and the `(card id, weight)` pairs **whose card row we actually hold**, in
/// upstream order (a `fixed` sheet is dealt in that order, so it is load-bearing).
pub(super) struct ResolvedSheet {
    pub name: String,
    pub foil: bool,
    /// Upstream balances colours on this sheet. Recorded, never simulated — it becomes a
    /// caveat.
    pub balance_colors: bool,
    /// Draws from this sheet may repeat a card.
    pub allow_duplicates: bool,
    /// The sheet is a fixed list: a slot takes its first `count` cards, in order.
    pub fixed: bool,
    /// The denominator a card's weight is a share of, **including** the weight of cards our
    /// catalog doesn't hold — so `total_weight - Σ cards' weights` is the unaccounted
    /// share, which the caveats report rather than re-normalising away.
    pub total_weight: u64,
    /// `(card id, weight)` in stored order. On a `fixed` sheet an entry may be
    /// [`UNRESOLVED_CARD_ID`]: a position we can't name, kept so the cards after it stay
    /// where they belong. It consumes its pick and is worth nothing.
    pub cards: Vec<(i32, u32)>,
}

impl ResolvedSheet {
    /// Σ of the stored cards' weights — what an opening can actually deal.
    /// [`UNRESOLVED_CARD_ID`] placeholders don't count: they hold a fixed sheet's position,
    /// not a card, so their weight is as unaccounted for as a dropped card's.
    fn stored_weight(&self) -> u64 {
        self.cards
            .iter()
            .filter(|&&(id, _)| id != UNRESOLVED_CARD_ID)
            .map(|&(_, w)| u64::from(w))
            .sum()
    }

    /// The denominator to price against: upstream's `total_weight`, falling back to the
    /// stored weights when a row states none (then nothing is unaccounted for).
    fn denominator(&self) -> u64 {
        if self.total_weight > 0 {
            self.total_weight
        } else {
            self.stored_weight()
        }
    }

    /// The share of this sheet's weight sitting on cards the catalog doesn't hold, `0.0`
    /// when it is fully accounted for.
    fn unaccounted_share(&self) -> f64 {
        let total = self.denominator();
        if total == 0 {
            return 0.0;
        }
        let missing = total.saturating_sub(self.stored_weight());
        missing as f64 / total as f64
    }
}

/// One booster configuration, resolved: its identity, its pack variants, and its sheets.
pub(super) struct ResolvedConfig {
    pub set_code: String,
    pub code: String,
    pub name: Option<String>,
    /// The denominator a variant's weight is a share of (Σ of the stored variants').
    pub total_weight: u64,
    pub variants: Vec<Variant>,
    pub sheets: Vec<ResolvedSheet>,
}

impl ResolvedConfig {
    /// The denominator to roll a variant against: the stored total, falling back to Σ of
    /// the variants' own weights when the column says nothing.
    pub(super) fn variant_denominator(&self) -> u64 {
        if self.total_weight > 0 {
            self.total_weight
        } else {
            self.variants.iter().map(|v| v.weight).sum()
        }
    }

    /// The sheet a variant slot names, or `None` — a slot may name a sheet the ingest
    /// didn't store, and a missing sheet is skipped rather than failing the read.
    pub(super) fn sheet(&self, name: &str) -> Option<&ResolvedSheet> {
        self.sheets.iter().find(|s| s.name == name)
    }
}

/// "One copy of this product opens `quantity` of this booster."
pub(super) struct ResolvedPack {
    pub quantity: u32,
    pub config: ResolvedConfig,
}

/// The **prices** of every card a product's sheets name, parsed to cents once.
///
/// Deliberately not the card *rows*. A request has to price every card on every sheet — a
/// product opening play and collector boosters names a couple of thousand — but only ever
/// renders a few dozen of them: at most twelve top contributors per slot, and at most
/// [`MAX_CARDS_PER_OPENING`] dealt cards. So the loader reads three columns here and fetches
/// whole rows afterwards, for the ids an engine actually put on the wire ([`ResolvedRead`]).
/// Parsing the stored decimal per lookup would be the read's hot loop — a card sits on
/// several sheets and is read once per slot — so it happens once, here.
pub(super) struct CardIndex {
    prices: HashMap<i32, (Option<i128>, Option<i128>)>,
}

impl CardIndex {
    /// Build from the `(id, price_usd, price_usd_foil)` tuples the loader's first pass
    /// selected.
    fn from_price_rows(rows: Vec<(i32, Option<String>, Option<String>)>) -> Self {
        let prices = rows
            .into_iter()
            .map(|(id, usd, usd_foil)| {
                (
                    id,
                    (
                        price_cents(usd.as_deref()),
                        price_cents(usd_foil.as_deref()),
                    ),
                )
            })
            .collect();
        Self { prices }
    }

    /// The price a sheet values this card at, in cents: the foil price on a foil sheet, the
    /// regular price otherwise. Deliberately **not** falling back to the other finish — a
    /// foil sheet deals foils, and pricing one at its non-foil price would be a different
    /// card's price.
    pub(super) fn price_cents(&self, id: i32, foil: bool) -> Option<i128> {
        let (usd, usd_foil) = self.prices.get(&id)?;
        if foil { *usd_foil } else { *usd }
    }

    /// Whether the catalog still holds this card. A sheet card that isn't here was dropped
    /// by a re-import since the sync wrote the sheet; the loader filters those out so
    /// neither engine can plan a card it will then fail to render.
    fn contains(&self, id: i32) -> bool {
        self.prices.contains_key(&id)
    }
}

/// The catalog payloads for the cards an engine's plan names, keyed by internal id — what
/// the second pass loads and what a plan is rendered against.
pub(super) type CardResponses = HashMap<i32, CardResponse>;

// ---------- Shared caveat material ----------

/// Whether any sheet of any pack is colour-balanced upstream (which we don't simulate).
pub(super) fn any_balance_colors(packs: &[ResolvedPack]) -> bool {
    packs
        .iter()
        .flat_map(|p| p.config.sheets.iter())
        .any(|s| s.balance_colors)
}

/// The sheets holding weight on cards the catalog doesn't hold, worded for a caveat:
/// `"play/common (12% of its weight)"`, worst first, at most three named and the rest
/// counted. `None` when every sheet is fully accounted for.
pub(super) fn unaccounted_sheets(packs: &[ResolvedPack]) -> Option<String> {
    let mut missing: Vec<(String, f64)> = Vec::new();
    for pack in packs {
        for sheet in &pack.config.sheets {
            let share = sheet.unaccounted_share();
            if share > 0.0 {
                missing.push((format!("{}/{}", pack.config.code, sheet.name), share));
            }
        }
    }
    if missing.is_empty() {
        return None;
    }
    // Worst first, then by name so the sentence is stable across requests.
    missing.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    let extra = missing.len().saturating_sub(3);
    let named: Vec<String> = missing
        .iter()
        .take(3)
        .map(|(name, share)| format!("{name} ({} of its weight)", percent(*share)))
        .collect();
    let mut sentence = named.join(", ");
    if extra > 0 {
        sentence.push_str(&format!(", and {extra} more"));
    }
    Some(sentence)
}

/// A `0..1` share as a whole-percent label that never rounds away the fact it qualifies:
/// a non-zero share is never "0%" (a sheet that is 0.4% unaccounted for is "<1%", which
/// would otherwise read as "none"), and a share short of one is never "100%" (a caveat
/// saying "cards without a price count as $0 — 100% of the picks are priced" contradicts
/// itself; ">99%" is what a 99.7% share honestly is).
pub(super) fn percent(share: f64) -> String {
    if !share.is_finite() || share <= 0.0 {
        return "0%".to_string();
    }
    if share >= 1.0 {
        return "100%".to_string();
    }
    let pct = share * 100.0;
    if pct < 0.5 {
        return "<1%".to_string();
    }
    if pct >= 99.5 {
        return ">99%".to_string();
    }
    format!("{pct:.0}%")
}

/// Round an f64 cent amount to whole cents and render it as 2-dp USD. A non-finite input
/// (which no priced path can produce) renders as `0.00` rather than `NaN` reaching a client.
pub(super) fn usd(cents: f64) -> String {
    if !cents.is_finite() {
        return crate::handlers::shared::valuation::format_cents(0);
    }
    crate::handlers::shared::valuation::format_cents(cents.round() as i128)
}

// ---------- Loading ----------

/// A stored sheet row paired with its already-decoded `(card id, weight)` pairs. The loader
/// decodes the JSON column once, while collecting the card ids it has to fetch, rather than
/// parsing it a second time to build the [`ResolvedSheet`].
type DecodedSheet = (booster_sheet::Model, Vec<(i32, u32)>);

/// What one product's boosters resolve to: the packs one copy opens (in `sealed_packs`
/// order — configuration id ascending, which is also the opening order) and the prices of
/// every card their sheets name.
///
/// This is the **first** of the read's two passes. The second ([`load_card_responses`])
/// fetches whole card rows, and only for the ids the engine's plan puts on the wire.
pub(super) struct ResolvedRead {
    pub packs: Vec<ResolvedPack>,
    pub index: CardIndex,
}

/// Load everything the two engines need to *compute* for one product: the boosters one copy
/// opens, and the price of every catalog card their sheets name — three columns per card,
/// never the whole row.
///
/// A sheet card whose row has since vanished is dropped from the sheet **but its weight
/// stays in `total_weight`**, exactly as the ingest treats a card it couldn't resolve, so a
/// re-import that removed a printing shows up as unaccounted weight rather than as a
/// silently richer pack.
async fn load_boosters(
    state: &AppState,
    game: &str,
    product_id: i32,
) -> Result<ResolvedRead, AppError> {
    let packs = SealedPack::find()
        .filter(sealed_pack::Column::Game.eq(game))
        .filter(sealed_pack::Column::ProductId.eq(product_id))
        .order_by_asc(sealed_pack::Column::ConfigId)
        .all(&state.db)
        .await?;
    if packs.is_empty() {
        return Ok(ResolvedRead {
            packs: Vec::new(),
            index: CardIndex::from_price_rows(Vec::new()),
        });
    }

    let config_ids: Vec<i32> = packs.iter().map(|p| p.config_id).collect();
    let configs: HashMap<i32, booster_config::Model> = BoosterConfig::find()
        .filter(booster_config::Column::Id.is_in(config_ids.clone()))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|c| (c.id, c))
        .collect();

    let sheet_rows = BoosterSheet::find()
        .filter(booster_sheet::Column::ConfigId.is_in(config_ids))
        .order_by_asc(booster_sheet::Column::Id)
        .all(&state.db)
        .await?;

    // Every distinct card any sheet names, loaded in chunks under the bind limit.
    let mut wanted: Vec<i32> = Vec::new();
    let mut seen: HashSet<i32> = HashSet::new();
    let mut sheets_by_config: HashMap<i32, Vec<DecodedSheet>> = HashMap::new();
    for row in sheet_rows {
        let cards = row.cards();
        for &(id, _) in &cards {
            if seen.insert(id) {
                wanted.push(id);
            }
        }
        sheets_by_config
            .entry(row.config_id)
            .or_default()
            .push((row, cards));
    }

    // Prices only, in chunks under the bind limit. `select_only` matters: a `cards` row is
    // ~70 columns and a big product names thousands of them, so pulling whole rows here to
    // read two decimal strings was the read's dominant cost — and every one of those rows
    // was then dropped, since the wire carries a few dozen cards at most.
    let mut price_rows: Vec<(i32, Option<String>, Option<String>)> =
        Vec::with_capacity(wanted.len());
    for chunk in wanted.chunks(SHEET_CARDS_IN_CHUNK) {
        let mut rows: Vec<(i32, Option<String>, Option<String>)> = Card::find()
            .select_only()
            .column(card::Column::Id)
            .column(card::Column::PriceUsd)
            .column(card::Column::PriceUsdFoil)
            .filter(card::Column::Game.eq(game))
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(&state.db)
            .await?;
        price_rows.append(&mut rows);
    }
    let index = CardIndex::from_price_rows(price_rows);

    let resolved = packs
        .into_iter()
        .filter_map(|pack| {
            let config = configs.get(&pack.config_id)?;
            let sheets = sheets_by_config
                .remove(&pack.config_id)
                .unwrap_or_default()
                .into_iter()
                .map(|(row, cards)| {
                    let fixed = row.fixed;
                    ResolvedSheet {
                        name: row.name,
                        foil: row.foil,
                        balance_colors: row.balance_colors,
                        allow_duplicates: row.allow_duplicates,
                        fixed,
                        total_weight: row.total_weight.max(0) as u64,
                        cards: cards
                            .into_iter()
                            // A card whose row went since the sync drops out — except on a
                            // fixed sheet, where a position has to stay a position. The
                            // ingest already writes an unresolvable one as the sentinel;
                            // this keeps it, and turns a vanished row into one too.
                            .filter_map(|(id, weight)| match index.contains(id) {
                                true => Some((id, weight)),
                                false if fixed => Some((UNRESOLVED_CARD_ID, weight)),
                                false => None,
                            })
                            .collect(),
                    }
                })
                .collect();
            Some(ResolvedPack {
                quantity: pack.quantity.max(0) as u32,
                config: ResolvedConfig {
                    set_code: config.set_code.clone(),
                    code: config.code.clone(),
                    name: config.name.clone(),
                    total_weight: config.total_weight.max(0) as u64,
                    variants: config.variants(),
                    sheets,
                },
            })
        })
        .collect();

    Ok(ResolvedRead {
        packs: resolved,
        index,
    })
}

/// The read's **second** pass: the full catalog rows for the cards an engine's plan names,
/// dressed for the wire. A plan holds a few dozen ids at most, so this is one small chunked
/// query rather than the thousands of rows the sheets themselves cover.
///
/// A card whose row vanished between the two passes simply isn't in the map, and the plan
/// drops that line — the same tolerance every card link in the catalog takes.
async fn load_card_responses(
    state: &AppState,
    game: &str,
    ids: &[i32],
) -> Result<CardResponses, AppError> {
    let mut responses = CardResponses::with_capacity(ids.len());
    for chunk in ids.chunks(SHEET_CARDS_IN_CHUNK) {
        let rows = Card::find()
            .filter(card::Column::Game.eq(game))
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .all(&state.db)
            .await?;
        for row in rows {
            responses.insert(row.id, CardResponse::from(row));
        }
    }
    Ok(responses)
}

// ---------- Handlers ----------

/// Get a sealed product's expected value
///
/// `GET /api/games/{game}/products/{id}/ev` -> what one copy of the product is worth on
/// average at today's prices, broken down per booster and per sheet.
///
/// `{ "data": null }` — not a 404 — when the product has no booster data: MTGJSON doesn't
/// describe it, it isn't a booster at all (a commander deck opens nothing), or its packs
/// are a randomised `variable` choice, which has no defined expected value. A client can
/// therefore hide the panel from one response rather than treating an error as an answer.
#[utoipa::path(
    get,
    path = "/api/games/{game}/products/{id}/ev",
    tag = "Sealed products",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "Product id"),
    ),
    responses(
        (status = 200, description = "The product's expected value, or `null` when it has no booster data.", body = DataBody<Option<ProductEv>>),
        (status = 404, description = "Unknown game or product."),
    ),
)]
pub async fn product_ev(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<DataBody<Option<ProductEv>>>, AppError> {
    require_game(&game)?;
    let product = load_product(&state, &game, &id).await?;
    let read = load_boosters(&state, &game, product.id).await?;
    if read.packs.is_empty() {
        return Ok(Json(DataBody { data: None }));
    }
    // The maths runs on prices alone and names the handful of cards its `top` lists quote;
    // only those get their full catalog row fetched.
    let plan = ev::evaluate(&read.packs, &read.index);
    let responses = load_card_responses(&state, &game, &plan.card_ids()).await?;
    Ok(Json(DataBody {
        data: Some(plan.render(&responses)),
    }))
}

/// Open a sealed product
///
/// `GET /api/games/{game}/products/{id}/open?seed=&copies=` -> one simulated opening of the
/// product: every pack it holds, rolled variant by rolled variant, with what each slot
/// dealt and what the pulls are worth at today's prices.
///
/// Stateless and seeded like the deck goldfish: the run is a pure function of
/// `(product, seed, copies)`, all of which ride in the URL, so a surprising box can be
/// handed to someone else as a link and open the same way.
///
/// **A seedless request answers `no-store`.** Without a `seed` the roll is random, so the
/// response is not a function of its URL — and this route sits in the public *catalog*
/// cache group (`s-maxage=3600` plus a day of `stale-while-revalidate`), where a shared
/// cache would otherwise pin one anonymous visitor's box as *the* box for the best part of
/// a day. Same rule, and the same reason, as the goldfish's.
#[utoipa::path(
    get,
    path = "/api/games/{game}/products/{id}/open",
    tag = "Sealed products",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("id" = String, Path, description = "Product id"),
        OpenParams,
    ),
    responses(
        (status = 200, description = "One seeded opening.", body = PackOpening),
        (status = 404, description = "Unknown game or product."),
        (status = 422, description = "The product has no booster data, or the opening would be too large."),
    ),
)]
pub async fn open_product(
    State(state): State<AppState>,
    Path((game, id)): Path<(String, String)>,
    Query(params): Query<OpenParams>,
) -> Result<Response, AppError> {
    require_game(&game)?;
    let product = load_product(&state, &game, &id).await?;
    let read = load_boosters(&state, &game, product.id).await?;

    let seedless = params.seed.is_none();
    let seed = params.seed.unwrap_or_else(rand::random::<u32>);
    let copies = params.copies.unwrap_or(1);
    // The draw is decided from the sheets and the seed; the cards it dealt — bounded by
    // `MAX_CARDS_PER_OPENING`, and a fresh roll on every click — are the only rows fetched.
    let plan = open::open_packs(&read.packs, &read.index, seed, copies)?;
    let responses = load_card_responses(&state, &game, &plan.card_ids()).await?;

    let mut response = Json(plan.render(&responses)).into_response();
    if seedless {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
    }
    Ok(response)
}

#[cfg(test)]
pub(super) mod test_support {
    //! Tiny hand-built configurations the two engines' unit tests share, so the maths and
    //! the draw are pinned against the *same* fixture and can't quietly diverge.

    use super::*;
    use crate::test_support::card_model;

    /// A price index straight from `(card id, usd, usd_foil)`. The engines read nothing but
    /// prices, so their tests never need a card row to compute against — which is the whole
    /// point of the loader's first pass.
    pub(super) fn index(prices: &[(i32, Option<&str>, Option<&str>)]) -> CardIndex {
        CardIndex::from_price_rows(
            prices
                .iter()
                .map(|(id, usd, usd_foil)| {
                    (*id, usd.map(str::to_string), usd_foil.map(str::to_string))
                })
                .collect(),
        )
    }

    /// The stub payload map a plan is rendered against — one default catalog row per id,
    /// standing in for the loader's second pass. `Card.id` on the wire is `ext-<id>`.
    pub(super) fn responses(ids: &[i32]) -> CardResponses {
        ids.iter()
            .map(|&id| (id, CardResponse::from(card_model(id))))
            .collect()
    }

    pub(super) fn sheet(
        name: &str,
        foil: bool,
        total_weight: u64,
        cards: &[(i32, u32)],
    ) -> ResolvedSheet {
        ResolvedSheet {
            name: name.to_string(),
            foil,
            balance_colors: false,
            allow_duplicates: false,
            fixed: false,
            total_weight,
            cards: cards.to_vec(),
        }
    }

    pub(super) fn variant(weight: u64, slots: &[(&str, u32)]) -> Variant {
        Variant {
            weight,
            slots: slots
                .iter()
                .map(|(name, count)| ((*name).to_string(), *count))
                .collect(),
        }
    }

    pub(super) fn pack(quantity: u32, config: ResolvedConfig) -> ResolvedPack {
        ResolvedPack { quantity, config }
    }

    pub(super) fn config(variants: Vec<Variant>, sheets: Vec<ResolvedSheet>) -> ResolvedConfig {
        let total_weight = variants.iter().map(|v| v.weight).sum();
        ResolvedConfig {
            set_code: "tst".to_string(),
            code: "play".to_string(),
            name: Some("Play Booster".to_string()),
            total_weight,
            variants,
            sheets,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn percent_never_rounds_a_real_share_down_to_nothing() {
        assert_eq!(percent(0.0), "0%");
        assert_eq!(percent(0.004), "<1%");
        assert_eq!(percent(0.12), "12%");
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(f64::NAN), "0%");
    }

    /// The mirror case at the top: a share that is *nearly* whole must not print as whole,
    /// or the caveat it qualifies ("cards without a price count as $0 — 100% of the picks
    /// are priced") contradicts itself on the very product that has one unpriced card.
    #[test]
    fn percent_never_rounds_a_short_share_up_to_whole() {
        assert_eq!(percent(0.9971), ">99%");
        assert_eq!(percent(0.995), ">99%");
        assert_eq!(percent(0.994), "99%");
        assert_eq!(percent(1.0 - 1e-12), ">99%");
        assert_eq!(
            percent(1.2),
            "100%",
            "an over-one share still reads as whole"
        );
    }

    #[test]
    fn usd_rounds_to_cents_and_never_emits_nan() {
        assert_eq!(usd(1234.4), "12.34");
        assert_eq!(usd(1234.6), "12.35");
        assert_eq!(usd(f64::INFINITY), "0.00");
    }

    #[test]
    fn a_sheet_reports_the_weight_its_cards_do_not_account_for() {
        // 100 total, 40 stored: 60% of the sheet isn't in the catalog.
        let s = sheet("common", false, 100, &[(1, 25), (2, 15)]);
        assert!((s.unaccounted_share() - 0.6).abs() < 1e-9);
        assert_eq!(s.denominator(), 100);

        // No stated total: the stored weights *are* the denominator, nothing is missing.
        let s = sheet("common", false, 0, &[(1, 3), (2, 1)]);
        assert_eq!(s.denominator(), 4);
        assert_eq!(s.unaccounted_share(), 0.0);
    }

    #[test]
    fn the_unaccounted_caveat_names_the_worst_sheets_and_counts_the_rest() {
        let packs = vec![pack(
            1,
            config(
                vec![variant(1, &[("a", 1)])],
                vec![
                    sheet("a", false, 100, &[(1, 90)]),
                    sheet("b", false, 100, &[(1, 50)]),
                    sheet("c", false, 100, &[(1, 10)]),
                    sheet("d", false, 100, &[(1, 20)]),
                    sheet("whole", false, 10, &[(1, 10)]),
                ],
            ),
        )];
        let sentence = unaccounted_sheets(&packs).expect("some weight is unaccounted for");
        assert_eq!(
            sentence,
            "play/c (90% of its weight), play/d (80% of its weight), \
             play/b (50% of its weight), and 1 more"
        );

        // A fully-resolved configuration says nothing at all.
        let whole = vec![pack(
            1,
            config(
                vec![variant(1, &[("a", 1)])],
                vec![sheet("a", false, 4, &[(1, 3), (2, 1)])],
            ),
        )];
        assert!(unaccounted_sheets(&whole).is_none());
    }

    #[test]
    fn a_foil_sheet_prices_the_foil_finish_and_never_falls_back() {
        let index = index(&[(1, Some("2.00"), Some("9.00")), (2, Some("1.00"), None)]);
        assert_eq!(index.price_cents(1, false), Some(200));
        assert_eq!(index.price_cents(1, true), Some(900));
        // Priced regular, unpriced foil: a foil sheet reports no price rather than
        // quoting the non-foil one, which is a different card.
        assert_eq!(index.price_cents(2, true), None);
        assert_eq!(index.price_cents(99, false), None);
    }
}

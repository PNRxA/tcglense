//! Foil-variant pairing: the price enrichment (issue #209) and the catalog-listing fold.
//!
//! Some sets model a card's **foil** printing as a *separate* Scryfall object whose collector
//! number is the nonfoil's plus a star (U+2605): `sld` `741` (nonfoil) and `741★` (foil).
//! Scryfall keeps the foil price only on the `741★` object; the nonfoil base `741` carries a
//! nonfoil price and an **empty foil price**.
//!
//! The collection consolidates a foil-★ holding onto its nonfoil base as a foil copy (see
//! `crate::collection_import::consolidate`), and collection valuation prices a foil copy from
//! its card's `price_usd_foil` — which on the base is empty, so a folded foil would value at
//! $0. [`enrich_foil_variant_prices`] copies each foil-★ sibling's foil price onto its nonfoil
//! base so the base carries **both** prices, and the folded foil values correctly (and the
//! public catalog shows the base's foil price too). Runs on every sync tick, before the daily
//! price snapshot, so the enriched price is captured into the base card's history like any
//! other.
//!
//! Because the base then carries both prices, such a star can be a **duplicate tile** in a
//! card grid — "Secret Lair x Hatsune Miku: Sakura Superstar" listed `1587` and `1587★` side
//! by side, both showing the same $12.33 foil. [`refresh_foil_variant_folds`] decides which
//! stars that is true of and records the answer in `cards.folded_onto_id`; the listings then
//! filter on [`not_folded_foil_variant`], and [`has_folded_foil_variant`] is the mirror `is:`
//! leaves read so a folded star's finish and treatment stay searchable on its base.
//!
//! **Two rules, not one — this is the module's whole subtlety.** The pairing above (foil-only
//! star ↔ nonfoil base, same set + oracle id + collector number sans the star) is what
//! [`enrich_foil_variant_prices`], `collection_import::consolidate` and the `m..023` migration
//! share, and it matches **1,626 pairs catalog-wide**. That is the right rule for copying a
//! price, where a wrong pair costs nothing. It is the wrong rule for *hiding a row*, where a
//! wrong pair costs a printing: two thirds of those pairs are 7ed/8ed/9ed/10e-era cards whose
//! foil is **black-bordered** where the nonfoil is white, or Fate Reforged cards whose foil
//! carries a watermark — genuinely different cards, and ones `border:` / `wm:` / `art:` query
//! directly. So the fold applies a strictly narrower test on top: [`same_printed_card`], which
//! folds only ~550 pairs and spares every star a visitor could tell apart from its base.
//!
//! The star row itself is **kept** either way: its Scryfall id is a live wire id (card detail,
//! collection/wishlist/deck/alert rows, provider imports all resolve it), so this is a
//! presentation fold, never a delete.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use sea_orm::sea_query::{Expr, SimpleExpr};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    FromQueryResult, QueryFilter, QuerySelect, Select, SelectModel, Selector, Statement,
};

use super::ingest::IngestError;
use super::search::{cust_vals, escape_like};
use crate::db::Dialect;
use crate::entities::card;
use crate::entities::prelude::Card;

/// The Scryfall foil-variant collector-number suffix (U+2605 BLACK STAR).
///
/// The canonical spelling, re-exported at `crate::scryfall::FOIL_STAR` so
/// `collection_import` (which folds the *holding*) and this module (which folds the *listing*)
/// can't drift on what a foil-★ variant even is.
pub(crate) const FOIL_STAR: char = '★';

/// How many `(set_code, collector_number)` pairs to resolve per base-lookup statement. Two
/// binds each, well under SQLite's per-statement cap — the same bound
/// `collection_import::consolidate` chunks its identical lookup by.
const BASE_LOOKUP_CHUNK: usize = 400;

/// Copy every foil-★ sibling's `price_usd_foil` onto its nonfoil base card. Idempotent and
/// safe to run every tick: the nonfoil base never carries its own foil price, so overwriting
/// it with the current sibling price just keeps it fresh as prices move. Returns the number of
/// base rows updated (for logging).
///
/// **Star-driven** (issue-#209 follow-up perf, `m..044`): the `…★` foil stars are a tiny set
/// (~1,851 rows against ~40k nonfoil bases and ~106k cards), so the `UPDATE` starts from them —
/// found through the partial index `idx_cards_foil_variant_star` on `finishes = 'foil' AND
/// collector_number LIKE '%★'` — and joins each back to its base by **stripping the trailing
/// star** (`substr(..., 1, length(...) - 1)`; the star is a single char, so this is exact). The
/// earlier base-driven form re-derived the match with a correlated subquery per candidate, which
/// made the planner sequential-scan the whole wide `cards` heap twice (once for the ~40k nonfoil
/// bases, once to hash the ~12k foil rows) — ~8 s on the weak, cold prod Postgres. This form
/// scans only the ~1,851 stars and point-seeks each base, ~3.9× fewer buffers there, and is
/// verified to produce byte-identical results (0 mismatches over the 1,627 real pairs).
///
/// Cross-backend plain SQL: `UPDATE…FROM` self-join + `substr`/`length` + `LIKE` + `||` concat,
/// all of which render byte-identically on SQLite (≥ 3.33 for `UPDATE…FROM`; the shipping
/// `IS DISTINCT FROM` below already requires ≥ 3.39) and Postgres — no `db::Dialect` gate.
/// Game-agnostic (the star↔base join is same-game), so the star convention is handled for
/// whatever game has such pairs; today only MTG does.
pub(crate) async fn enrich_foil_variant_prices(
    db: &DatabaseConnection,
) -> Result<u64, IngestError> {
    // Stamp `updated_at` too, and only on the rows this actually changes (the `IS DISTINCT
    // FROM` guard). This is a **live price write**, so it must bump `updated_at` exactly like
    // the guarded card upsert does — the price-alert evaluator's change-narrowing
    // (`crate::alerts`) treats `cards.updated_at` as "a datum (incl. foil price) changed", and
    // for ★-variant Secret Lair cards the base's foil price arrives *only* through this path,
    // so an un-stamped write here would hide a foil-price crossing from the narrowed scan. The
    // `?` binds a chrono `Value` (the same encoding SeaORM stores `updated_at` with, so the
    // narrowing's `updated_at >= since` comparison lines up); the shared `Dialect::placeholders`
    // seam renumbers it to `$1` on Postgres.
    let backend = db.get_database_backend();
    let sql = crate::db::Dialect::from_backend(backend).placeholders(ENRICH_SQL);
    let stmt = Statement::from_sql_and_values(backend, sql, [Utc::now().into()]);
    let result = db.execute(stmt).await?;
    Ok(result.rows_affected())
}

const ENRICH_SQL: &str = r#"
UPDATE cards AS base
SET price_usd_foil = star.price_usd_foil, updated_at = ?
FROM cards AS star
WHERE star.finishes = 'foil'
  AND star.collector_number LIKE '%★'
  AND base.game = star.game
  AND base.set_code = star.set_code
  AND base.oracle_id = star.oracle_id
  AND base.finishes = 'nonfoil'
  AND base.collector_number = substr(star.collector_number, 1, length(star.collector_number) - 1)
  -- Only rewrite a base whose foil price would actually change. Without this, every matched base
  -- is re-written each tick even when the sibling price is unchanged, churning an MVCC tuple +
  -- indexes for nothing. `IS DISTINCT FROM` is null-safe and valid on both Postgres and SQLite
  -- (≥ 3.39).
  AND base.price_usd_foil IS DISTINCT FROM star.price_usd_foil"#;

// ---------- The catalog-listing fold ----------

/// The `promo_types` tokens a folded star is allowed to carry that its base does not:
/// pure **foil finishes**, i.e. how the same printed card face was foiled.
///
/// This is what separates "one card in two finishes" from "two different cards". Against the
/// live catalog, `surgefoil` (40k/t40k), `rainbowfoil` and `galaxyfoil` (Secret Lair) are the
/// only tokens any real pair differs by, and the star only ever *adds* them — no pair has the
/// base carrying a token the star lacks. The rest of the list is their unambiguous cousins,
/// so a future set foiling the same art a new way folds without a code change.
///
/// Deliberately **not** here, though `scryfall::search` treats them as the same promo family:
/// `textured`, `gilded`, `neonink`, `embossed`, `stepandcompleat`, `doublerainbow` (visibly
/// different treatments, arguably their own printings) and `serialized` (a numbered card, not
/// a finish at all). A star differing by one of those keeps its own tile — the conservative
/// direction, and the one that costs a duplicate rather than a missing printing.
const FOIL_TREATMENT_PROMO_TYPES: [&str; 6] = [
    "surgefoil",
    "rainbowfoil",
    "galaxyfoil",
    "halofoil",
    "confettifoil",
    "silverfoil",
];

/// The printed attributes that must be **identical** for a star to fold onto its base.
///
/// The pairing rule the price enrichment and `collection_import::consolidate` share asks only
/// about finishes and numbering, which is right for *copying a price* — a wrong pair there
/// costs nothing — and wrong for *hiding a row*, where a wrong pair costs a printing. Of the
/// 1,626 pairs that rule matches catalog-wide, only ~550 are the same card twice:
///
/// | sets                   | pairs | what differs                    | folded |
/// |------------------------|-------|---------------------------------|--------|
/// | `7ed` `8ed` `9ed` `dkm`| 1 052 | `border_color` (white → black)  | no     |
/// | `frf`, some `unh`      |    14 | `watermark` / `illustration_id` | no     |
/// | 3 of `sld`             |     3 | `border_color` + `full_art`     | no     |
/// | `10e`                  |   125 | nothing                         | yes    |
/// | `40k` `t40k`           |   304 | `promo_types` + `surgefoil`     | yes    |
/// | `sld` (the rest)       |   107 | `promo_types` + `rainbowfoil`   | yes    |
///
/// A 9th-Edition foil is black-bordered where its nonfoil is white: a different card, and one
/// the app's own `border:` / `wm:` / `art:` leaves query directly. Every column named here —
/// border colour, watermark, frame, frame effects, full-art, illustration, security stamp,
/// flavour text and (modulo the star's foil treatments) `promo_types` — is therefore one a user
/// can *see* or *search on*, and folding may not silently drop a value only the star carries.
///
/// `flavor_text` is the least obvious of them and the reason the list is not just "how it looks":
/// a 10th-Edition premium foil prints **flavour text** where its nonfoil base prints reminder
/// text, so the base's column is NULL while the star's carries real text. Folding such a star
/// would hide the only row `ft:` / `has:flavor` can match — the same loss `border:` would take on
/// a 9ed pair.
fn same_printed_card(base: &FoldCandidate, star: &FoldCandidate) -> bool {
    base.border_color == star.border_color
        && base.watermark == star.watermark
        && base.frame == star.frame
        && base.frame_effects == star.frame_effects
        && base.full_art == star.full_art
        && base.illustration_id == star.illustration_id
        && base.security_stamp == star.security_stamp
        && base.flavor_text == star.flavor_text
        && promo_types_match(base.promo_types.as_deref(), star.promo_types.as_deref())
}

/// `promo_types` equal once the star's foil-treatment tokens are set aside — and *only* the
/// star's: a token on the base that the star lacks means the two rows disagree about the card
/// itself, not about how it was foiled, so it blocks the fold.
fn promo_types_match(base: Option<&str>, star: Option<&str>) -> bool {
    fn tokens(v: Option<&str>) -> std::collections::BTreeSet<&str> {
        v.unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect()
    }
    let base = tokens(base);
    let star = tokens(star);
    base.difference(&star).next().is_none()
        && star
            .difference(&base)
            .all(|t| FOIL_TREATMENT_PROMO_TYPES.contains(t))
}

/// The columns [`same_printed_card`] compares, plus the identity a pair is resolved by.
///
/// Thirteen columns of a ~70-column row: the pass must never drag the wide heap. A
/// `FromQueryResult` model rather than a tuple projection — SeaORM's `into_tuple` implements
/// nothing past twelve columns, and a model keeps the projection and the comparison naming the
/// same fields.
#[derive(Debug, FromQueryResult)]
struct FoldCandidate {
    id: i32,
    set_code: String,
    collector_number: String,
    oracle_id: Option<String>,
    border_color: Option<String>,
    watermark: Option<String>,
    frame: Option<String>,
    frame_effects: Option<String>,
    full_art: Option<bool>,
    illustration_id: Option<String>,
    security_stamp: Option<String>,
    promo_types: Option<String>,
    flavor_text: Option<String>,
}

/// The column projection both halves of the pass select, in the order [`FoldCandidate`] reads
/// them.
fn select_fold_columns(query: Select<card::Entity>) -> Selector<SelectModel<FoldCandidate>> {
    query
        .select_only()
        .column(card::Column::Id)
        .column(card::Column::SetCode)
        .column(card::Column::CollectorNumber)
        .column(card::Column::OracleId)
        .column(card::Column::BorderColor)
        .column(card::Column::Watermark)
        .column(card::Column::Frame)
        .column(card::Column::FrameEffects)
        .column(card::Column::FullArt)
        .column(card::Column::IllustrationId)
        .column(card::Column::SecurityStamp)
        .column(card::Column::PromoTypes)
        .column(card::Column::FlavorText)
        .into_model::<FoldCandidate>()
}

/// Recompute `cards.folded_onto_id` for `game`: point each foil-★ variant that is the *same
/// printed card* as its nonfoil base at that base, and clear the pointer on every row that no
/// longer qualifies. Returns `(folded, cleared)` for logging.
///
/// Runs beside [`enrich_foil_variant_prices`] on every sync tick, and once at boot on the
/// no-sync path. Deciding it here rather than in each listing query is what makes the fold
/// affordable *and* correct: the attribute comparison ([`same_printed_card`]) is plain Rust
/// over a bounded set instead of unwritable cross-backend SQL, and the listings are left with
/// `folded_onto_id IS NULL` — a single indexed column test with a real planner estimate.
/// The predicate this replaced was a correlated `EXISTS`, which Postgres De Morgans out of the
/// `NOT (…)` and converts to a *hashed* SubPlan: the hash build sequentially scans the wide
/// `cards` heap, once per statement, on every catalog page **and** its `COUNT(*)` — which
/// `list_cards` pays twice per page. Measured on Postgres 16 over a 108k-row catalog shaped
/// like the real one (40k nonfoil-only, 1,851 stars, ~550 folded), warm cache:
///
/// | statement                    | correlated `EXISTS` | `folded_onto_id IS NULL` |
/// |------------------------------|---------------------|--------------------------|
/// | listing page (`LIMIT 60`)    | 48.2 ms / 7 050 buf | **0.14 ms / 70 buf**     |
/// | pagination `COUNT(*)`        | 1 454 ms / 8 220 buf| **36.7 ms / 3 249 buf**  |
///
/// **Never bumps `updated_at`.** This is a display pointer, not a price: the alert evaluator's
/// change-narrowing treats a bumped `updated_at` as "a price datum moved", so stamping it here
/// would drag every folded card into each narrowed scan for nothing.
pub(crate) async fn refresh_foil_variant_folds(
    db: &DatabaseConnection,
    game: &str,
) -> Result<(u64, u64), IngestError> {
    // 1. The purely-foil `…★` stars — a tiny set (~1,851 of ~106k), reached through the
    //    `(game, finishes)` index (`m..026`) with the trailing-`★` `LIKE` as a residual, the
    //    same entry point `collection_import::consolidate` uses.
    let stars: Vec<FoldCandidate> = select_fold_columns(
        Card::find()
            .filter(card::Column::Game.eq(game))
            .filter(card::Column::Finishes.eq("foil"))
            .filter(card::Column::CollectorNumber.like(format!("%{FOIL_STAR}"))),
    )
    .all(db)
    .await
    .map_err(IngestError::Db)?;

    // 2. Their candidate bases, resolved by `(set_code, collector_number)` — a point seek per
    //    pair through `m..024`, chunked so the bind list stays under SQLite's per-statement cap.
    let mut wanted: Vec<(String, String)> = Vec::with_capacity(stars.len());
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for star in &stars {
        let key = (
            star.set_code.clone(),
            strip_foil_star(&star.collector_number).to_string(),
        );
        if seen.insert(key.clone()) {
            wanted.push(key);
        }
    }
    let mut bases: HashMap<(String, String), FoldCandidate> = HashMap::new();
    for chunk in wanted.chunks(BASE_LOOKUP_CHUNK) {
        let mut any = Condition::any();
        for (set_code, number) in chunk {
            any = any.add(
                Condition::all()
                    .add(card::Column::SetCode.eq(set_code.as_str()))
                    .add(card::Column::CollectorNumber.eq(number.as_str())),
            );
        }
        let rows: Vec<FoldCandidate> = select_fold_columns(
            Card::find()
                .filter(card::Column::Game.eq(game))
                .filter(card::Column::Finishes.eq("nonfoil"))
                .filter(any),
        )
        .all(db)
        .await
        .map_err(IngestError::Db)?;
        for base in rows {
            bases.insert((base.set_code.clone(), base.collector_number.clone()), base);
        }
    }

    // 3. Decide each pair. A star with no base, a base that is itself foilable (it never
    //    matched the `nonfoil`-exact filter above), a mismatched gameplay identity, or any
    //    printed-attribute difference all leave the star listable in its own right.
    let mut fold: Vec<(i32, i32)> = Vec::new();
    for star in &stars {
        let key = (
            star.set_code.clone(),
            strip_foil_star(&star.collector_number).to_string(),
        );
        let Some(base) = bases.get(&key) else {
            continue;
        };
        let (Some(so), Some(bo)) = (star.oracle_id.as_deref(), base.oracle_id.as_deref()) else {
            continue;
        };
        if so != bo || !same_printed_card(base, star) {
            continue;
        }
        fold.push((star.id, base.id));
    }

    // 4. Write. Both statements are guarded so an unchanged tick writes nothing at all.
    let mut folded = 0u64;
    for (star_id, base_id) in &fold {
        let res = Card::update_many()
            .col_expr(card::Column::FoldedOntoId, Expr::value(*base_id))
            .filter(card::Column::Id.eq(*star_id))
            .filter(
                Condition::any()
                    .add(card::Column::FoldedOntoId.is_null())
                    .add(card::Column::FoldedOntoId.ne(*base_id)),
            )
            .exec(db)
            .await
            .map_err(IngestError::Db)?;
        folded += res.rows_affected;
    }
    let keep: Vec<i32> = fold.iter().map(|(star_id, _)| *star_id).collect();
    let mut cleared = 0u64;
    for chunk in stale_clear_chunks(db, game, &keep).await? {
        let res = Card::update_many()
            .col_expr(card::Column::FoldedOntoId, Expr::value(Option::<i32>::None))
            .filter(card::Column::Id.is_in(chunk))
            .exec(db)
            .await
            .map_err(IngestError::Db)?;
        cleared += res.rows_affected;
    }
    Ok((folded, cleared))
}

/// Every currently-folded row in `game` that this pass did **not** re-confirm, chunked for the
/// clearing `IN`. Read first rather than issuing a blanket `folded_onto_id IS NOT NULL AND id
/// NOT IN (…)` update, so the bind list is bounded by the folded set (~550) instead of by
/// whatever the caller's catalog happens to hold.
async fn stale_clear_chunks(
    db: &DatabaseConnection,
    game: &str,
    keep: &[i32],
) -> Result<Vec<Vec<i32>>, IngestError> {
    let keep: HashSet<i32> = keep.iter().copied().collect();
    let currently: Vec<i32> = Card::find()
        .select_only()
        .column(card::Column::Id)
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::FoldedOntoId.is_not_null())
        .into_tuple()
        .all(db)
        .await
        .map_err(IngestError::Db)?;
    Ok(currently
        .into_iter()
        .filter(|id| !keep.contains(id))
        .collect::<Vec<_>>()
        .chunks(BASE_LOOKUP_CHUNK)
        .map(<[i32]>::to_vec)
        .collect())
}

/// Drop one trailing [`FOIL_STAR`], matching `ENRICH_SQL`'s `substr(…, 1, length(…) - 1)`.
/// A caller has already gated on the number ending in a star.
fn strip_foil_star(collector_number: &str) -> &str {
    collector_number
        .strip_suffix(FOIL_STAR)
        .unwrap_or(collector_number)
}

/// The public catalog listings' fold: keep every row **except** a foil-★ variant folded onto
/// its nonfoil base. Applied by `handlers::catalog::catalog_cards`, the one base query every
/// card grid starts from, so a printing shows up once — with both its prices — instead of as
/// a base tile and a near-identical star tile.
///
/// A plain `IS NULL` on an indexed column: no subquery, and a selectivity the planner can
/// actually estimate (~99.5% true), so it stays a residual and cannot perturb the listing's
/// `ORDER BY … LIMIT` plan.
pub(crate) fn not_folded_foil_variant() -> Condition {
    Condition::all().add(card::Column::FoldedOntoId.is_null())
}

/// How many foil-★ variants are folded out of each set's grid, keyed by set code — and the one
/// place a stored `card_count` is reconciled with them.
///
/// `card_sets.card_count` is the provider's own set-object count, stored verbatim at ingest and
/// never derived from `cards`, so on a set where the fold hides rows every surface that publishes
/// it would overstate the grid it links to: the catalog set list, one set's metadata, and the
/// collection/wish-list (and public-mirror) set tiles, whose completion denominator is dressed
/// from the same `card_sets` row. All of them adjust through [`FoldedSetCounts::adjust`], so two
/// reads of the same set can't disagree about how many cards it holds.
#[derive(Debug, Default)]
pub(crate) struct FoldedSetCounts(HashMap<String, i32>);

impl FoldedSetCounts {
    /// `stored` less the rows the fold hides in `set_code`, floored at zero.
    ///
    /// The floor is not theoretical: `card_count` and `cards` are written by different passes, so
    /// a partial or stale set sync can leave a set counted below the rows it actually holds — and
    /// a tile reading "-1 cards" is worse than one reading a stale number.
    pub(crate) fn adjust(&self, set_code: &str, stored: i32) -> i32 {
        stored
            .saturating_sub(self.0.get(set_code).copied().unwrap_or(0))
            .max(0)
    }
}

impl FromIterator<(String, i32)> for FoldedSetCounts {
    fn from_iter<T: IntoIterator<Item = (String, i32)>>(iter: T) -> Self {
        FoldedSetCounts(iter.into_iter().collect())
    }
}

/// Every set's folded-row count for `game`, for a read that publishes many sets at once.
///
/// One grouped scan over the ~550 non-NULL `folded_onto_id` rows through `m..070`'s index — the
/// set list is CDN-cached, so this runs about as often as the sub-type scan beside it. It does
/// not chase the pre-existing ±1 paper-vs-provider skew a tile already tolerates; it only removes
/// the gap this fold opens.
pub(crate) async fn folded_counts_by_set(
    db: &DatabaseConnection,
    game: &str,
) -> Result<FoldedSetCounts, DbErr> {
    folded_counts(db, game, None).await
}

/// One set's folded-row count, for a read that publishes a single set. Same shape as
/// [`folded_counts_by_set`] so both apply through the same [`FoldedSetCounts::adjust`].
pub(crate) async fn folded_counts_in_set(
    db: &DatabaseConnection,
    game: &str,
    set_code: &str,
) -> Result<FoldedSetCounts, DbErr> {
    folded_counts(db, game, Some(set_code)).await
}

async fn folded_counts(
    db: &DatabaseConnection,
    game: &str,
    set_code: Option<&str>,
) -> Result<FoldedSetCounts, DbErr> {
    let mut query = Card::find()
        .select_only()
        .column(card::Column::SetCode)
        .column_as(card::Column::Id.count(), "folded")
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::FoldedOntoId.is_not_null())
        .group_by(card::Column::SetCode);
    if let Some(code) = set_code {
        query = query.filter(card::Column::SetCode.eq(code));
    }
    let rows: Vec<(String, i64)> = query.into_tuple().all(db).await?;
    Ok(rows
        .into_iter()
        .map(|(code, n)| (code, i32::try_from(n).unwrap_or(i32::MAX)))
        .collect())
}

/// This row is a base with a foil-★ variant folded onto it — so it *is* obtainable in foil,
/// and in whatever foil *treatment* the star records, even though its own `finishes` and
/// `promo_types` say otherwise.
///
/// `is:foil` and the foil-treatment `is:` leaves (`is:rainbowfoil`, `is:surgefoil`, …) OR this
/// in, because the row that used to answer them is now folded out of the grid. Rewriting the
/// base's own columns instead is not an option: `finishes = 'nonfoil'`-exactly is the
/// load-bearing half of the pairing rule in all three of its other homes (`ENRICH_SQL`,
/// `collection_import::consolidate`, and the `m..023` migration), so widening it would stop
/// the very enrichment and consolidation that make the fold correct.
///
/// `extra` narrows the folded star the semi-join looks for (the treatment leaves pass their
/// `promo_types` membership test); `None` asks only that a folded star exists. The probe is an
/// equality on `folded_onto_id`, indexed by `m..070` and non-`NULL` on only ~550 rows, so this
/// is a small hash semi-join rather than the per-row correlated subplan it replaced.
pub(crate) fn has_folded_foil_variant(dialect: Dialect, promo_type: Option<&str>) -> SimpleExpr {
    // `IS NOT NULL` is implied by the equality, but spelling it lets Postgres restrict the
    // subplan's build to `m..070`'s ~550 non-NULL index entries instead of reading every row's
    // `folded_onto_id` — the same "give the planner the qual it can index" care the listing
    // predicate no longer needs, now that it is a bare column test.
    const BASE: &str = "EXISTS (SELECT 1 FROM cards AS fv \
                        WHERE fv.folded_onto_id IS NOT NULL AND fv.folded_onto_id = cards.id";
    match promo_type {
        None => Expr::cust(format!("{BASE})")),
        // The same comma-membership test `search::compile::array_member` renders, applied to the
        // folded star's `promo_types` — so `is:rainbowfoil` finds the base whose rainbow foil we
        // folded, exactly as `is:foil` finds the base whose foil we folded.
        Some(token) => cust_vals(
            dialect,
            format!(
                "{BASE} AND (',' || LOWER(COALESCE(fv.promo_types, '')) || ',') LIKE ? ESCAPE '\\')"
            ),
            [format!("%,{},%", escape_like(&token.to_lowercase()))],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::card;
    use crate::test_support::{card_model, migrated_memory_db};
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter};

    #[allow(clippy::too_many_arguments)]
    async fn insert_in_set(
        db: &DatabaseConnection,
        id: i32,
        set_code: &str,
        collector_number: &str,
        finishes: &str,
        oracle_id: &str,
        usd: Option<&str>,
        usd_foil: Option<&str>,
    ) {
        card::Model {
            external_id: format!("ext-{id}"),
            set_code: set_code.into(),
            collector_number: collector_number.into(),
            finishes: Some(finishes.into()),
            oracle_id: Some(oracle_id.into()),
            price_usd: usd.map(str::to_string),
            price_usd_foil: usd_foil.map(str::to_string),
            ..card_model(id)
        }
        .into_active_model()
        .insert(db)
        .await
        .expect("insert card");
    }

    async fn insert(
        db: &DatabaseConnection,
        id: i32,
        collector_number: &str,
        finishes: &str,
        oracle_id: &str,
        usd: Option<&str>,
        usd_foil: Option<&str>,
    ) {
        insert_in_set(
            db,
            id,
            "sld",
            collector_number,
            finishes,
            oracle_id,
            usd,
            usd_foil,
        )
        .await;
    }

    async fn fetch(db: &DatabaseConnection, external_id: &str) -> card::Model {
        card::Entity::find()
            .filter(card::Column::ExternalId.eq(external_id))
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    async fn foil_price(db: &DatabaseConnection, external_id: &str) -> Option<String> {
        fetch(db, external_id).await.price_usd_foil
    }

    #[tokio::test]
    async fn enriches_a_nonfoil_base_from_its_foil_star_sibling() {
        let db = migrated_memory_db().await;
        // The issue's case: base 741 (nonfoil, no foil price) + star 741★ (foil, $29.39).
        insert(&db, 1, "741", "nonfoil", "ora-chaos", Some("26.75"), None).await;
        insert(&db, 2, "741★", "foil", "ora-chaos", None, Some("29.39")).await;
        // An ambiguous base (itself foilable) and its star -> NOT enriched (rule needs a
        // strictly-nonfoil base).
        insert(
            &db,
            3,
            "33",
            "nonfoil,foil",
            "ora-proctor",
            Some("1.00"),
            Some("2.00"),
        )
        .await;
        insert(&db, 4, "33★", "foil", "ora-proctor", None, Some("9.99")).await;
        // A plain card with no star sibling -> untouched.
        insert(&db, 5, "100", "nonfoil", "ora-plain", Some("5.00"), None).await;
        // An alphanumeric collector number -> the star-strip (`substr(..., length - 1)`) must
        // drop only the trailing ★, matching "W3a" not a numeric prefix.
        insert(&db, 6, "W3a", "nonfoil", "ora-alpha", Some("4.00"), None).await;
        insert(&db, 7, "W3a★", "foil", "ora-alpha", None, Some("3.33")).await;

        let n = enrich_foil_variant_prices(&db).await.expect("enrich");

        assert_eq!(n, 2, "only the clean nonfoil bases are enriched");
        assert_eq!(
            foil_price(&db, "ext-1").await.as_deref(),
            Some("29.39"),
            "base gets star foil price"
        );
        assert_eq!(
            foil_price(&db, "ext-2").await.as_deref(),
            Some("29.39"),
            "star unchanged"
        );
        assert_eq!(
            foil_price(&db, "ext-3").await.as_deref(),
            Some("2.00"),
            "ambiguous base kept its own"
        );
        assert_eq!(
            foil_price(&db, "ext-5").await,
            None,
            "no-sibling card untouched"
        );
        assert_eq!(
            foil_price(&db, "ext-6").await.as_deref(),
            Some("3.33"),
            "alphanumeric base gets star foil price"
        );

        // A live foil-price write must bump `updated_at` (the alert evaluator's change-narrowing
        // depends on it) — the seeded rows carry a fixed 2024-01-01 stamp, so an enriched base is
        // now-stamped while an untouched card keeps the old stamp.
        let seeded: sea_orm::prelude::DateTimeUtc = "2024-01-02T00:00:00Z".parse().unwrap();
        assert!(
            fetch(&db, "ext-1").await.updated_at > seeded,
            "enriched base is re-stamped"
        );
        assert!(
            fetch(&db, "ext-5").await.updated_at < seeded,
            "untouched card keeps its stamp"
        );
    }

    #[tokio::test]
    async fn enrichment_refreshes_a_stale_base_price_and_is_idempotent() {
        let db = migrated_memory_db().await;
        insert(&db, 1, "741", "nonfoil", "ora-chaos", Some("26.75"), None).await;
        insert(&db, 2, "741★", "foil", "ora-chaos", None, Some("29.39")).await;

        enrich_foil_variant_prices(&db).await.expect("enrich once");
        assert_eq!(foil_price(&db, "ext-1").await.as_deref(), Some("29.39"));
        // Re-running is a no-op (same value): the guard skips the write, so zero rows are
        // touched; a later price move re-copies the new value.
        let reran = enrich_foil_variant_prices(&db).await.expect("enrich again");
        assert_eq!(reran, 0, "unchanged base is not rewritten");
        assert_eq!(foil_price(&db, "ext-1").await.as_deref(), Some("29.39"));

        let star = card::Entity::find()
            .filter(card::Column::ExternalId.eq("ext-2"))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let mut star = star.into_active_model();
        star.price_usd_foil = sea_orm::Set(Some("31.00".into()));
        star.update(&db).await.expect("bump star price");

        enrich_foil_variant_prices(&db).await.expect("re-enrich");
        assert_eq!(
            foil_price(&db, "ext-1").await.as_deref(),
            Some("31.00"),
            "base tracks the new star price"
        );
    }

    /// A card with the printed attributes the fold rule compares. Any further attribute a
    /// test cares about (a flavour text, say) rides struct-update syntax on top, the way
    /// [`card_model`] intends.
    #[allow(clippy::too_many_arguments)]
    fn printed(
        id: i32,
        set_code: &str,
        collector_number: &str,
        finishes: &str,
        oracle_id: &str,
        border: &str,
        promo_types: Option<&str>,
    ) -> card::Model {
        card::Model {
            external_id: format!("ext-{id}"),
            set_code: set_code.into(),
            collector_number: collector_number.into(),
            finishes: Some(finishes.into()),
            oracle_id: Some(oracle_id.into()),
            border_color: Some(border.into()),
            promo_types: promo_types.map(str::to_string),
            ..card_model(id)
        }
    }

    async fn insert_model(db: &DatabaseConnection, model: card::Model) {
        model
            .into_active_model()
            .insert(db)
            .await
            .expect("insert card");
    }

    /// Insert a card with the printed attributes the fold rule compares.
    #[allow(clippy::too_many_arguments)]
    async fn insert_printed(
        db: &DatabaseConnection,
        id: i32,
        set_code: &str,
        collector_number: &str,
        finishes: &str,
        oracle_id: &str,
        border: &str,
        promo_types: Option<&str>,
    ) {
        insert_model(
            db,
            printed(
                id,
                set_code,
                collector_number,
                finishes,
                oracle_id,
                border,
                promo_types,
            ),
        )
        .await;
    }

    /// External ids of the rows a catalog listing would show.
    async fn listed(db: &DatabaseConnection) -> Vec<String> {
        let mut v: Vec<String> = card::Entity::find()
            .filter(not_folded_foil_variant())
            .all(db)
            .await
            .expect("listing")
            .into_iter()
            .map(|c| c.external_id)
            .collect();
        v.sort();
        v
    }

    /// External ids matching the base-side predicate, optionally narrowed to a treatment.
    async fn foil_available(db: &DatabaseConnection, promo: Option<&str>) -> Vec<String> {
        let mut v: Vec<String> = card::Entity::find()
            .filter(has_folded_foil_variant(Dialect::Sqlite, promo))
            .all(db)
            .await
            .expect("foil-available")
            .into_iter()
            .map(|c| c.external_id)
            .collect();
        v.sort();
        v
    }

    /// The rule's matrix, drawn from the real catalog rather than invented shapes: the six
    /// populations the pairing rule the price enrichment shares would match, and which of them
    /// are actually one card twice.
    ///
    /// The distinction this pins is the whole point of the pass. `9ed`'s foil is *black*
    /// bordered where its nonfoil is white — a different card, and one `border:black` queries
    /// directly — so the broad rule that is right for copying a price is wrong for hiding a row.
    #[tokio::test]
    async fn only_a_star_that_is_the_same_printed_card_folds() {
        let db = migrated_memory_db().await;
        // sld: differs only by the rainbow-foil treatment -> folds.
        insert_printed(
            &db,
            1,
            "sld",
            "1587",
            "nonfoil",
            "ora-shelter",
            "borderless",
            Some("universesbeyond"),
        )
        .await;
        insert_printed(
            &db,
            2,
            "sld",
            "1587★",
            "foil",
            "ora-shelter",
            "borderless",
            Some("universesbeyond,rainbowfoil"),
        )
        .await;
        // 40k: differs only by the surge-foil treatment -> folds.
        insert_printed(
            &db,
            3,
            "40k",
            "1",
            "nonfoil",
            "ora-szarekh",
            "black",
            Some("universesbeyond"),
        )
        .await;
        insert_printed(
            &db,
            4,
            "40k",
            "1★",
            "foil",
            "ora-szarekh",
            "black",
            Some("universesbeyond,surgefoil"),
        )
        .await;
        // 10e: attribute-identical -> folds.
        insert_printed(&db, 5, "10e", "1", "nonfoil", "ora-angel", "black", None).await;
        insert_printed(&db, 6, "10e", "1★", "foil", "ora-angel", "black", None).await;
        // 9ed: the foil is black-bordered, the nonfoil white -> two real printings.
        insert_printed(
            &db,
            7,
            "9ed",
            "188",
            "nonfoil",
            "ora-chariot",
            "white",
            None,
        )
        .await;
        insert_printed(&db, 8, "9ed", "188★", "foil", "ora-chariot", "black", None).await;
        // A star carrying a treatment we deliberately don't fold on (a textured foil).
        insert_printed(&db, 9, "xxx", "5", "nonfoil", "ora-tex", "black", None).await;
        insert_printed(
            &db,
            10,
            "xxx",
            "5★",
            "foil",
            "ora-tex",
            "black",
            Some("textured"),
        )
        .await;
        // A base carrying a token its star lacks -> they disagree about the card, not the foil.
        insert_printed(
            &db,
            11,
            "yyy",
            "6",
            "nonfoil",
            "ora-sn",
            "black",
            Some("serialized"),
        )
        .await;
        insert_printed(
            &db,
            12,
            "yyy",
            "6★",
            "foil",
            "ora-sn",
            "black",
            Some("rainbowfoil"),
        )
        .await;
        // An orphan star, an etched star, an ambiguous (already-foilable) base, a cross-set
        // near-match, and an oracle mismatch: all spared, as before.
        insert_printed(&db, 13, "sld", "796★", "foil", "ora-vault", "black", None).await;
        insert_printed(
            &db,
            14,
            "sld",
            "900",
            "nonfoil",
            "ora-etched",
            "black",
            None,
        )
        .await;
        insert_printed(
            &db,
            15,
            "sld",
            "900★",
            "etched",
            "ora-etched",
            "black",
            None,
        )
        .await;
        insert_printed(
            &db,
            16,
            "stx",
            "33",
            "nonfoil,foil",
            "ora-proctor",
            "black",
            None,
        )
        .await;
        insert_printed(&db, 17, "stx", "33★", "foil", "ora-proctor", "black", None).await;
        insert_printed(&db, 18, "who", "77", "nonfoil", "ora-split", "black", None).await;
        insert_printed(&db, 19, "sld", "77★", "foil", "ora-split", "black", None).await;
        insert_printed(&db, 20, "abc", "500", "nonfoil", "ora-a", "black", None).await;
        insert_printed(&db, 21, "abc", "500★", "foil", "ora-b", "black", None).await;

        let (folded, cleared) = refresh_foil_variant_folds(&db, "mtg").await.expect("fold");
        assert_eq!(
            (folded, cleared),
            (3, 0),
            "sld + 40k + 10e fold, nothing else"
        );

        assert_eq!(
            listed(&db).await,
            [
                "ext-1", "ext-10", "ext-11", "ext-12", "ext-13", "ext-14", "ext-15", "ext-16",
                "ext-17", "ext-18", "ext-19", "ext-20", "ext-21", "ext-3", "ext-5", "ext-7",
                "ext-8", "ext-9"
            ],
            "the three folded stars leave the grid; the 9ed foil and every spared star stay"
        );
        assert_eq!(
            foil_available(&db, None).await,
            ["ext-1", "ext-3", "ext-5"],
            "only the three bases that lost a star answer is:foil on the fold's strength"
        );
        assert_eq!(
            foil_available(&db, Some("rainbowfoil")).await,
            ["ext-1"],
            "and the treatment leaves see through the fold to the star's own promo_types"
        );
        assert_eq!(foil_available(&db, Some("surgefoil")).await, ["ext-3"]);
        assert_eq!(
            foil_available(&db, Some("textured")).await,
            Vec::<String>::new(),
            "a treatment we never fold on is still answered by the star's own row"
        );
    }

    /// A 10th-Edition premium foil prints **flavour text** where its nonfoil base prints
    /// reminder text: attribute-identical in every other column the rule compares, and matched
    /// by the broad pairing rule the price enrichment shares. Folding it would hide the only row
    /// `ft:` / `has:flavor` can match — the same loss `border:` would take on a 9ed pair — so a
    /// flavour text only the star carries blocks the fold, while an equal one doesn't.
    #[tokio::test]
    async fn a_10e_premium_foils_flavor_text_blocks_the_fold() {
        let db = migrated_memory_db().await;
        // The 10e-premium case: the base's `flavor_text` is NULL, the star's is real text.
        insert_printed(&db, 1, "10e", "1", "nonfoil", "ora-angel", "black", None).await;
        insert_model(
            &db,
            card::Model {
                flavor_text: Some("Wings of light, sword of judgment.".into()),
                ..printed(2, "10e", "1★", "foil", "ora-angel", "black", None)
            },
        )
        .await;
        // One number along, both rows print the *same* flavour text: still one card twice.
        for (id, number, finishes) in [(3, "2", "nonfoil"), (4, "2★", "foil")] {
            insert_model(
                &db,
                card::Model {
                    flavor_text: Some("The forest remembers.".into()),
                    ..printed(id, "10e", number, finishes, "ora-elf", "black", None)
                },
            )
            .await;
        }

        assert_eq!(
            refresh_foil_variant_folds(&db, "mtg").await.expect("fold"),
            (1, 0),
            "only the equal-flavour pair folds"
        );
        assert_eq!(
            listed(&db).await,
            ["ext-1", "ext-2", "ext-3"],
            "the star carrying flavour text its base lacks keeps its own tile"
        );
    }

    /// The counts every surface publishing a set's `card_count` adjusts by: per set, scoped to
    /// one set on the same shape, and floored at zero so a stale `card_sets` row can never
    /// publish a negative "N cards".
    #[tokio::test]
    async fn folded_counts_are_per_set_and_never_publish_a_negative() {
        let db = migrated_memory_db().await;
        // sld folds one star; 9ed's black-bordered foil is a printing of its own.
        insert_printed(
            &db,
            1,
            "sld",
            "1587",
            "nonfoil",
            "ora-shelter",
            "black",
            None,
        )
        .await;
        insert_printed(&db, 2, "sld", "1587★", "foil", "ora-shelter", "black", None).await;
        insert_printed(
            &db,
            3,
            "9ed",
            "188",
            "nonfoil",
            "ora-chariot",
            "white",
            None,
        )
        .await;
        insert_printed(&db, 4, "9ed", "188★", "foil", "ora-chariot", "black", None).await;
        refresh_foil_variant_folds(&db, "mtg").await.expect("fold");

        let all = folded_counts_by_set(&db, "mtg").await.expect("counts");
        assert_eq!(all.adjust("sld", 3), 2, "the folded star leaves the grid");
        assert_eq!(all.adjust("9ed", 2), 2, "nothing folded in 9ed");
        assert_eq!(all.adjust("zzz", 7), 7, "a set with no folds is untouched");
        // A `card_sets` row that lags the cards it counts must not publish a negative.
        assert_eq!(all.adjust("sld", 0), 0);

        // The single-set read answers the same numbers, so a set's own page and its tile in
        // the list can't disagree.
        let one = folded_counts_in_set(&db, "mtg", "sld")
            .await
            .expect("scoped counts");
        assert_eq!(one.adjust("sld", 3), 2);
        assert_eq!(
            one.adjust("9ed", 2),
            2,
            "scoping to sld carries no other set's folds"
        );
    }

    /// The pass is idempotent, and it *retracts*: a star that stops qualifying — because the
    /// catalog changed under it — is listed again on the next tick. This is what makes a
    /// derived column safe to trust in a listing.
    #[tokio::test]
    async fn the_pass_is_idempotent_and_clears_a_fold_that_stops_qualifying() {
        let db = migrated_memory_db().await;
        insert_printed(
            &db,
            1,
            "sld",
            "1587",
            "nonfoil",
            "ora-shelter",
            "borderless",
            None,
        )
        .await;
        insert_printed(
            &db,
            2,
            "sld",
            "1587★",
            "foil",
            "ora-shelter",
            "borderless",
            Some("rainbowfoil"),
        )
        .await;

        assert_eq!(
            refresh_foil_variant_folds(&db, "mtg").await.unwrap(),
            (1, 0)
        );
        assert_eq!(listed(&db).await, ["ext-1"]);
        assert_eq!(
            refresh_foil_variant_folds(&db, "mtg").await.unwrap(),
            (0, 0),
            "a settled catalog writes nothing"
        );

        // Upstream re-cuts the star as a full-art printing: no longer the same card.
        let star = card::Entity::find()
            .filter(card::Column::ExternalId.eq("ext-2"))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let mut star = star.into_active_model();
        star.full_art = sea_orm::Set(Some(true));
        star.update(&db).await.expect("re-cut the star");

        assert_eq!(
            refresh_foil_variant_folds(&db, "mtg").await.unwrap(),
            (0, 1),
            "the fold is retracted"
        );
        assert_eq!(
            listed(&db).await,
            ["ext-1", "ext-2"],
            "and the star is listable again"
        );
    }

    /// A base whose row vanishes (a re-import can drop a printing) must not strand its star as
    /// invisible: `folded_onto_id` is FK-less and orphan-tolerant, and the pass clears it.
    #[tokio::test]
    async fn a_fold_whose_base_disappears_is_cleared_not_stranded() {
        let db = migrated_memory_db().await;
        insert_printed(
            &db,
            1,
            "sld",
            "1587",
            "nonfoil",
            "ora-shelter",
            "borderless",
            None,
        )
        .await;
        insert_printed(
            &db,
            2,
            "sld",
            "1587★",
            "foil",
            "ora-shelter",
            "borderless",
            Some("rainbowfoil"),
        )
        .await;
        refresh_foil_variant_folds(&db, "mtg").await.unwrap();

        card::Entity::delete_many()
            .filter(card::Column::ExternalId.eq("ext-1"))
            .exec(&db)
            .await
            .expect("drop the base");

        assert_eq!(
            refresh_foil_variant_folds(&db, "mtg").await.unwrap(),
            (0, 1)
        );
        assert_eq!(
            listed(&db).await,
            ["ext-2"],
            "the orphaned star is listed again"
        );
    }

    #[test]
    fn promo_types_match_allows_only_the_stars_foil_treatments() {
        // Equal, and equal-modulo-a-treatment the star adds.
        assert!(promo_types_match(None, None));
        assert!(promo_types_match(
            Some("universesbeyond"),
            Some("universesbeyond")
        ));
        assert!(promo_types_match(
            Some("universesbeyond"),
            Some("universesbeyond,rainbowfoil")
        ));
        assert!(promo_types_match(None, Some("surgefoil")));
        // A non-treatment token on the star, or any token the base has and the star lacks.
        assert!(!promo_types_match(None, Some("textured")));
        assert!(!promo_types_match(None, Some("serialized")));
        assert!(!promo_types_match(Some("sldbonus"), Some("rainbowfoil")));
        assert!(!promo_types_match(
            Some("sldbonus,rainbowfoil"),
            Some("rainbowfoil")
        ));
    }

    #[test]
    fn strip_foil_star_drops_exactly_one_trailing_star() {
        assert_eq!(strip_foil_star("1587★"), "1587");
        assert_eq!(strip_foil_star("W3a★"), "W3a");
        assert_eq!(strip_foil_star("★"), "");
        assert_eq!(strip_foil_star("1587"), "1587");
    }
}

//! Set sub-type ("card treatment") grouping — the by-treatment view (issue #282).
//!
//! This mirrors the Secret Lair drop grouping (see [`super::drops`]) — a set's cards
//! bucketed into named groups the SPA can browse — but the grouping key is **derived**,
//! not curated. Unlike Scryfall's drop titles (which live only on a gallery page), the
//! treatments we care about — Borderless, Showcase, Extended Art, Full Art — are already
//! in the bulk card data (`border_color` / `frame_effects` / `full_art`), so
//! [`classify`] reads them straight off the ingested [`card::Model`]. Each card lands in
//! exactly one sub-type by priority order; cards with no special treatment fall into the
//! base "Normal" group.
//!
//! A small curated **override** snapshot ([`subtype_overrides.json`]) layers on top for
//! the sub-types the dataset *doesn't* mark — the issue's "if there is no support from
//! the dataset" case, e.g. Scryfall's panoramic "Borderless Scene" cards, which carry no
//! distinguishing field. It's the same `(game, set, collector_number)` join key as the
//! drop snapshot; an override wins over the derived classification. The shipped file is
//! empty (every current sub-type derives cleanly) — it's a ready seam, not dead weight.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use sea_orm::sea_query::{Expr, SimpleExpr};
use sea_orm::{ColumnTrait, Condition, DbErr, EntityTrait, QueryFilter, QuerySelect};
use serde::Deserialize;

use crate::entities::card;
use crate::entities::prelude::Card;

/// One card-treatment sub-type: a stable slug (anchors/links), a display title, and its
/// position in the by-treatment view's section order (Normal first, then treatments).
#[derive(Debug)]
pub struct Subtype {
    pub slug: &'static str,
    pub title: &'static str,
    /// Section order in the grouped view. Distinct per sub-type; `Normal` is 0 so the
    /// base cards head the list, treatments follow.
    pub order: usize,
}

// `static` (not `const`) so each sub-type has a single stable address — [`classify`]
// returns a `&'static Subtype` and callers compare identity by pointer (see
// [`is_special`]).
pub static NORMAL: Subtype = Subtype { slug: "normal", title: "Normal", order: 0 };
static BORDERLESS: Subtype = Subtype { slug: "borderless", title: "Borderless", order: 1 };
static SHOWCASE: Subtype = Subtype { slug: "showcase", title: "Showcase", order: 2 };
static EXTENDED_ART: Subtype = Subtype { slug: "extended-art", title: "Extended Art", order: 3 };
static FULL_ART: Subtype = Subtype { slug: "full-art", title: "Full Art", order: 4 };
/// Curated-only: no card *derives* to this (Scryfall doesn't mark scene cards), but the
/// override snapshot can assign it. See the module docs.
static BORDERLESS_SCENE: Subtype =
    Subtype { slug: "borderless-scene", title: "Borderless Scene", order: 5 };

/// Every sub-type an override may name, resolved by slug when the snapshot is loaded.
static CURATED: &[&Subtype] =
    &[&NORMAL, &BORDERLESS, &SHOWCASE, &EXTENDED_ART, &FULL_ART, &BORDERLESS_SCENE];

/// The sub-type a card belongs to. A curated override (by `(game, set, collector_number)`)
/// wins; otherwise the treatment is derived from the card's print attributes.
pub fn classify(card: &card::Model) -> &'static Subtype {
    override_for(&card.game, &card.set_code, &card.collector_number).unwrap_or_else(|| derive(card))
}

/// Whether a card has any special treatment (i.e. classifies to something other than
/// [`NORMAL`]). Backs the per-set `has_subtypes` flag on the owned-set tiles, where the
/// owned cards are already in hand so no query is needed.
pub fn is_special(card: &card::Model) -> bool {
    !std::ptr::eq(classify(card), &NORMAL)
}

/// Derive a card's treatment from its print attributes, in priority order. Kept in
/// lock-step with [`has_subtype_condition`] (the SQL form of "is non-Normal"): a card is
/// special here iff it matches that predicate, so the by-set flag and the grouped view
/// always agree. Priority resolves the overlaps seen in real data — a borderless
/// *showcase* card is a Showcase, a borderless *full-art* card is Borderless (measured:
/// showcase∩extendedart and borderless∩extendedart are both empty).
fn derive(card: &card::Model) -> &'static Subtype {
    if has_token(card.frame_effects.as_deref(), "showcase") {
        &SHOWCASE
    } else if has_token(card.frame_effects.as_deref(), "extendedart") {
        &EXTENDED_ART
    } else if eq_ci(card.border_color.as_deref(), "borderless") {
        &BORDERLESS
    } else if card.full_art == Some(true) {
        &FULL_ART
    } else {
        &NORMAL
    }
}

/// Whether the comma-joined `field` contains `token` (case-insensitive), matching the SQL
/// membership test in [`has_subtype_condition`]. Comma-split (not substring) so `showcase`
/// can't match inside another token.
fn has_token(field: Option<&str>, token: &str) -> bool {
    field.is_some_and(|f| f.split(',').any(|t| t.trim().eq_ignore_ascii_case(token)))
}

/// Case-insensitive equality against an optional short enum column (`border_color`).
fn eq_ci(field: Option<&str>, value: &str) -> bool {
    field.is_some_and(|f| f.eq_ignore_ascii_case(value))
}

// ---------- Curated overrides ----------

/// The committed override snapshot, embedded at compile time (see the module docs).
const OVERRIDES_JSON: &str = include_str!("subtype_overrides.json");

#[derive(Deserialize)]
struct RawOverrides {
    sets: Vec<RawOverrideSet>,
}

#[derive(Deserialize)]
struct RawOverrideSet {
    game: String,
    set: String,
    entries: Vec<RawOverrideEntry>,
}

#[derive(Deserialize)]
struct RawOverrideEntry {
    /// A slug from [`CURATED`].
    subtype: String,
    collector_numbers: Vec<String>,
}

/// `(game, set_code, collector_number)` -> the curated sub-type for that exact card.
type OverrideTable = HashMap<(String, String, String), &'static Subtype>;

fn overrides() -> &'static OverrideTable {
    static TABLE: OnceLock<OverrideTable> = OnceLock::new();
    TABLE.get_or_init(|| build_overrides(OVERRIDES_JSON))
}

/// Parse the embedded override snapshot into its lookup table. A malformed committed file
/// disables overrides (logged) rather than taking the server down — `overrides_parse`
/// guards the shipped file; an unknown sub-type slug skips just that entry.
fn build_overrides(json: &str) -> OverrideTable {
    let raw: RawOverrides = match serde_json::from_str(json) {
        Ok(raw) => raw,
        Err(err) => {
            tracing::error!(error = %err, "failed to parse subtype_overrides.json; overrides disabled");
            return OverrideTable::new();
        }
    };
    let mut table = OverrideTable::new();
    for set in raw.sets {
        for entry in set.entries {
            let Some(subtype) = by_slug(&entry.subtype) else {
                tracing::error!(subtype = %entry.subtype, "unknown sub-type slug in subtype_overrides.json; skipping");
                continue;
            };
            for cn in entry.collector_numbers {
                table
                    .entry((set.game.clone(), set.set.clone(), cn))
                    .or_insert(subtype);
            }
        }
    }
    table
}

fn by_slug(slug: &str) -> Option<&'static Subtype> {
    CURATED.iter().copied().find(|s| s.slug == slug)
}

/// The curated sub-type for one card, or `None` when it isn't overridden. Short-circuits
/// (no key allocation) in the common case that no overrides are shipped.
fn override_for(game: &str, set_code: &str, collector_number: &str) -> Option<&'static Subtype> {
    let table = overrides();
    if table.is_empty() {
        return None;
    }
    table
        .get(&(game.to_string(), set_code.to_string(), collector_number.to_string()))
        .copied()
}

/// The set codes a game's overrides touch — folded into `has_subtypes` so a set whose only
/// special cards are curated still offers the by-treatment view. Empty for the shipped file.
fn override_set_codes(game: &str) -> HashSet<String> {
    overrides()
        .keys()
        .filter(|(g, ..)| g == game)
        .map(|(_, set, _)| set.clone())
        .collect()
}

// ---------- `has_subtypes` (the by-treatment gate) ----------

/// `frame_effects` comma-membership as a raw, backend-portable SQL fragment (`||` concat +
/// `LIKE` work identically on SQLite and Postgres). `token` is a compile-time constant, so
/// inlining it is injection-safe.
fn frame_effect_is(token: &str) -> SimpleExpr {
    Expr::cust(format!(
        "(',' || LOWER(COALESCE(frame_effects, '')) || ',') LIKE '%,{token},%'"
    ))
}

/// The SQL predicate selecting cards that [`derive`] to a non-Normal sub-type. Kept in
/// lock-step with `derive`: any card matching this is classified special and vice versa,
/// so a set's `has_subtypes` flag agrees with what its grouped view actually shows. Each
/// arm mirrors `derive`'s case-insensitive comparisons (`has_token`/`eq_ci`) via `LOWER`,
/// so the two can't desync if a printing ever stores a non-lowercase border/frame token.
/// Applied only to single-table `cards` queries, so the bare column names are unambiguous.
pub fn has_subtype_condition() -> Condition {
    Condition::any()
        .add(frame_effect_is("showcase"))
        .add(frame_effect_is("extendedart"))
        .add(Expr::cust("LOWER(COALESCE(border_color, '')) = 'borderless'"))
        .add(card::Column::FullArt.eq(true))
}

/// The set codes in a game that have at least one card with a special treatment — the
/// by-treatment gate for the whole set list. One aggregate scan of the game's cards
/// (the set list is CDN-cached, so the origin runs this ~hourly), unioned with any set the
/// curated overrides touch.
pub async fn sets_with_subtypes(
    db: &sea_orm::DatabaseConnection,
    game: &str,
) -> Result<HashSet<String>, DbErr> {
    let mut codes: HashSet<String> = Card::find()
        .select_only()
        .column(card::Column::SetCode)
        .distinct()
        .filter(card::Column::Game.eq(game))
        .filter(has_subtype_condition())
        .into_tuple::<String>()
        .all(db)
        .await?
        .into_iter()
        .collect();
    codes.extend(override_set_codes(game));
    Ok(codes)
}

/// Whether a single set has any special-treatment card — the by-treatment gate for one
/// set (its cards are index-scanned via `(game, set_code)`, so this is cheap).
pub async fn set_has_subtypes(
    db: &sea_orm::DatabaseConnection,
    game: &str,
    set_code: &str,
) -> Result<bool, DbErr> {
    if override_set_codes(game).contains(set_code) {
        return Ok(true);
    }
    let found = Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::SetCode.eq(set_code))
        .filter(has_subtype_condition())
        .select_only()
        .column(card::Column::Id)
        .into_tuple::<i32>()
        .one(db)
        .await?;
    Ok(found.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::card_model;

    /// A card in `mtg`/`tst` (collector number = `id`) with the given treatment fields;
    /// everything else defaulted.
    fn treated(
        id: i32,
        border_color: Option<&str>,
        frame_effects: Option<&str>,
        full_art: Option<bool>,
    ) -> card::Model {
        card::Model {
            border_color: border_color.map(str::to_string),
            frame_effects: frame_effects.map(str::to_string),
            full_art,
            ..card_model(id)
        }
    }

    #[test]
    fn derives_each_treatment() {
        assert_eq!(classify(&treated(1, None, None, None)).slug, "normal");
        assert_eq!(classify(&treated(2, None, Some("showcase"), None)).slug, "showcase");
        assert_eq!(classify(&treated(3, None, Some("extendedart"), None)).slug, "extended-art");
        assert_eq!(classify(&treated(4, Some("borderless"), None, None)).slug, "borderless");
        assert_eq!(classify(&treated(5, None, None, Some(true))).slug, "full-art");
    }

    #[test]
    fn priority_resolves_overlaps() {
        // Showcase wins over a borderless border and full art.
        assert_eq!(
            classify(&treated(1, Some("borderless"), Some("legendary,showcase"), Some(true))).slug,
            "showcase",
        );
        // Borderless wins over full art (most borderless printings are also full-art).
        assert_eq!(classify(&treated(2, Some("borderless"), None, Some(true))).slug, "borderless");
    }

    #[test]
    fn classification_is_case_insensitive() {
        // `derive` folds case (eq_ci / eq_ignore_ascii_case), and `has_subtype_condition`
        // mirrors it with LOWER — so a non-lowercase token can't desync the flag from the
        // grouped view. Guard the classifier half here.
        assert_eq!(classify(&treated(1, Some("Borderless"), None, None)).slug, "borderless");
        assert_eq!(classify(&treated(2, None, Some("Showcase"), None)).slug, "showcase");
    }

    #[test]
    fn token_membership_is_exact() {
        // A leading token, a trailing token, and a middle token all match; a substring does not.
        assert!(has_token(Some("showcase,legendary"), "showcase"));
        assert!(has_token(Some("legendary,showcase"), "showcase"));
        assert!(has_token(Some("legendary,showcase,inverted"), "showcase"));
        assert!(!has_token(Some("showcasefoil"), "showcase"));
        assert!(!has_token(None, "showcase"));
    }

    #[test]
    fn is_special_matches_classify() {
        assert!(!is_special(&treated(1, None, None, None)));
        assert!(is_special(&treated(2, Some("borderless"), None, None)));
    }

    #[test]
    fn overrides_parse() {
        // The shipped snapshot parses (a malformed one would disable overrides silently).
        let table = build_overrides(OVERRIDES_JSON);
        // Nothing is overridden today, so classification is purely derived.
        assert!(table.is_empty());
    }

    #[test]
    fn override_wins_over_derivation() {
        // A synthetic override forces an otherwise-Normal card into a curated sub-type.
        let json = r#"{"sets":[{"game":"mtg","set":"tst","entries":[
            {"subtype":"borderless-scene","collector_numbers":["100","101"]}]}]}"#;
        let table = build_overrides(json);
        assert_eq!(
            table.get(&("mtg".into(), "tst".into(), "100".into())).map(|s| s.slug),
            Some("borderless-scene"),
        );
        // An unknown slug is skipped, not fatal.
        let bad = r#"{"sets":[{"game":"mtg","set":"tst","entries":[
            {"subtype":"not-a-subtype","collector_numbers":["1"]}]}]}"#;
        assert!(build_overrides(bad).is_empty());
    }
}

//! The release calendar (issue #679): `GET /api/games/{game}/releases?from&to` — what is
//! releasing, and what recently did, inside a date window.
//!
//! It is the page behind the release heads-ups. The alert settings sell two subscriptions —
//! "new set" and "Secret Lair drop" — and until this route there was nowhere to *look*: a
//! player wondering what ships next month had no page. Every date already existed in the
//! catalog (`card_sets.released_at`, the drop table + `sld` card dates, `precon_decks` and
//! `products`); this read only assembles them.
//!
//! Three couplings hold it honest:
//!
//! - **One definition of "a release worth listing."** Which sets and drops appear — the
//!   top-level filter, the set-type allow-list, never `sld` per set, the `sl`-prefix upgrade to
//!   a Secret Lair release, drops read off `sld` alone — is [`crate::catalog::releases`], the
//!   seam the alert engine reads too. Nothing here re-decides it, so the calendar can never
//!   list a set the notification skips, or vice versa.
//! - **A set's `Set` payload is the set list's.** The nested set rides the same
//!   [`SetResponse`] `/sets` publishes, with `has_subtypes` and the folded-variant
//!   `card_count` adjustment filled the way [`super::sets::list_sets`] fills them, so a
//!   client that already renders a set tile renders a calendar entry unchanged — and the two
//!   routes can't publish two different counts for one set.
//! - **The same for every visitor.** Public, CDN + `ETag` cached like every catalog read;
//!   nothing per-user rides it (the SPA's "get a heads-up" button is a link to the alert
//!   settings, not a flag in this payload). The only clock use is the *default* window when a
//!   caller passes no bounds; the SPA always sends explicit, month-aligned bounds so its URL
//!   is stable for a whole month.
//!
//! Nesting: a set's preconstructed decks and sealed products are gathered across its catalog
//! **group** — the top-level set plus every child that names it as `parent_set_code` — because
//! that is where they live: an expansion's Commander decks sit in its `…c` child set. The
//! window admits a *set* by its own date; what it ships is nested whole, never re-filtered by
//! the child rows' dates. A Secret Lair drop's products are attributed through the cards the
//! product is known to contain (the `contains` membership rows → the drop table), the reverse
//! of how [`crate::catalog::sld_product_dates`] dates them — never by name, and never by date
//! alone, since a superdrop releases many drops on one day.
//!
//! This is a **fact page**, not a spoiler feed: the catalog holds no preview data, and nothing
//! here (or in the SPA) words one.

use std::collections::{HashMap, HashSet};

use axum::{Json, extract::State};
use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};

use crate::catalog::releases::{self, SLD_SET_CODE, SldDropRelease};
use crate::entities::prelude::{Card, CardSet, PreconDeck, Product, SealedContent};
use crate::entities::sealed_content::Membership;
use crate::entities::{card, card_set, precon_deck, product, sealed_content};
use crate::error::AppError;
use crate::extract::{Path, Query};
use crate::handlers::precons::{PreconDeckResponse, face_cards, precon_response};
use crate::handlers::shared::{ProductResponse, product_response, require_game};
use crate::scryfall::drops;
use crate::state::AppState;

use super::sets::SetResponse;

/// The widest window a caller may ask for, in days (inclusive of both bounds). A year of
/// releases is a few dozen sets, each with its products nested whole, so this bounds the
/// response without ever cutting a season in half; wider is a `422`, never a silent clamp.
pub const MAX_WINDOW_DAYS: i64 = 366;

/// How far ahead the default window looks when a caller passes no `to` (from `from`).
pub const DEFAULT_WINDOW_DAYS: i64 = 90;

/// SQLite caps host parameters per statement (as few as 999 on old builds), so every by-id
/// lookup is chunked well under the bind limit — a superdrop bundle references thousands of
/// cards.
const IN_CHUNK: usize = 900;

/// `?from=&to=`: the inclusive `YYYY-MM-DD` bounds of the window.
#[derive(Debug, Default, Deserialize)]
pub struct ReleaseParams {
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
}

/// The release calendar for a window: the sets releasing in it (each with what it ships)
/// and the Secret Lair drops. Both lists are date-ascending, so a client renders a month view
/// by walking them in step.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "ReleaseCalendar"))]
pub struct ReleaseCalendarResponse {
    /// The window's first day (`YYYY-MM-DD`), inclusive — as resolved, so a defaulted
    /// request learns what it was answered for.
    pub from: String,
    /// The window's last day (`YYYY-MM-DD`), inclusive.
    pub to: String,
    /// Sets releasing inside the window, date then code ascending.
    pub sets: Vec<SetReleaseResponse>,
    /// Secret Lair drops (inside the `sld` set) with cards releasing inside the window, date
    /// then title ascending. Always empty for a game without Secret Lair.
    pub secret_lair_drops: Vec<SecretLairDropReleaseResponse>,
}

/// One set's release: the set as `/sets` publishes it, plus what ships with it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "SetRelease"))]
pub struct SetReleaseResponse {
    pub set: SetResponse,
    /// The set's release date (`YYYY-MM-DD`) — `set.released_at`, made non-null: a dateless
    /// set can't be inside a window.
    pub released_at: String,
    /// Whether the set is a Secret Lair release of its own (an `sl`-coded top-level set such
    /// as The Zeta Set, `slz`) — the same classification the Secret Lair heads-up uses, so a
    /// client labels it as the notification would.
    pub secret_lair: bool,
    /// The preconstructed decks shipping with the set — across its whole catalog group (the
    /// set plus its child sets), name then slug ascending.
    pub precons: Vec<PreconDeckResponse>,
    /// The sealed products shipping with the set — across its whole catalog group, name
    /// ascending.
    pub products: Vec<ProductResponse>,
}

/// One Secret Lair drop's release.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "SecretLairDropRelease"))]
pub struct SecretLairDropReleaseResponse {
    /// The drop's stable slug — what the set page's by-drop view keys on.
    pub slug: String,
    /// The drop's curated title, as the set page's by-drop headers show it.
    pub title: String,
    /// The set the drop is filed under (`sld`), so a client can build the set-page link.
    pub set_code: String,
    /// The drop's street date (`YYYY-MM-DD`): the earliest in-window `released_at` among its
    /// cards.
    pub released_at: String,
    /// The sealed products that *are* this drop (its foil / non-foil editions, a bundle that
    /// includes it), attributed through the cards they contain; empty when the catalog holds
    /// none yet.
    pub products: Vec<ProductResponse>,
}

/// Resolve `?from`/`?to` into an inclusive `(from, to)` window: absent `from` is `today`,
/// absent `to` is `from` + [`DEFAULT_WINDOW_DAYS`]; a bound that isn't a `YYYY-MM-DD` date,
/// a `to` before `from`, or a span past [`MAX_WINDOW_DAYS`] is a `422`. Pure (the clock is
/// injected) so the policy is unit-testable.
pub(crate) fn resolve_window(
    params: &ReleaseParams,
    today: NaiveDate,
) -> Result<(NaiveDate, NaiveDate), AppError> {
    let parse = |name: &str, value: &str| -> Result<NaiveDate, AppError> {
        value.trim().parse::<NaiveDate>().map_err(|_| {
            AppError::Validation(format!("`{name}` must be a date in YYYY-MM-DD form"))
        })
    };
    let from = match params.from.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(value) => parse("from", value)?,
        None => today,
    };
    let to = match params.to.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(value) => parse("to", value)?,
        None => from + Duration::days(DEFAULT_WINDOW_DAYS),
    };
    if to < from {
        return Err(AppError::Validation(
            "`to` must not be earlier than `from`".to_string(),
        ));
    }
    if (to - from).num_days() + 1 > MAX_WINDOW_DAYS {
        return Err(AppError::Validation(format!(
            "the window may span at most {MAX_WINDOW_DAYS} days"
        )));
    }
    Ok((from, to))
}

/// List releases
///
/// `GET /api/games/{game}/releases?from&to` -> the sets releasing inside the window (each
/// with the preconstructed decks and sealed products it ships) and the Secret Lair drops,
/// both date-ascending. The same definition of "a release" the day-before heads-ups use.
#[utoipa::path(
    get,
    path = "/api/games/{game}/releases",
    tag = "Cards",
    params(
        ("game" = String, Path, description = "Game id slug, e.g. `mtg`"),
        ("from" = Option<String>, Query, description = "First day of the window, `YYYY-MM-DD`, inclusive (default: today)"),
        ("to" = Option<String>, Query, description = "Last day of the window, `YYYY-MM-DD`, inclusive (default: `from` + 90 days; at most 366 days after `from`)"),
    ),
    responses(
        (status = 200, description = "The sets and Secret Lair drops releasing inside the window.", body = ReleaseCalendarResponse),
        (status = 404, description = "Unknown game."),
        (status = 422, description = "A malformed date, `to` before `from`, or a window wider than a year."),
    ),
)]
pub async fn list_releases(
    State(state): State<AppState>,
    Path(game): Path<String>,
    Query(params): Query<ReleaseParams>,
) -> Result<Json<ReleaseCalendarResponse>, AppError> {
    require_game(&game)?;
    let (from, to) = resolve_window(&params, Utc::now().date_naive())?;
    let from = from.to_string();
    let to = to.to_string();

    let sets = set_releases(&state, &game, &from, &to).await?;
    let secret_lair_drops = if game == releases::GAME {
        drop_releases(&state, &from, &to).await?
    } else {
        Vec::new()
    };

    Ok(Json(ReleaseCalendarResponse {
        from,
        to,
        sets,
        secret_lair_drops,
    }))
}

/// The window's sets, each dressed as `/sets` dresses it and carrying what it ships.
async fn set_releases(
    state: &AppState,
    game: &str,
    from: &str,
    to: &str,
) -> Result<Vec<SetReleaseResponse>, AppError> {
    let sets: Vec<card_set::Model> = releases::announceable_sets(from, to)
        .filter(card_set::Column::Game.eq(game))
        .all(&state.db)
        .await?;
    if sets.is_empty() {
        return Ok(Vec::new());
    }

    // Each set's catalog group: itself plus the children naming it as parent. One query for
    // every child of every set in the window, then a code -> root map to bucket by.
    let root_codes: Vec<String> = sets.iter().map(|set| set.code.clone()).collect();
    let children: Vec<card_set::Model> = CardSet::find()
        .filter(card_set::Column::Game.eq(game))
        .filter(card_set::Column::ParentSetCode.is_in(root_codes.iter().cloned()))
        .all(&state.db)
        .await?;
    let mut root_of: HashMap<String, String> = root_codes
        .iter()
        .map(|code| (code.clone(), code.clone()))
        .collect();
    let mut names: HashMap<String, String> = sets
        .iter()
        .map(|set| (set.code.clone(), set.name.clone()))
        .collect();
    for child in &children {
        if let Some(parent) = &child.parent_set_code {
            root_of.insert(child.code.clone(), parent.clone());
            names.insert(child.code.clone(), child.name.clone());
        }
    }
    let group_codes: Vec<String> = root_of.keys().cloned().collect();

    // What the groups ship: every precon and product filed under any code in any group.
    let precon_rows: Vec<precon_deck::Model> = PreconDeck::find()
        .filter(precon_deck::Column::Game.eq(game))
        .filter(precon_deck::Column::SetCode.is_in(group_codes.iter().cloned()))
        .order_by_asc(precon_deck::Column::Name)
        .order_by_asc(precon_deck::Column::Slug)
        .all(&state.db)
        .await?;
    let faces = face_cards(state, &precon_rows).await?;
    let mut precons_by_root: HashMap<String, Vec<PreconDeckResponse>> = HashMap::new();
    for row in &precon_rows {
        let Some(root) = root_of.get(&row.set_code) else {
            continue;
        };
        precons_by_root
            .entry(root.clone())
            .or_default()
            .push(precon_response(
                row,
                names.get(&row.set_code).cloned(),
                row.face_card_id.and_then(|id| faces.get(&id).cloned()),
            ));
    }

    let product_rows: Vec<product::Model> = Product::find()
        .filter(product::Column::Game.eq(game))
        .filter(product::Column::SetCode.is_in(group_codes.iter().cloned()))
        .order_by_asc(product::Column::Name)
        .order_by_asc(product::Column::ExternalId)
        .all(&state.db)
        .await?;
    let mut products_by_root: HashMap<String, Vec<ProductResponse>> = HashMap::new();
    for row in product_rows {
        let Some(root) = root_of.get(&row.set_code) else {
            continue;
        };
        products_by_root
            .entry(root.clone())
            .or_default()
            .push(product_response(row, &names));
    }

    // The set payload, dressed exactly as the set list dresses it (see the module docs).
    let with_subtypes = crate::scryfall::subtypes::sets_with_subtypes(&state.db, game).await?;
    let folded = crate::scryfall::folded_counts_by_set(&state.db, game).await?;

    Ok(sets
        .into_iter()
        .filter_map(|model| {
            let released_at = model.released_at.clone()?;
            let secret_lair = releases::is_secret_lair_release(&model);
            let code = model.code.clone();
            let mut set = SetResponse::from(model);
            set.has_subtypes = with_subtypes.contains(&set.code);
            set.card_count = folded.adjust(&set.code, set.card_count);
            Some(SetReleaseResponse {
                set,
                released_at,
                secret_lair,
                precons: precons_by_root.remove(&code).unwrap_or_default(),
                products: products_by_root.remove(&code).unwrap_or_default(),
            })
        })
        .collect())
}

/// The window's Secret Lair drops, date then title ascending, each with the products the
/// catalog attributes to it.
async fn drop_releases(
    state: &AppState,
    from: &str,
    to: &str,
) -> Result<Vec<SecretLairDropReleaseResponse>, AppError> {
    let mut drops = releases::sld_drops_releasing(&state.db, from, to).await?;
    if drops.is_empty() {
        return Ok(Vec::new());
    }
    drops.sort_by(|a, b| {
        a.released_at
            .cmp(&b.released_at)
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.order.cmp(&b.order))
    });
    let mut products = drop_products(state, from, to).await?;
    Ok(drops
        .into_iter()
        .map(|drop: SldDropRelease| SecretLairDropReleaseResponse {
            products: products.remove(&drop.slug).unwrap_or_default(),
            slug: drop.slug,
            title: drop.title,
            set_code: SLD_SET_CODE.to_string(),
            released_at: drop.released_at,
        })
        .collect())
}

/// The `sld` products dated inside the window, attributed to a drop slug through the cards
/// they contain: each product's `contains` rows -> those cards' collector numbers -> the drop
/// table, reduced to the product's **modal** drop ([`modal_drop_by_product`]). A product none
/// of whose contents resolve to a drop is unattributable and left out — the calendar states
/// only what it can place.
async fn drop_products(
    state: &AppState,
    from: &str,
    to: &str,
) -> Result<HashMap<String, Vec<ProductResponse>>, AppError> {
    let Some(table) = drops::table(releases::GAME, SLD_SET_CODE) else {
        return Ok(HashMap::new());
    };
    let products: Vec<product::Model> = Product::find()
        .filter(product::Column::Game.eq(releases::GAME))
        .filter(product::Column::SetCode.eq(SLD_SET_CODE))
        .filter(product::Column::ReleasedAt.gte(from))
        .filter(product::Column::ReleasedAt.lte(to))
        .order_by_asc(product::Column::Name)
        .order_by_asc(product::Column::ExternalId)
        .all(&state.db)
        .await?;
    if products.is_empty() {
        return Ok(HashMap::new());
    }
    let product_ids: Vec<i32> = products.iter().map(|p| p.id).collect();

    let mut contains: Vec<(i32, i32)> = Vec::new();
    for chunk in product_ids.chunks(IN_CHUNK) {
        let rows: Vec<(i32, i32)> = SealedContent::find()
            .select_only()
            .column(sealed_content::Column::ProductId)
            .column(sealed_content::Column::CardId)
            .filter(sealed_content::Column::Game.eq(releases::GAME))
            .filter(sealed_content::Column::Membership.eq(Membership::Contains.as_str()))
            .filter(sealed_content::Column::ProductId.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(&state.db)
            .await?;
        contains.extend(rows);
    }

    let card_ids: Vec<i32> = contains
        .iter()
        .map(|(_, card_id)| *card_id)
        .collect::<HashSet<i32>>()
        .into_iter()
        .collect();
    let mut card_slugs: HashMap<i32, String> = HashMap::new();
    for chunk in card_ids.chunks(IN_CHUNK) {
        let rows: Vec<(i32, String)> = Card::find()
            .select_only()
            .column(card::Column::Id)
            .column(card::Column::CollectorNumber)
            .filter(card::Column::Game.eq(releases::GAME))
            .filter(card::Column::Id.is_in(chunk.iter().copied()))
            .into_tuple()
            .all(&state.db)
            .await?;
        for (id, collector_number) in rows {
            if let Some(drop) = table.drop_for(&collector_number) {
                card_slugs.insert(id, drop.slug.clone());
            }
        }
    }

    let slug_of = modal_drop_by_product(&contains, &card_slugs);
    // A product's set is always `sld`; the one name the DTO needs is its.
    let names: HashMap<String, String> = CardSet::find()
        .filter(card_set::Column::Game.eq(releases::GAME))
        .filter(card_set::Column::Code.eq(SLD_SET_CODE))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|set| (set.code, set.name))
        .collect();

    let mut by_slug: HashMap<String, Vec<ProductResponse>> = HashMap::new();
    for product in products {
        let Some(slug) = slug_of.get(&product.id) else {
            continue;
        };
        by_slug
            .entry(slug.clone())
            .or_default()
            .push(product_response(product, &names));
    }
    Ok(by_slug)
}

/// Reduce each product's contained cards' drop slugs (looked up in `card_slugs`) to the
/// product's modal drop — most cards wins, ties to the lexically first slug so the answer is
/// deterministic. A product whose contents place in no drop is absent.
fn modal_drop_by_product(
    contains: &[(i32, i32)],
    card_slugs: &HashMap<i32, String>,
) -> HashMap<i32, String> {
    let mut tallies: HashMap<i32, HashMap<&str, usize>> = HashMap::new();
    for (product_id, card_id) in contains {
        if let Some(slug) = card_slugs.get(card_id) {
            *tallies
                .entry(*product_id)
                .or_default()
                .entry(slug.as_str())
                .or_insert(0) += 1;
        }
    }
    tallies
        .into_iter()
        .filter_map(|(product_id, by_slug)| {
            by_slug
                .into_iter()
                .max_by(|(slug_a, n_a), (slug_b, n_b)| {
                    n_a.cmp(n_b).then_with(|| slug_b.cmp(slug_a))
                })
                .map(|(slug, _)| (product_id, slug.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn params(from: Option<&str>, to: Option<&str>) -> ReleaseParams {
        ReleaseParams {
            from: from.map(str::to_string),
            to: to.map(str::to_string),
        }
    }

    #[test]
    fn window_defaults_to_today_plus_ninety_days() {
        let today = day("2026-09-08");
        let (from, to) = resolve_window(&ReleaseParams::default(), today).unwrap();
        assert_eq!(from, today);
        assert_eq!(to, day("2026-12-07"));
        // A `from` alone defaults `to` relative to it, not to today.
        let (from, to) = resolve_window(&params(Some("2026-01-01"), None), today).unwrap();
        assert_eq!(from, day("2026-01-01"));
        assert_eq!(to, day("2026-04-01"));
        // Blank values read as absent.
        let (from, _) = resolve_window(&params(Some("  "), Some("")), today).unwrap();
        assert_eq!(from, today);
    }

    #[test]
    fn window_accepts_explicit_bounds_and_trims() {
        let (from, to) = resolve_window(
            &params(Some(" 2026-08-01 "), Some("2026-08-31")),
            day("2026-09-08"),
        )
        .unwrap();
        assert_eq!((from, to), (day("2026-08-01"), day("2026-08-31")));
        // A one-day window is fine.
        let (from, to) = resolve_window(
            &params(Some("2026-08-01"), Some("2026-08-01")),
            day("2026-09-08"),
        )
        .unwrap();
        assert_eq!(from, to);
    }

    #[test]
    fn window_rejects_bad_dates_reversal_and_width() {
        let today = day("2026-09-08");
        for (from, to) in [
            (Some("2026-13-01"), None),
            (Some("yesterday"), None),
            (None, Some("2026/12/01")),
            (Some("2026-09-10"), Some("2026-09-09")),
            // 367 days inclusive: one past the cap.
            (Some("2026-01-01"), Some("2027-01-02")),
        ] {
            assert!(
                matches!(
                    resolve_window(&params(from, to), today),
                    Err(AppError::Validation(_))
                ),
                "{from:?}..{to:?} must be a 422"
            );
        }
        // Exactly the cap (366 days inclusive) is allowed.
        assert!(resolve_window(&params(Some("2026-01-01"), Some("2027-01-01")), today).is_ok());
    }

    #[test]
    fn modal_drop_picks_the_commonest_slug_deterministically() {
        let card_slugs = HashMap::from([
            (1, "a".to_string()),
            (2, "a".to_string()),
            (3, "b".to_string()),
            (4, "b".to_string()),
            (5, "c".to_string()),
        ]);
        let contains = vec![
            // 100: two "a", one "c" -> a.
            (100, 1),
            (100, 2),
            (100, 5),
            // 200: a tie between "a" and "b" -> the lexically first.
            (200, 1),
            (200, 3),
            // 300: contents place in no drop -> absent.
            (300, 99),
        ];
        let modal = modal_drop_by_product(&contains, &card_slugs);
        assert_eq!(modal.get(&100).map(String::as_str), Some("a"));
        assert_eq!(modal.get(&200).map(String::as_str), Some("a"));
        assert!(!modal.contains_key(&300));
    }
}

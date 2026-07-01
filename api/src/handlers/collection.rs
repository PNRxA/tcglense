//! Authenticated, per-user card-collection endpoints.
//!
//! A collection records how many copies of each card a signed-in user owns, per
//! game (`/api/collection/{game}/...`). Every route requires a valid access token
//! (via [`AuthUser`]) and is wired into the router's `private` group, so responses
//! are `Cache-Control: no-store` — per-user data must never be shared-cached.
//!
//! Card ids in the path are the provider's **external** id (the same id the public
//! catalog exposes); each is resolved to the internal `cards.id` before storage,
//! so a holding survives a catalog re-import and the stored `card_id` matches
//! `card_price_history`. Ownership is always scoped by `user.id` from the token, so
//! one user can never read or mutate another's collection.

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
    SqlErr,
};
use serde::{Deserialize, Serialize};

use crate::auth::extractor::AuthUser;
use crate::catalog::{self, Game};
use crate::entities::prelude::{Card, CollectionItem};
use crate::entities::{card, collection_item};
use crate::error::AppError;
use crate::extract::JsonBody;
use crate::handlers::catalog::CardResponse;
use crate::state::AppState;

const DEFAULT_PAGE_SIZE: u64 = 60;
const MAX_PAGE_SIZE: u64 = 200;
/// A generous per-card holding cap: far above any real collection, but bounded so a
/// single count can't overflow the valuation arithmetic or be abused to store a
/// pathological value.
const MAX_QUANTITY: i32 = 1_000_000;

// ---------- Response / request DTOs ----------

/// One owned card: the full public card payload plus how many copies are owned.
#[derive(Debug, Serialize)]
pub struct CollectionEntry {
    pub card: CardResponse,
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// Just the owned counts for one card — what the card-detail controls read and write.
#[derive(Debug, Serialize)]
pub struct CollectionQuantities {
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// Aggregate stats for a user's per-game collection (the collection landing header).
#[derive(Debug, Serialize)]
pub struct CollectionSummary {
    /// Distinct cards owned (one per collection row).
    pub unique_cards: i64,
    /// Total copies owned (regular + foil) across every card.
    pub total_cards: i64,
    /// Estimated USD value: regular copies at the card's `usd`, foil copies at
    /// `usd_foil`, as a 2-dp decimal string. `null` when nothing owned is priced.
    pub total_value_usd: Option<String>,
}

/// A page of results plus the cursor metadata the SPA paginates with (mirrors the
/// catalog's page shape, kept local so the two modules stay decoupled).
#[derive(Debug, Serialize)]
pub struct Page<T> {
    pub data: Vec<T>,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
    pub has_more: bool,
}

/// Body of `PUT .../cards/{id}`: the desired absolute counts (not a delta). Setting
/// both to zero removes the card from the collection.
#[derive(Debug, Deserialize)]
pub struct SetQuantitiesRequest {
    pub quantity: i32,
    pub foil_quantity: i32,
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

impl ListParams {
    /// Resolve the requested 1-based page and clamp the page size to `[1, MAX]`.
    fn page_and_size(&self) -> (u64, u64) {
        let page = self.page.unwrap_or(1).max(1);
        let page_size = self
            .page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE);
        (page, page_size)
    }
}

// ---------- Handlers ----------

/// `GET /api/collection/{game}` -> the signed-in user's owned cards for a game,
/// most-recently-updated first, paginated. Each entry carries the full card payload
/// plus the owned counts.
pub async fn list_collection(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<Json<Page<CollectionEntry>>, AppError> {
    require_game(&game)?;
    let (page, page_size) = params.page_and_size();

    let paginator = CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user.id))
        .filter(collection_item::Column::Game.eq(game.as_str()))
        // Newest change first, with a stable id tiebreaker for deterministic paging.
        .order_by_desc(collection_item::Column::UpdatedAt)
        .order_by_desc(collection_item::Column::Id)
        .paginate(&state.db, page_size);

    let total = paginator.num_items().await?;
    let rows = paginator.fetch_page(page - 1).await?;
    if rows.is_empty() {
        return Ok(Json(build_page(Vec::new(), page, page_size, total)));
    }

    // Load the referenced cards in one query, then assemble entries in row order.
    let card_ids: Vec<i32> = rows.iter().map(|r| r.card_id).collect();
    let mut by_id: HashMap<i32, card::Model> = Card::find()
        .filter(card::Column::Id.is_in(card_ids))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|c| (c.id, c))
        .collect();

    let data: Vec<CollectionEntry> = rows
        .into_iter()
        .filter_map(|r| {
            by_id.remove(&r.card_id).map(|c| CollectionEntry {
                card: CardResponse::from(c),
                quantity: r.quantity,
                foil_quantity: r.foil_quantity,
            })
        })
        .collect();

    Ok(Json(build_page(data, page, page_size, total)))
}

/// `GET /api/collection/{game}/summary` -> aggregate stats (distinct cards, total
/// copies, estimated USD value) for the signed-in user's collection in a game.
pub async fn collection_summary(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(game): Path<String>,
) -> Result<Json<CollectionSummary>, AppError> {
    require_game(&game)?;

    // A collection is bounded by how many distinct cards a user owns, so we load the
    // rows and their cards and total copies + value in Rust (never trusting the
    // stored decimal price strings to SQL arithmetic).
    let rows = CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user.id))
        .filter(collection_item::Column::Game.eq(game.as_str()))
        .all(&state.db)
        .await?;
    if rows.is_empty() {
        return Ok(Json(CollectionSummary {
            unique_cards: 0,
            total_cards: 0,
            total_value_usd: None,
        }));
    }

    let unique_cards = rows.len() as i64;
    let total_cards: i64 = rows
        .iter()
        .map(|r| i64::from(r.quantity) + i64::from(r.foil_quantity))
        .sum();

    let card_ids: Vec<i32> = rows.iter().map(|r| r.card_id).collect();
    let by_id: HashMap<i32, card::Model> = Card::find()
        .filter(card::Column::Id.is_in(card_ids))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|c| (c.id, c))
        .collect();

    let mut total_cents: i128 = 0;
    let mut any_priced = false;
    for r in &rows {
        let Some(card) = by_id.get(&r.card_id) else {
            continue;
        };
        if let Some(cents) = price_cents(card.price_usd.as_deref()) {
            total_cents += cents * i128::from(r.quantity);
            any_priced = true;
        }
        if let Some(cents) = price_cents(card.price_usd_foil.as_deref()) {
            total_cents += cents * i128::from(r.foil_quantity);
            any_priced = true;
        }
    }

    Ok(Json(CollectionSummary {
        unique_cards,
        total_cards,
        total_value_usd: any_priced.then(|| format_cents(total_cents)),
    }))
}

/// `GET /api/collection/{game}/cards/{id}` -> how many copies of one card the user
/// owns (zeros when the card isn't in their collection). `id` is the external card
/// id; a `404` means the game or card is unknown.
pub async fn get_collection_entry(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, id)): Path<(String, String)>,
) -> Result<Json<CollectionQuantities>, AppError> {
    require_game(&game)?;
    let card = load_card(&state, &game, &id).await?;
    let row = find_row(&state, user.id, &game, card.id).await?;
    Ok(Json(match row {
        Some(r) => CollectionQuantities {
            quantity: r.quantity,
            foil_quantity: r.foil_quantity,
        },
        None => CollectionQuantities {
            quantity: 0,
            foil_quantity: 0,
        },
    }))
}

/// `PUT /api/collection/{game}/cards/{id}` -> set the owned counts for one card
/// (absolute values, not a delta). Both zero removes the card from the collection.
/// Returns the resulting counts. `404` for an unknown game/card, `422` for a
/// negative or oversized count.
pub async fn set_collection_entry(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((game, id)): Path<(String, String)>,
    JsonBody(payload): JsonBody<SetQuantitiesRequest>,
) -> Result<Json<CollectionQuantities>, AppError> {
    require_game(&game)?;
    let quantity = validate_quantity(payload.quantity, "quantity")?;
    let foil_quantity = validate_quantity(payload.foil_quantity, "foil_quantity")?;
    let card = load_card(&state, &game, &id).await?;

    let existing = find_row(&state, user.id, &game, card.id).await?;
    let now = Utc::now();

    // Owning zero of both is "not in the collection": drop the row if present.
    if quantity == 0 && foil_quantity == 0 {
        if let Some(row) = existing {
            CollectionItem::delete_by_id(row.id)
                .exec(&state.db)
                .await?;
        }
        return Ok(Json(CollectionQuantities {
            quantity: 0,
            foil_quantity: 0,
        }));
    }

    match existing {
        Some(row) => {
            let mut active: collection_item::ActiveModel = row.into();
            active.quantity = Set(quantity);
            active.foil_quantity = Set(foil_quantity);
            active.updated_at = Set(now);
            active.update(&state.db).await?;
        }
        None => {
            let active = collection_item::ActiveModel {
                user_id: Set(user.id),
                game: Set(game.clone()),
                card_id: Set(card.id),
                quantity: Set(quantity),
                foil_quantity: Set(foil_quantity),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            };
            // The unique (user, game, card) index is the real source of truth: two
            // concurrent first-adds can both see `None`, so a unique violation means
            // we lost the race — fall back to updating the row that won.
            if let Err(err) = active.insert(&state.db).await {
                if matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
                    if let Some(row) = find_row(&state, user.id, &game, card.id).await? {
                        let mut active: collection_item::ActiveModel = row.into();
                        active.quantity = Set(quantity);
                        active.foil_quantity = Set(foil_quantity);
                        active.updated_at = Set(now);
                        active.update(&state.db).await?;
                    }
                } else {
                    return Err(err.into());
                }
            }
        }
    }

    Ok(Json(CollectionQuantities {
        quantity,
        foil_quantity,
    }))
}

// ---------- Helpers ----------

fn require_game(game: &str) -> Result<&'static Game, AppError> {
    catalog::find(game).ok_or_else(|| AppError::NotFound(format!("unknown game '{game}'")))
}

/// Resolve a card by its external (provider) id within a game, 404 if unknown.
async fn load_card(state: &AppState, game: &str, external_id: &str) -> Result<card::Model, AppError> {
    Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::ExternalId.eq(external_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("card '{external_id}' not found")))
}

/// The user's collection row for a card, if any.
async fn find_row(
    state: &AppState,
    user_id: i32,
    game: &str,
    card_id: i32,
) -> Result<Option<collection_item::Model>, AppError> {
    Ok(CollectionItem::find()
        .filter(collection_item::Column::UserId.eq(user_id))
        .filter(collection_item::Column::Game.eq(game))
        .filter(collection_item::Column::CardId.eq(card_id))
        .one(&state.db)
        .await?)
}

fn build_page<T>(data: Vec<T>, page: u64, page_size: u64, total: u64) -> Page<T> {
    Page {
        data,
        page,
        page_size,
        total,
        has_more: page.saturating_mul(page_size) < total,
    }
}

fn validate_quantity(value: i32, field: &str) -> Result<i32, AppError> {
    if value < 0 {
        return Err(AppError::Validation(format!(
            "{field} must not be negative"
        )));
    }
    if value > MAX_QUANTITY {
        return Err(AppError::Validation(format!(
            "{field} must be at most {MAX_QUANTITY}"
        )));
    }
    Ok(value)
}

/// Parse a stored decimal price string (e.g. `"12.34"`) to integer USD cents,
/// rounding to the nearest cent. `None`/empty/unparseable yields `None` so an
/// unpriced card simply doesn't contribute to a valuation.
fn price_cents(price: Option<&str>) -> Option<i128> {
    let value: f64 = price?.trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some((value * 100.0).round() as i128)
}

/// Format integer USD cents as a 2-dp decimal string (e.g. `1234` -> `"12.34"`).
fn format_cents(cents: i128) -> String {
    let dollars = cents / 100;
    let rem = (cents % 100).abs();
    format!("{dollars}.{rem:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_and_size_defaults_and_clamps() {
        let p = ListParams {
            page: None,
            page_size: None,
        };
        assert_eq!(p.page_and_size(), (1, DEFAULT_PAGE_SIZE));

        let p = ListParams {
            page: Some(0),
            page_size: Some(9999),
        };
        assert_eq!(p.page_and_size(), (1, MAX_PAGE_SIZE));

        let p = ListParams {
            page: Some(3),
            page_size: Some(20),
        };
        assert_eq!(p.page_and_size(), (3, 20));
    }

    #[test]
    fn build_page_derives_has_more() {
        let page = build_page(vec![1, 2, 3], 1, 3, 10);
        assert!(page.has_more, "more rows remain after page 1");
        let page = build_page(vec![1], 4, 3, 10);
        assert!(!page.has_more, "page 4 of 10 rows is the last");
        let page = build_page(Vec::<i32>::new(), 1, 60, 0);
        assert!(!page.has_more);
    }

    #[test]
    fn validate_quantity_bounds() {
        assert_eq!(validate_quantity(0, "quantity").unwrap(), 0);
        assert_eq!(validate_quantity(5, "quantity").unwrap(), 5);
        assert!(matches!(
            validate_quantity(-1, "quantity"),
            Err(AppError::Validation(_))
        ));
        assert!(matches!(
            validate_quantity(MAX_QUANTITY + 1, "foil_quantity"),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn price_cents_parses_and_rounds() {
        assert_eq!(price_cents(Some("12.34")), Some(1234));
        assert_eq!(price_cents(Some("0.5")), Some(50));
        assert_eq!(price_cents(Some("  1  ")), Some(100));
        assert_eq!(price_cents(Some("0.005")), Some(1)); // rounds to nearest cent
        assert_eq!(price_cents(Some("")), None);
        assert_eq!(price_cents(Some("n/a")), None);
        assert_eq!(price_cents(None), None);
    }

    #[test]
    fn format_cents_renders_two_decimals() {
        assert_eq!(format_cents(1234), "12.34");
        assert_eq!(format_cents(5), "0.05");
        assert_eq!(format_cents(100), "1.00");
        assert_eq!(format_cents(0), "0.00");
    }
}

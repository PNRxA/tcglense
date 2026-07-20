//! Authenticated, per-user **price alerts** (issue #525).
//!
//! A user sets an alert on a single card or sealed product for a game: a `below`/`above`
//! threshold on that target's live catalog price. The [`crate::alerts`] evaluator re-checks
//! them on an interval and notifies the owner over their configured channels
//! ([`crate::notifications`] — Discord / Telegram — plus optional email). This module is the
//! HTTP surface: CRUD over the alerts (`/api/alerts`), the per-user channel settings
//! (`/api/alerts/channels`), and a "send a test" probe (`/api/alerts/channels/test`).
//!
//! Every route takes [`SessionUser`](crate::auth::extractor::SessionUser) — a real signed-in
//! session, never an API key: the channel settings hold delivery credentials (a Discord
//! webhook URL, a Telegram bot token), so a leaked API key must not be able to read or
//! redirect a user's notifications (the same reasoning that gates API-key *management*). All
//! routes live in the router's `private`, `no-store`, per-user-rate-limited group. Like the
//! collection/wish-list holdings, an alert stores its target by **internal** catalog id
//! (resolved from the provider's external id on create) and an alert that isn't the caller's
//! is a **404**, never a 403 (no existence oracle over alert ids).

use serde::{Deserialize, Serialize};

use sea_orm::prelude::DateTimeUtc;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::entities::prelude::{Card, PriceAlert, Product};
use crate::entities::{card, price_alert, product};
use crate::error::AppError;
use crate::handlers::shared::valuation::{format_cents, price_cents};
use crate::state::AppState;

mod read;
mod write;

pub use read::{get_alert_channels, list_alerts};
pub use write::{
    create_alert, delete_alert, set_alert_channels, test_alert_channels, update_alert,
};

// ---------- Limits ----------

/// Generous per-user alert cap — far above any real user, but bounded so a single account
/// can't create unbounded rows the evaluator must scan each tick.
const MAX_ALERTS_PER_USER: u64 = 500;

/// Upper bound on a threshold: $1,000,000 in cents. Well past any real card/sealed price,
/// but bounded so a wild value can't be stored.
const MAX_THRESHOLD_CENTS: i128 = 100_000_000;

// ---------- Response DTOs ----------

/// The compact target of an alert, for rendering an alert row and its detail link: the
/// provider **external** id (Scryfall UUID for a card, TCGplayer id for a product), display
/// name, set code, a small image, and the target's current price for the watched finish.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct AlertTarget {
    /// `"card"` or `"product"`.
    pub kind: String,
    pub external_id: String,
    pub name: String,
    pub set_code: String,
    pub image_url: Option<String>,
    /// The target's current catalog price for the alert's finish, or null when unpriced /
    /// the target was removed from the catalog by a re-import.
    pub current_price: Option<String>,
}

/// One price alert, dressed with its resolved target.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, rename = "PriceAlert"))]
pub struct AlertResponse {
    pub id: i32,
    pub game: String,
    pub target: AlertTarget,
    /// `"nonfoil"` / `"foil"` / `"etched"` (etched is card-only).
    pub finish: String,
    /// `"below"` or `"above"`.
    pub direction: String,
    /// The USD threshold as a decimal string.
    pub threshold: String,
    pub is_active: bool,
    /// Whether the alert is currently in its notified (crossed) state.
    pub triggered: bool,
    pub last_triggered_at: Option<DateTimeUtc>,
    pub last_price: Option<String>,
    pub created_at: DateTimeUtc,
}

/// The user's notification delivery settings. Returned to the owner (session-only, no-store)
/// so the settings form can prefill; the Discord webhook URL and Telegram bot token are the
/// user's own credentials, never logged (redacted in the entity's `Debug`).
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct AlertChannels {
    pub discord_webhook_url: Option<String>,
    /// Whether Discord delivery is on (independent of whether a URL is saved).
    pub discord_enabled: bool,
    pub telegram_bot_token: Option<String>,
    pub telegram_chat_id: Option<String>,
    /// Whether Telegram delivery is on (independent of whether a token/chat is saved).
    pub telegram_enabled: bool,
    pub email_enabled: bool,
    /// Whether the deployment offers the email channel at all (`ALERTS_EMAIL_ENABLED` set
    /// **and** an email provider configured). When false the SPA hides the email toggle.
    pub email_available: bool,
    /// Whether the user opted into a heads-up the day before a **Secret Lair drop** releases.
    pub sld_release_enabled: bool,
    /// Whether the user opted into a heads-up the day before a **new set** releases.
    pub set_release_enabled: bool,
}

/// One channel's outcome from the "send a test" probe.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct AlertTestResult {
    pub channel: String,
    pub ok: bool,
    pub detail: Option<String>,
}

/// The result of `POST /api/alerts/channels/test`: one entry per configured channel.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct AlertTestResponse {
    pub results: Vec<AlertTestResult>,
}

// ---------- Request DTOs ----------

/// Body of `POST /api/alerts`: create an alert. `external_id` is the provider id of the
/// target (resolved to the internal catalog id for `game`); a mismatched finish for the
/// target kind, an unknown game/target, a bad direction, or a non-positive threshold is a
/// `422`/`404`.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CreateAlertRequest {
    pub game: String,
    pub target_kind: String,
    pub external_id: String,
    pub finish: String,
    pub direction: String,
    pub threshold: String,
}

/// Body of `PUT /api/alerts/{id}`: change any subset of an alert's finish / direction /
/// threshold / active flag (absent field = unchanged). Changing the finish, direction, or
/// threshold re-arms the alert (clears the triggered latch) so the new condition is judged
/// fresh.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct UpdateAlertRequest {
    #[serde(default)]
    pub finish: Option<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub threshold: Option<String>,
    #[serde(default)]
    pub is_active: Option<bool>,
}

/// Body of `PUT /api/alerts/channels`: the desired notification settings. A blank string
/// clears that credential; the form is prefilled from the GET, so a partial edit (e.g.
/// toggling email) resubmits the other fields unchanged. Telegram needs both the bot token
/// and the chat id, or neither.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct SetAlertChannelsRequest {
    #[serde(default)]
    pub discord_webhook_url: Option<String>,
    /// Whether Discord delivers. Defaults to `true` when omitted so an older client that
    /// only sends a URL keeps the channel on (a saved URL delivered before this flag existed).
    #[serde(default = "default_true")]
    pub discord_enabled: bool,
    #[serde(default)]
    pub telegram_bot_token: Option<String>,
    #[serde(default)]
    pub telegram_chat_id: Option<String>,
    /// Whether Telegram delivers. Defaults to `true` when omitted (see `discord_enabled`).
    #[serde(default = "default_true")]
    pub telegram_enabled: bool,
    #[serde(default)]
    pub email_enabled: bool,
    /// Opt into a heads-up the day before a Secret Lair drop releases. Defaults to `false`
    /// (a deliberate subscription — unlike the channel on/off flags, an omitted value is off).
    #[serde(default)]
    pub sld_release_enabled: bool,
    /// Opt into a heads-up the day before a new set releases. Defaults to `false`.
    #[serde(default)]
    pub set_release_enabled: bool,
}

/// serde default for the channel on/off flags: an omitted flag means "on" so an older client
/// that only sends the credential keeps the channel delivering.
fn default_true() -> bool {
    true
}

// ---------- Shared helpers ----------

/// Load an alert by id, proving it belongs to `user_id`. An alert that doesn't exist or
/// belongs to another user is a **404** (never 403 — no existence oracle over alert ids).
pub(crate) async fn load_alert(
    state: &AppState,
    user_id: i32,
    id: i32,
) -> Result<price_alert::Model, AppError> {
    PriceAlert::find_by_id(id)
        .filter(price_alert::Column::UserId.eq(user_id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("alert not found".to_string()))
}

/// Validate the target kind (`"card"` / `"product"`).
pub(crate) fn validate_target_kind(kind: &str) -> Result<&'static str, AppError> {
    match kind {
        "card" => Ok("card"),
        "product" => Ok("product"),
        _ => Err(AppError::Validation(
            "target_kind must be 'card' or 'product'".to_string(),
        )),
    }
}

/// Validate a finish for a target kind: cards allow nonfoil/foil/etched, products
/// nonfoil/foil (TCGCSV is USD-only, no etched).
pub(crate) fn validate_finish(kind: &str, finish: &str) -> Result<&'static str, AppError> {
    match (kind, finish) {
        (_, "nonfoil") => Ok("nonfoil"),
        (_, "foil") => Ok("foil"),
        ("card", "etched") => Ok("etched"),
        ("product", "etched") => Err(AppError::Validation(
            "sealed products have no etched finish".to_string(),
        )),
        _ => Err(AppError::Validation(
            "finish must be 'nonfoil', 'foil', or 'etched'".to_string(),
        )),
    }
}

/// Validate an alert direction (`"below"` / `"above"`).
pub(crate) fn validate_direction(direction: &str) -> Result<&'static str, AppError> {
    match direction {
        "below" => Ok("below"),
        "above" => Ok("above"),
        _ => Err(AppError::Validation(
            "direction must be 'below' or 'above'".to_string(),
        )),
    }
}

/// Parse + normalise a threshold string to a canonical 2-dp decimal (via integer cents).
/// A non-positive, unparseable, or absurdly large value is a `422`.
pub(crate) fn normalize_threshold(threshold: &str) -> Result<String, AppError> {
    let cents = price_cents(Some(threshold))
        .ok_or_else(|| AppError::Validation("threshold must be a number".to_string()))?;
    if cents <= 0 {
        return Err(AppError::Validation(
            "threshold must be greater than zero".to_string(),
        ));
    }
    if cents > MAX_THRESHOLD_CENTS {
        return Err(AppError::Validation("threshold is too large".to_string()));
    }
    Ok(format_cents(cents))
}

/// Turn a set of alert rows into dressed [`AlertResponse`]s, batch-loading their card /
/// product targets once. An alert whose target the catalog no longer has still lists — with
/// a null price and the stored kind — so a user can see and delete a stale alert.
pub(crate) async fn build_alert_responses(
    state: &AppState,
    alerts: Vec<price_alert::Model>,
) -> Result<Vec<AlertResponse>, AppError> {
    use std::collections::HashMap;

    let card_ids: Vec<i32> = alerts.iter().filter_map(|a| a.card_id).collect();
    let product_ids: Vec<i32> = alerts.iter().filter_map(|a| a.product_id).collect();

    let cards: HashMap<i32, card::Model> = if card_ids.is_empty() {
        HashMap::new()
    } else {
        Card::find()
            .filter(card::Column::Id.is_in(card_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|c| (c.id, c))
            .collect()
    };
    let products: HashMap<i32, product::Model> = if product_ids.is_empty() {
        HashMap::new()
    } else {
        Product::find()
            .filter(product::Column::Id.is_in(product_ids))
            .all(&state.db)
            .await?
            .into_iter()
            .map(|p| (p.id, p))
            .collect()
    };

    let responses = alerts
        .into_iter()
        .map(|alert| {
            let target = build_target(&alert, &cards, &products);
            AlertResponse {
                id: alert.id,
                game: alert.game,
                target,
                finish: alert.finish,
                direction: alert.direction,
                threshold: alert.threshold,
                is_active: alert.is_active,
                triggered: alert.triggered,
                last_triggered_at: alert.last_triggered_at,
                last_price: alert.last_price,
                created_at: alert.created_at,
            }
        })
        .collect();
    Ok(responses)
}

/// Build the target block for one alert from the loaded catalog maps.
fn build_target(
    alert: &price_alert::Model,
    cards: &std::collections::HashMap<i32, card::Model>,
    products: &std::collections::HashMap<i32, product::Model>,
) -> AlertTarget {
    match alert.target_kind.as_str() {
        "card" => match alert.card_id.and_then(|id| cards.get(&id)) {
            Some(card) => AlertTarget {
                kind: "card".to_string(),
                external_id: card.external_id.clone(),
                name: card.name.clone(),
                set_code: card.set_code.clone(),
                image_url: card.image_small.clone(),
                current_price: crate::alerts::card_price(card, &alert.finish).map(str::to_string),
            },
            None => orphan_target("card"),
        },
        "product" => match alert.product_id.and_then(|id| products.get(&id)) {
            Some(product) => AlertTarget {
                kind: "product".to_string(),
                external_id: product.external_id.clone(),
                name: product.name.clone(),
                set_code: product.set_code.clone(),
                image_url: product.image_url.clone(),
                current_price: crate::alerts::product_price(product, &alert.finish)
                    .map(str::to_string),
            },
            None => orphan_target("product"),
        },
        other => orphan_target(other),
    }
}

/// A placeholder target for an alert whose catalog row is gone (a re-import), so a stale
/// alert still renders (and can be deleted) instead of vanishing.
fn orphan_target(kind: &str) -> AlertTarget {
    AlertTarget {
        kind: kind.to_string(),
        external_id: String::new(),
        name: "(no longer in catalog)".to_string(),
        set_code: String::new(),
        image_url: None,
        current_price: None,
    }
}

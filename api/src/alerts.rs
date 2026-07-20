//! Price-alert evaluation engine (issue #525).
//!
//! Once per interval ([`crate::tasks`] drives the cadence) the engine re-prices every armed
//! alert against the **live catalog price** — the overwritten `price_usd*` column on the
//! `cards` / `products` row, not the accumulating history tables — and notifies the owner on
//! the rising edge of a below/above threshold crossing.
//!
//! Edge-triggered hysteresis (the `triggered` latch on the alert row) keeps a persistently
//! crossed alert from re-notifying every tick: it fires once when `met && !triggered`, and
//! silently re-arms when the price crosses back (`!met && triggered`). Prices are decimal
//! strings, so all comparisons go through integer USD cents ([`price_cents`]); an unpriced
//! or orphaned target is skipped, never an error.

use std::collections::HashMap;

use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

use crate::email::{self, Emailer};
use crate::entities::prelude::{AlertChannel, Card, PriceAlert, Product, User};
use crate::entities::{alert_channel, card, price_alert, product};
use crate::handlers::shared::valuation::price_cents;
use crate::notifications::{self, AlertNotification, ChannelOutcome};

/// Whether an alert's condition is currently met, given both sides already in integer
/// cents. Pure so the threshold semantics are unit-testable in isolation. An unknown
/// direction is never met (defensive — the value is validated on create).
pub(crate) fn is_met(direction: &str, threshold_cents: i128, price_cents: i128) -> bool {
    match direction {
        "below" => price_cents <= threshold_cents,
        "above" => price_cents >= threshold_cents,
        _ => false,
    }
}

/// The live price string for a card, by the alert's `finish` (`nonfoil` / `foil` /
/// `etched`). Returns the raw stored decimal string (or `None` when that finish is
/// unpriced / the finish is unknown).
pub(crate) fn card_price<'a>(card: &'a card::Model, finish: &str) -> Option<&'a str> {
    match finish {
        "foil" => card.price_usd_foil.as_deref(),
        "etched" => card.price_usd_etched.as_deref(),
        _ => card.price_usd.as_deref(),
    }
}

/// The live price string for a sealed product, by the alert's `finish` (`nonfoil` /
/// `foil`; TCGCSV is USD-only, no etched).
pub(crate) fn product_price<'a>(product: &'a product::Model, finish: &str) -> Option<&'a str> {
    match finish {
        "foil" => product.price_usd_foil.as_deref(),
        _ => product.price_usd.as_deref(),
    }
}

/// A single alert resolved against its (loaded) target: the current price string, plus the
/// target's display name / set / external id for the message and link.
struct Resolved<'a> {
    price: &'a str,
    name: &'a str,
    set_code: &'a str,
    external_id: &'a str,
    /// SPA detail-page path segment kind: `"cards"` (card) or `"sealed"` (product).
    is_card: bool,
}

/// Evaluate every armed alert once and deliver any that fire.
///
/// `email_globally_enabled` is `ALERTS_EMAIL_ENABLED` — even a user who opted in gets no
/// email while the deployment keeps it off. Errors on any single alert / channel are logged
/// and swallowed so one bad row never stalls the batch.
pub async fn evaluate_all(
    db: &DatabaseConnection,
    notify_http: &reqwest::Client,
    emailer: &Emailer,
    public_site_url: &str,
    email_globally_enabled: bool,
) {
    let alerts = match PriceAlert::find()
        .filter(price_alert::Column::IsActive.eq(true))
        .all(db)
        .await
    {
        Ok(alerts) => alerts,
        Err(err) => {
            tracing::warn!(error = %err, "failed to load active price alerts");
            return;
        }
    };
    if alerts.is_empty() {
        return;
    }

    // Batch-load the referenced cards + products once (orphan-tolerant: a target the
    // catalog no longer has is simply absent from these maps and skipped below).
    let card_ids = dedup(alerts.iter().filter_map(|a| a.card_id));
    let product_ids = dedup(alerts.iter().filter_map(|a| a.product_id));
    let cards = load_cards(db, &card_ids).await;
    let products = load_products(db, &product_ids).await;

    let mut fired = 0usize;
    for alert in alerts {
        // Resolve the alert to its live price + display fields via the loaded target.
        let resolved = resolve(&alert, &cards, &products);
        let Some(resolved) = resolved else { continue };

        let (Some(price_cents), Some(threshold_cents)) = (
            price_cents(Some(resolved.price)),
            price_cents(Some(&alert.threshold)),
        ) else {
            // Unpriced target or (defensively) an unparseable threshold: can't decide,
            // leave the latch untouched.
            continue;
        };

        let met = is_met(&alert.direction, threshold_cents, price_cents);
        match (met, alert.triggered) {
            (true, false) => {
                // Rising edge: fire, and latch **only if** delivery reached at least one
                // channel. Latching on a failed/absent delivery would permanently swallow
                // the trigger — most concretely for an alert created before any channel is
                // configured (nothing to deliver to yet). Leaving it un-latched means the
                // next tick retries, so it delivers once the channel works / is added.
                let notification = build_notification(&alert, &resolved, public_site_url);
                let delivered = deliver(
                    db,
                    notify_http,
                    emailer,
                    email_globally_enabled,
                    alert.user_id,
                    &notification,
                )
                .await;
                if delivered {
                    latch(db, alert, resolved.price.to_string()).await;
                    fired += 1;
                }
            }
            (false, true) => {
                // Price crossed back: re-arm silently so a later crossing notifies again.
                rearm(db, alert).await;
            }
            _ => {}
        }
    }

    if fired > 0 {
        tracing::info!(fired, "price alerts fired this evaluation");
    }
}

/// Resolve an alert against the loaded target maps, or `None` when the target is orphaned /
/// the wrong kind.
fn resolve<'a>(
    alert: &price_alert::Model,
    cards: &'a HashMap<i32, card::Model>,
    products: &'a HashMap<i32, product::Model>,
) -> Option<Resolved<'a>> {
    match alert.target_kind.as_str() {
        "card" => {
            let card = cards.get(&alert.card_id?)?;
            Some(Resolved {
                price: card_price(card, &alert.finish)?,
                name: &card.name,
                set_code: &card.set_code,
                external_id: &card.external_id,
                is_card: true,
            })
        }
        "product" => {
            let product = products.get(&alert.product_id?)?;
            Some(Resolved {
                price: product_price(product, &alert.finish)?,
                name: &product.name,
                set_code: &product.set_code,
                external_id: &product.external_id,
                is_card: false,
            })
        }
        _ => None,
    }
}

/// Build the human-readable notification (subject + body + detail link) for a firing alert.
fn build_notification(
    alert: &price_alert::Model,
    resolved: &Resolved<'_>,
    public_site_url: &str,
) -> AlertNotification {
    let arrow = if alert.direction == "below" {
        "📉"
    } else {
        "📈"
    };
    let finish = match alert.finish.as_str() {
        "foil" => " (foil)",
        "etched" => " (etched)",
        _ => "",
    };
    let set = resolved.set_code.to_uppercase();
    let title = format!("Price alert: {}", resolved.name);
    let body = format!(
        "{arrow} {name}{finish} ({set}) is now ${price} — {relation} your ${threshold} threshold.",
        name = resolved.name,
        price = resolved.price,
        relation = if alert.direction == "below" {
            "at or below"
        } else {
            "at or above"
        },
        threshold = alert.threshold,
    );
    let base = public_site_url.trim_end_matches('/');
    let url = if resolved.is_card {
        format!("{base}/cards/{}/cards/{}", alert.game, resolved.external_id)
    } else {
        format!("{base}/sealed/{}/{}", alert.game, resolved.external_id)
    };
    AlertNotification {
        title,
        body,
        url: Some(url),
    }
}

/// Deliver one firing alert over every channel the user configured. Loads the user's
/// [`alert_channel`] settings + email once; a channel that isn't set up is skipped. Failures
/// are logged per-channel, never propagated. Returns `true` iff at least one channel
/// accepted the message — the caller latches the alert only then, so a failed or
/// not-yet-configured delivery is retried next tick instead of silently swallowed.
async fn deliver(
    db: &DatabaseConnection,
    notify_http: &reqwest::Client,
    emailer: &Emailer,
    email_globally_enabled: bool,
    user_id: i32,
    notification: &AlertNotification,
) -> bool {
    let channels = match AlertChannel::find()
        .filter(alert_channel::Column::UserId.eq(user_id))
        .one(db)
        .await
    {
        Ok(channels) => channels,
        Err(err) => {
            tracing::warn!(error = %err, user_id, "failed to load alert channels");
            return false;
        }
    };
    // No settings row = no channels configured yet: nothing delivered, so don't latch.
    let Some(channels) = channels else {
        return false;
    };

    let mut outcomes: Vec<ChannelOutcome> = Vec::new();

    // Each channel delivers only when it's both enabled AND configured.
    if channels.discord_enabled
        && let Some(webhook) = channels.discord_webhook_url.as_deref()
    {
        outcomes.push(notifications::send_discord(notify_http, webhook, notification).await);
    }
    if channels.telegram_enabled
        && let (Some(token), Some(chat)) = (
            channels.telegram_bot_token.as_deref(),
            channels.telegram_chat_id.as_deref(),
        )
    {
        outcomes.push(notifications::send_telegram(notify_http, token, chat, notification).await);
    }
    if channels.email_enabled && email_globally_enabled && emailer.is_enabled() {
        outcomes.push(deliver_email(db, emailer, user_id, notification).await);
    }

    for outcome in &outcomes {
        if !outcome.ok {
            tracing::warn!(
                user_id,
                channel = outcome.channel,
                detail = outcome.detail.as_deref().unwrap_or(""),
                "price-alert delivery failed on a channel"
            );
        }
    }

    outcomes.iter().any(|outcome| outcome.ok)
}

/// Send the email channel for a firing alert (the user's account address).
async fn deliver_email(
    db: &DatabaseConnection,
    emailer: &Emailer,
    user_id: i32,
    notification: &AlertNotification,
) -> ChannelOutcome {
    let email = match User::find_by_id(user_id).one(db).await {
        Ok(Some(user)) => user.email,
        Ok(None) => return ChannelOutcome::fail("email", "user no longer exists"),
        Err(err) => {
            tracing::warn!(error = %err, user_id, "failed to load user for alert email");
            return ChannelOutcome::fail("email", "failed to load user");
        }
    };
    let link = notification.url.as_deref().unwrap_or("");
    let message = email::alert_email(&email, &notification.title, &notification.body, link);
    match emailer.send(message).await {
        Ok(()) => ChannelOutcome::ok("email"),
        Err(err) => {
            tracing::warn!(error = %err, "failed to send alert email");
            ChannelOutcome::fail("email", "failed to send email")
        }
    }
}

/// Latch a fired alert: mark it triggered, stamp the firing time + the price that fired it.
async fn latch(db: &DatabaseConnection, alert: price_alert::Model, price: String) {
    let now = Utc::now();
    let mut active: price_alert::ActiveModel = alert.into();
    active.triggered = Set(true);
    active.last_triggered_at = Set(Some(now));
    active.last_price = Set(Some(price));
    active.updated_at = Set(now);
    if let Err(err) = active.update(db).await {
        tracing::warn!(error = %err, "failed to latch a fired price alert");
    }
}

/// Re-arm an alert whose price crossed back over the threshold (no notification).
async fn rearm(db: &DatabaseConnection, alert: price_alert::Model) {
    let mut active: price_alert::ActiveModel = alert.into();
    active.triggered = Set(false);
    active.updated_at = Set(Utc::now());
    if let Err(err) = active.update(db).await {
        tracing::warn!(error = %err, "failed to re-arm a price alert");
    }
}

/// Sort + dedup a set of ids so the `IN (…)` batch loads carry each id once.
fn dedup(ids: impl Iterator<Item = i32>) -> Vec<i32> {
    let mut unique: Vec<i32> = ids.collect();
    unique.sort_unstable();
    unique.dedup();
    unique
}

/// Batch-load cards by id into an `id -> Model` map (empty ids -> empty map; errors logged).
async fn load_cards(db: &DatabaseConnection, ids: &[i32]) -> HashMap<i32, card::Model> {
    if ids.is_empty() {
        return HashMap::new();
    }
    match Card::find()
        .filter(card::Column::Id.is_in(ids.iter().copied()))
        .all(db)
        .await
    {
        Ok(rows) => rows.into_iter().map(|m| (m.id, m)).collect(),
        Err(err) => {
            tracing::warn!(error = %err, "failed to batch-load alert card targets");
            HashMap::new()
        }
    }
}

/// Batch-load products by id into an `id -> Model` map (empty ids -> empty map; errors logged).
async fn load_products(db: &DatabaseConnection, ids: &[i32]) -> HashMap<i32, product::Model> {
    if ids.is_empty() {
        return HashMap::new();
    }
    match Product::find()
        .filter(product::Column::Id.is_in(ids.iter().copied()))
        .all(db)
        .await
    {
        Ok(rows) => rows.into_iter().map(|m| (m.id, m)).collect(),
        Err(err) => {
            tracing::warn!(error = %err, "failed to batch-load alert product targets");
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_is_met_at_or_under_threshold() {
        assert!(is_met("below", 1000, 999));
        assert!(is_met("below", 1000, 1000)); // inclusive
        assert!(!is_met("below", 1000, 1001));
    }

    #[test]
    fn above_is_met_at_or_over_threshold() {
        assert!(is_met("above", 1000, 1001));
        assert!(is_met("above", 1000, 1000)); // inclusive
        assert!(!is_met("above", 1000, 999));
    }

    #[test]
    fn unknown_direction_never_fires() {
        assert!(!is_met("sideways", 1000, 1000));
    }

    /// The regression guard for the latch-on-delivery fix: a met alert with **no channel**
    /// configured must NOT be latched (else adding a channel later never notifies), and once
    /// a channel (here the capturing emailer) delivers, it latches with the firing price.
    #[tokio::test]
    async fn met_alert_latches_only_once_a_channel_delivers() {
        use crate::email::{Emailer, Mailbox};
        use crate::entities::prelude::{Card, PriceAlert};
        use crate::entities::{alert_channel, card, price_alert};
        use sea_orm::{ActiveModelTrait, EntityTrait, Set};

        let db = crate::test_support::migrated_memory_db().await;
        let user_id = crate::test_support::insert_user(&db, "alerts@example.com").await;
        let card_id = crate::test_support::insert_card(&db, "ext-alert-1").await;
        // Price the card so the "below $50" alert evaluates as met at $8.25.
        let mut priced: card::ActiveModel = Card::find_by_id(card_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .into();
        priced.price_usd = Set(Some("8.25".to_string()));
        priced.update(&db).await.unwrap();

        let now = Utc::now();
        let alert = price_alert::ActiveModel {
            user_id: Set(user_id),
            game: Set(crate::scryfall::GAME.to_string()),
            target_kind: Set("card".to_string()),
            card_id: Set(Some(card_id)),
            finish: Set("nonfoil".to_string()),
            direction: Set("below".to_string()),
            threshold: Set("50.00".to_string()),
            is_active: Set(true),
            triggered: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let http = reqwest::Client::new();

        // No channels: met, but nothing delivered, so it must stay armed (not swallowed).
        evaluate_all(
            &db,
            &http,
            &Emailer::Disabled { log_body: true },
            "https://x.test",
            true,
        )
        .await;
        let after = PriceAlert::find_by_id(alert.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(!after.triggered, "an undeliverable alert must stay armed");
        assert!(after.last_price.is_none());

        // Configure the email channel; the capturing emailer "delivers" with no network.
        alert_channel::ActiveModel {
            user_id: Set(user_id),
            email_enabled: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        let mailbox = Mailbox::default();
        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            "https://x.test",
            true,
        )
        .await;

        let after = PriceAlert::find_by_id(alert.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(after.triggered, "a delivered alert latches");
        assert_eq!(after.last_price.as_deref(), Some("8.25"));
        assert_eq!(mailbox.emails().len(), 1, "the email channel was delivered");

        // Still met + already triggered: no re-notify (hysteresis).
        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            "https://x.test",
            true,
        )
        .await;
        assert_eq!(
            mailbox.emails().len(),
            1,
            "a latched alert does not re-notify"
        );
    }
}

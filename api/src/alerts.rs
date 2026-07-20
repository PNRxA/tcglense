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
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, DbErr, EntityTrait, JoinType,
    QueryFilter, QueryOrder, QuerySelect, RelationTrait, Set,
};

use crate::email::Emailer;
use crate::entities::prelude::{Card, PriceAlert, Product};
use crate::entities::{card, price_alert, product};
use crate::handlers::shared::valuation::price_cents;
use crate::notifications::{self, AlertNotification};

/// How many alerts one evaluation batch loads at a time. The scan is keyset-paginated by id
/// so **memory stays O(batch)** no matter how many total alerts exist — the difference
/// between "works at millions of alerts" and loading every row (and every target) at once.
const EVAL_BATCH: u64 = 2_000;

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

/// Evaluate armed alerts once and deliver any that fire.
///
/// **Scales to millions of alerts** on two axes:
/// - *Memory*: the armed alerts are keyset-paginated by id in [`EVAL_BATCH`]-sized chunks,
///   and each chunk's card/product targets are loaded per-chunk — so nothing loads the whole
///   table (or every target row) into memory at once.
/// - *Work*: `since` narrows the scan to alerts that **could** have changed verdict since the
///   last pass — those whose own row changed (created/edited) **or** whose target's price
///   changed. Because the catalog upsert only bumps `cards`/`products.updated_at` when a datum
///   actually changed (the ingest "changed guard"), an alert whose target price is unchanged
///   *and* whose own row is unchanged cannot have flipped, so it's skipped. `None` = a full
///   pass (first run after start / restart), which then establishes the baseline.
///
/// `email_globally_enabled` is `ALERTS_EMAIL_ENABLED` — even a user who opted in gets no email
/// while the deployment keeps it off. Errors on any single alert / channel are logged and
/// swallowed so one bad row never stalls the batch.
///
/// Returns `true` iff the pass **completed** (scanned every candidate page). Returns `false`
/// if a batch load errored mid-scan — the caller must then NOT advance its `since` cursor, or a
/// verdict flip in an un-scanned higher-id batch would be permanently narrowed out of every
/// future pass.
pub async fn evaluate_all(
    db: &DatabaseConnection,
    notify_http: &reqwest::Client,
    emailer: &Emailer,
    public_site_url: &str,
    email_globally_enabled: bool,
    since: Option<DateTimeUtc>,
) -> bool {
    let mut after: i32 = 0;
    let mut fired = 0usize;
    loop {
        let batch = match load_candidate_batch(db, since, after).await {
            Ok(batch) => batch,
            Err(err) => {
                tracing::warn!(error = %err, "failed to load a price-alert batch; pass incomplete");
                return false;
            }
        };
        let Some(last) = batch.last() else { break };
        after = last.id;
        let batch_len = batch.len();

        // Load only THIS chunk's targets (bounded by the batch size), orphan-tolerant: a
        // target the catalog no longer has is simply absent and its alert is skipped.
        let cards = load_cards(db, &dedup(batch.iter().filter_map(|a| a.card_id))).await;
        let products = load_products(db, &dedup(batch.iter().filter_map(|a| a.product_id))).await;

        for alert in batch {
            fired += evaluate_one(
                db,
                notify_http,
                emailer,
                email_globally_enabled,
                public_site_url,
                alert,
                &cards,
                &products,
            )
            .await;
        }

        // A short page is the last page; skip one extra empty round-trip.
        if (batch_len as u64) < EVAL_BATCH {
            break;
        }
    }

    if fired > 0 {
        tracing::info!(fired, "price alerts fired this evaluation");
    }
    true
}

/// Load the next keyset page of armed candidate alerts (`id > after`, ascending), optionally
/// narrowed by `since`. The narrowing LEFT-JOINs the target so a change to **either** the
/// alert row or its card/product `updated_at` (which the ingest bumps only on a real change)
/// re-includes it; an alert on an unchanged target whose own row is unchanged is filtered out.
async fn load_candidate_batch(
    db: &DatabaseConnection,
    since: Option<DateTimeUtc>,
    after: i32,
) -> Result<Vec<price_alert::Model>, DbErr> {
    let mut query = PriceAlert::find()
        .join(JoinType::LeftJoin, price_alert::Relation::Card.def())
        .join(JoinType::LeftJoin, price_alert::Relation::Product.def())
        .filter(price_alert::Column::IsActive.eq(true))
        .filter(price_alert::Column::Id.gt(after));
    if let Some(since) = since {
        // NULL (the unmatched side of a card-only / product-only alert) fails `>= since`, so
        // it never spuriously includes a row — the alert's own `updated_at` still gates it.
        query = query.filter(
            Condition::any()
                .add(price_alert::Column::UpdatedAt.gte(since))
                .add(card::Column::UpdatedAt.gte(since))
                .add(product::Column::UpdatedAt.gte(since)),
        );
    }
    query
        .order_by_asc(price_alert::Column::Id)
        .limit(EVAL_BATCH)
        .all(db)
        .await
}

/// Evaluate a single alert against its (already-loaded) target and act on the edge:
/// deliver + latch on the rising edge (only if delivery reached a channel), re-arm on the
/// falling edge. Returns `1` if the alert fired (delivered + latched), else `0`.
async fn evaluate_one(
    db: &DatabaseConnection,
    notify_http: &reqwest::Client,
    emailer: &Emailer,
    email_globally_enabled: bool,
    public_site_url: &str,
    alert: price_alert::Model,
    cards: &HashMap<i32, card::Model>,
    products: &HashMap<i32, product::Model>,
) -> usize {
    // Resolve the alert to its live price + display fields via the loaded target.
    let Some(resolved) = resolve(&alert, cards, products) else {
        return 0;
    };
    let (Some(price_cents), Some(threshold_cents)) = (
        price_cents(Some(resolved.price)),
        price_cents(Some(&alert.threshold)),
    ) else {
        // Unpriced target or (defensively) an unparseable threshold: can't decide, leave the
        // latch untouched.
        return 0;
    };

    match (
        is_met(&alert.direction, threshold_cents, price_cents),
        alert.triggered,
    ) {
        (true, false) => {
            // Rising edge: fire, and latch **only if** delivery reached at least one channel.
            // Latching on a failed/absent delivery would permanently swallow the trigger —
            // most concretely for an alert created before any channel is configured. Leaving
            // it un-latched means the next pass retries, so it delivers once a channel works.
            let notification = build_notification(&alert, &resolved, public_site_url);
            let delivered = notifications::deliver_to_user(
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
                return 1;
            }
            // Not delivered (no channel configured yet, or a transient/misconfigured channel):
            // touch `updated_at` so the change-narrowing keeps this still-met alert in the
            // candidate set next pass. Configuring/fixing a channel writes `alert_channels`, not
            // `price_alerts.updated_at`, so without this the retry-next-pass contract would be
            // narrowed away and the alert would only fire once the target price next moves.
            touch(db, alert).await;
            0
        }
        (false, true) => {
            // Price crossed back: re-arm silently so a later crossing notifies again.
            rearm(db, alert).await;
            0
        }
        _ => 0,
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

/// Bump only `updated_at` (no verdict change) on a still-met but **undelivered** alert, so the
/// change-narrowing re-includes it next pass — preserving the "retry delivery next pass"
/// contract (a later channel setup / fix isn't otherwise visible to the narrowing).
async fn touch(db: &DatabaseConnection, alert: price_alert::Model) {
    let mut active: price_alert::ActiveModel = alert.into();
    active.updated_at = Set(Utc::now());
    if let Err(err) = active.update(db).await {
        tracing::warn!(error = %err, "failed to touch an undelivered price alert");
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
            None,
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
            None,
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
            None,
        )
        .await;
        assert_eq!(
            mailbox.emails().len(),
            1,
            "a latched alert does not re-notify"
        );
    }

    /// The change-narrowing (#2) guard: with a `since` after both the alert and its target
    /// last changed, the alert is out of the candidate set and does NOT fire — but a full pass
    /// (`since = None`) does. This is what lets the evaluator skip the bulk of unchanged alerts
    /// each tick instead of re-pricing every one.
    #[tokio::test]
    async fn since_narrows_out_unchanged_alerts() {
        use crate::email::{Emailer, Mailbox};
        use crate::entities::prelude::{Card, PriceAlert};
        use crate::entities::{alert_channel, card, price_alert};
        use sea_orm::{ActiveModelTrait, EntityTrait, Set};

        let db = crate::test_support::migrated_memory_db().await;
        let user_id = crate::test_support::insert_user(&db, "narrow@example.com").await;
        let card_id = crate::test_support::insert_card(&db, "ext-narrow-1").await;

        // Timestamps: the alert + target are stamped in the past; `since` is "now" (after them).
        let past: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
        let mut priced: card::ActiveModel = Card::find_by_id(card_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .into();
        priced.price_usd = Set(Some("8.25".to_string()));
        priced.updated_at = Set(past);
        priced.update(&db).await.unwrap();

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
            created_at: Set(past),
            updated_at: Set(past),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        alert_channel::ActiveModel {
            user_id: Set(user_id),
            email_enabled: Set(true),
            created_at: Set(past),
            updated_at: Set(past),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let http = reqwest::Client::new();
        let mailbox = Mailbox::default();
        let since: DateTimeUtc = "2024-06-01T00:00:00Z".parse().unwrap();

        // Narrowed: nothing changed since June, so the (past-stamped) alert is skipped.
        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            "https://x.test",
            true,
            Some(since),
        )
        .await;
        assert!(
            !PriceAlert::find_by_id(alert.id)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .triggered,
            "an unchanged alert must be narrowed out of the scan"
        );
        assert_eq!(mailbox.emails().len(), 0);

        // Full pass: it evaluates and fires.
        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            "https://x.test",
            true,
            None,
        )
        .await;
        assert!(
            PriceAlert::find_by_id(alert.id)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .triggered,
            "a full pass evaluates the alert"
        );
        assert_eq!(mailbox.emails().len(), 1);
    }

    /// The retry-under-narrowing guard: a met alert that can't deliver yet (no channel) is
    /// *touched* so a later narrowed pass — one whose `since` would otherwise exclude the
    /// stale alert — still re-includes and fires it once a channel is added. Without the touch,
    /// configuring a channel (which writes only `alert_channels`) would be invisible to the
    /// narrowing and the alert would never fire until the target price next moved.
    #[tokio::test]
    async fn undelivered_met_alert_is_retried_under_narrowing() {
        use crate::email::{Emailer, Mailbox};
        use crate::entities::prelude::{Card, PriceAlert};
        use crate::entities::{alert_channel, card, price_alert};
        use sea_orm::{ActiveModelTrait, EntityTrait, Set};

        let db = crate::test_support::migrated_memory_db().await;
        let user_id = crate::test_support::insert_user(&db, "retry@example.com").await;
        let card_id = crate::test_support::insert_card(&db, "ext-retry-1").await;

        let past: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
        let mut priced: card::ActiveModel = Card::find_by_id(card_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .into();
        priced.price_usd = Set(Some("8.25".to_string()));
        priced.updated_at = Set(past);
        priced.update(&db).await.unwrap();

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
            created_at: Set(past),
            updated_at: Set(past),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let http = reqwest::Client::new();
        let mailbox = Mailbox::default();

        // Pass 1: no channel yet. It's met but undeliverable, so it must be TOUCHED (updated_at
        // advanced to ~now), not swallowed. A full pass here (since=None) returns completed=true.
        let completed = evaluate_all(
            &db,
            &http,
            &Emailer::Disabled { log_body: true },
            "https://x.test",
            true,
            None,
        )
        .await;
        assert!(completed, "a clean pass reports completed");
        let touched = PriceAlert::find_by_id(alert.id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(!touched.triggered, "undelivered alert stays armed");
        assert!(
            touched.updated_at > past,
            "undelivered alert is touched to keep it in the window"
        );

        // Configure a channel (writes alert_channels only — never price_alerts.updated_at).
        alert_channel::ActiveModel {
            user_id: Set(user_id),
            email_enabled: Set(true),
            created_at: Set(past),
            updated_at: Set(past),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        // Pass 2, NARROWED with a `since` after the original 2024 stamps but before the touch: the
        // touch is what keeps the alert in this window, so it now delivers + fires.
        let since: DateTimeUtc = "2025-01-01T00:00:00Z".parse().unwrap();
        assert!(
            touched.updated_at > since,
            "the touch is newer than the narrowing cursor"
        );
        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            "https://x.test",
            true,
            Some(since),
        )
        .await;
        assert!(
            PriceAlert::find_by_id(alert.id)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .triggered,
            "the retried alert fires once a channel is configured"
        );
        assert_eq!(mailbox.emails().len(), 1);
    }
}

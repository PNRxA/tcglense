//! Price-alert write endpoints: create / update / delete an alert, save the notification
//! channel settings, and send a test notification. All take [`SessionUser`] (a real
//! session, never an API key — the channel settings hold delivery credentials).

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde::Deserialize;

use crate::auth::extractor::SessionUser;
use crate::entities::prelude::{AlertChannel, PriceAlert};
use crate::entities::{alert_channel, price_alert};
use crate::error::AppError;
use crate::extract::{JsonBody, Path, Query};
use crate::handlers::shared::{load_card, load_product, require_game};
use crate::notifications::{self, AlertNotification};
use crate::state::AppState;

use super::read::email_available;
use super::{
    AlertChannels, AlertResponse, AlertTestResponse, AlertTestResult, CreateAlertRequest,
    MAX_ALERTS_PER_USER, SetAlertChannelsRequest, UpdateAlertRequest, build_alert_responses,
    load_alert, normalize_threshold, validate_direction, validate_finish, validate_target_kind,
};

/// Create alert
///
/// `POST /api/alerts` -> create a price alert on a card or sealed product and return it
/// dressed with its target. `422` for a bad kind/finish/direction/threshold or over the
/// per-user cap; `404` for an unknown game or target.
pub async fn create_alert(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    JsonBody(payload): JsonBody<CreateAlertRequest>,
) -> Result<Json<AlertResponse>, AppError> {
    require_game(&payload.game)?;
    let kind = validate_target_kind(&payload.target_kind)?;
    let finish = validate_finish(kind, &payload.finish)?;
    let direction = validate_direction(&payload.direction)?;
    let threshold = normalize_threshold(&payload.threshold)?;

    // Resolve the provider external id to the internal catalog id for the game (404 if the
    // target is unknown), storing the internal id like the collection/wish-list holdings.
    let (card_id, product_id) = match kind {
        "card" => {
            let card = load_card(&state, &payload.game, &payload.external_id).await?;
            (Some(card.id), None)
        }
        _ => {
            let product = load_product(&state, &payload.game, &payload.external_id).await?;
            (None, Some(product.id))
        }
    };

    let count = PriceAlert::find()
        .filter(price_alert::Column::UserId.eq(user.id))
        .count(&state.db)
        .await?;
    if count >= MAX_ALERTS_PER_USER {
        return Err(AppError::Validation(format!(
            "you can have at most {MAX_ALERTS_PER_USER} alerts"
        )));
    }

    let now = Utc::now();
    let alert = price_alert::ActiveModel {
        user_id: Set(user.id),
        game: Set(payload.game.clone()),
        target_kind: Set(kind.to_string()),
        card_id: Set(card_id),
        product_id: Set(product_id),
        finish: Set(finish.to_string()),
        direction: Set(direction.to_string()),
        threshold: Set(threshold),
        is_active: Set(true),
        triggered: Set(false),
        last_triggered_at: Set(None),
        last_price: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;

    let mut responses = build_alert_responses(&state, vec![alert]).await?;
    Ok(Json(responses.remove(0)))
}

/// Update alert
///
/// `PUT /api/alerts/{id}` -> change any subset of finish / direction / threshold / active
/// flag (absent = unchanged). Changing finish/direction/threshold re-arms the alert. `404`
/// if the alert isn't the caller's; `422` for a bad field.
pub async fn update_alert(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Path(id): Path<i32>,
    JsonBody(payload): JsonBody<UpdateAlertRequest>,
) -> Result<Json<AlertResponse>, AppError> {
    let alert = load_alert(&state, user.id, id).await?;
    let kind = alert.target_kind.clone();

    let mut active: price_alert::ActiveModel = alert.into();
    // Any change to the *condition* re-arms the latch so it's judged fresh next tick.
    let mut condition_changed = false;

    if let Some(finish) = payload.finish.as_deref() {
        let finish = validate_finish(&kind, finish)?;
        active.finish = Set(finish.to_string());
        condition_changed = true;
    }
    if let Some(direction) = payload.direction.as_deref() {
        let direction = validate_direction(direction)?;
        active.direction = Set(direction.to_string());
        condition_changed = true;
    }
    if let Some(threshold) = payload.threshold.as_deref() {
        let threshold = normalize_threshold(threshold)?;
        active.threshold = Set(threshold);
        condition_changed = true;
    }
    if let Some(is_active) = payload.is_active {
        active.is_active = Set(is_active);
    }
    if condition_changed {
        active.triggered = Set(false);
    }
    active.updated_at = Set(Utc::now());
    let updated = active.update(&state.db).await?;

    let mut responses = build_alert_responses(&state, vec![updated]).await?;
    Ok(Json(responses.remove(0)))
}

/// Delete alert
///
/// `DELETE /api/alerts/{id}` -> remove an alert. A `404` if it isn't the caller's.
pub async fn delete_alert(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Path(id): Path<i32>,
) -> Result<StatusCode, AppError> {
    let alert = load_alert(&state, user.id, id).await?;
    PriceAlert::delete_by_id(alert.id).exec(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Trim a submitted credential/id field: blank collapses to `None`.
fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Set alert channels
///
/// `PUT /api/alerts/channels` -> replace the caller's notification settings and return them.
/// The Discord webhook URL is host-allow-listed (`422` if it isn't a discord.com webhook);
/// Telegram needs both the bot token and the chat id, or neither (`422`).
pub async fn set_alert_channels(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    JsonBody(payload): JsonBody<SetAlertChannelsRequest>,
) -> Result<Json<AlertChannels>, AppError> {
    // Discord: blank clears; a non-blank value must be a discord.com webhook (SSRF gate).
    let discord = match clean(payload.discord_webhook_url) {
        Some(url) => {
            Some(notifications::validate_discord_webhook_url(&url).map_err(AppError::Validation)?)
        }
        None => None,
    };

    // Telegram: both or neither.
    let telegram_token = clean(payload.telegram_bot_token);
    let telegram_chat = clean(payload.telegram_chat_id);
    if telegram_token.is_some() != telegram_chat.is_some() {
        return Err(AppError::Validation(
            "Telegram needs both a bot token and a chat id".to_string(),
        ));
    }
    if let Some(token) = telegram_token.as_deref() {
        // The same validity check the sender applies, so a saved token can't silently
        // fail to deliver later.
        notifications::validate_telegram_bot_token(token).map_err(AppError::Validation)?;
    }

    let now = Utc::now();
    let existing = AlertChannel::find()
        .filter(alert_channel::Column::UserId.eq(user.id))
        .one(&state.db)
        .await?;

    match existing {
        Some(row) => {
            let mut active: alert_channel::ActiveModel = row.into();
            active.discord_webhook_url = Set(discord.clone());
            active.discord_enabled = Set(payload.discord_enabled);
            active.telegram_bot_token = Set(telegram_token.clone());
            active.telegram_chat_id = Set(telegram_chat.clone());
            active.telegram_enabled = Set(payload.telegram_enabled);
            active.email_enabled = Set(payload.email_enabled);
            active.sld_release_enabled = Set(payload.sld_release_enabled);
            active.set_release_enabled = Set(payload.set_release_enabled);
            active.updated_at = Set(now);
            active.update(&state.db).await?;
        }
        None => {
            alert_channel::ActiveModel {
                user_id: Set(user.id),
                discord_webhook_url: Set(discord.clone()),
                discord_enabled: Set(payload.discord_enabled),
                telegram_bot_token: Set(telegram_token.clone()),
                telegram_chat_id: Set(telegram_chat.clone()),
                telegram_enabled: Set(payload.telegram_enabled),
                email_enabled: Set(payload.email_enabled),
                sld_release_enabled: Set(payload.sld_release_enabled),
                set_release_enabled: Set(payload.set_release_enabled),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(&state.db)
            .await?;
        }
    }

    Ok(Json(AlertChannels {
        discord_webhook_url: discord,
        discord_enabled: payload.discord_enabled,
        telegram_bot_token: telegram_token,
        telegram_chat_id: telegram_chat,
        telegram_enabled: payload.telegram_enabled,
        email_enabled: payload.email_enabled,
        email_available: email_available(&state),
        sld_release_enabled: payload.sld_release_enabled,
        set_release_enabled: payload.set_release_enabled,
    }))
}

/// Query for `POST /api/alerts/channels/test`: an optional single-channel filter. Omitted =
/// test every configured+enabled channel (the original behaviour); `discord` / `telegram` /
/// `email` = test only that one, so the SPA's per-channel "Test" buttons can verify a single
/// setup without pinging the others.
#[derive(Debug, Deserialize)]
pub struct TestChannelsQuery {
    #[serde(default)]
    pub channel: Option<String>,
}

/// Test alert channels
///
/// `POST /api/alerts/channels/test` -> send a test notification and report the per-channel
/// outcome, so a user can verify their setup. Tests every configured+enabled channel by
/// default; `?channel=discord|telegram|email` narrows it to one (an unknown value is a `422`).
/// Returns an empty result list when the selected channel(s) aren't configured.
pub async fn test_alert_channels(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
    Query(query): Query<TestChannelsQuery>,
) -> Result<Json<AlertTestResponse>, AppError> {
    // Resolve the optional single-channel filter into which channels this call may touch.
    // `None` = all of them (the original behaviour); an unrecognised name is a 422.
    let (do_discord, do_telegram, do_email) = match query.channel.as_deref() {
        None => (true, true, true),
        Some("discord") => (true, false, false),
        Some("telegram") => (false, true, false),
        Some("email") => (false, false, true),
        Some(_) => {
            return Err(AppError::Validation(
                "channel must be 'discord', 'telegram', or 'email'".to_string(),
            ));
        }
    };

    let channels = AlertChannel::find()
        .filter(alert_channel::Column::UserId.eq(user.id))
        .one(&state.db)
        .await?;
    let Some(channels) = channels else {
        return Ok(Json(AlertTestResponse { results: vec![] }));
    };

    let notification = AlertNotification {
        title: "TCGLense test alert".to_string(),
        body: "✅ This is a test price alert from TCGLense. Your notifications are working."
            .to_string(),
        url: Some(format!(
            "{}/alerts",
            state.config.public_site_url.trim_end_matches('/')
        )),
    };

    let mut results: Vec<AlertTestResult> = Vec::new();

    // Only test channels that are in scope AND both configured AND enabled — the same
    // configured+enabled gate delivery uses.
    if do_discord
        && channels.discord_enabled
        && let Some(webhook) = channels.discord_webhook_url.as_deref()
    {
        let outcome = notifications::send_discord(&state.notify_http, webhook, &notification).await;
        results.push(outcome.into());
    }
    if do_telegram
        && channels.telegram_enabled
        && let (Some(token), Some(chat)) = (
            channels.telegram_bot_token.as_deref(),
            channels.telegram_chat_id.as_deref(),
        )
    {
        let outcome =
            notifications::send_telegram(&state.notify_http, token, chat, &notification).await;
        results.push(outcome.into());
    }
    if do_email && channels.email_enabled && email_available(&state) {
        let link = notification.url.as_deref().unwrap_or("");
        let message =
            crate::email::alert_email(&user.email, &notification.title, &notification.body, link);
        let ok = state.email.send(message).await.is_ok();
        results.push(AlertTestResult {
            channel: "email".to_string(),
            ok,
            detail: (!ok).then(|| "failed to send email".to_string()),
        });
    }

    Ok(Json(AlertTestResponse { results }))
}

impl From<crate::notifications::ChannelOutcome> for AlertTestResult {
    fn from(o: crate::notifications::ChannelOutcome) -> Self {
        AlertTestResult {
            channel: o.channel.to_string(),
            ok: o.ok,
            detail: o.detail,
        }
    }
}

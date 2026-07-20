//! Price-alert read endpoints: list the caller's alerts, and read their notification
//! channel settings. Both take [`SessionUser`] (a real session, never an API key).

use axum::{Json, extract::State};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde_json::json;

use crate::auth::extractor::SessionUser;
use crate::entities::prelude::{AlertChannel, PriceAlert};
use crate::entities::{alert_channel, price_alert};
use crate::error::AppError;
use crate::state::AppState;

use super::{AlertChannels, build_alert_responses};

/// Whether this deployment offers the email channel at all: `ALERTS_EMAIL_ENABLED` set and a
/// real email provider configured. Shared by the read + the settings write so they agree.
pub(crate) fn email_available(state: &AppState) -> bool {
    state.config.alerts_email_enabled && state.email.is_enabled()
}

/// List alerts
///
/// `GET /api/alerts` -> the caller's price alerts across all games, most-recently-updated
/// first, each dressed with its resolved target (name, set, image, current price).
pub async fn list_alerts(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let alerts = PriceAlert::find()
        .filter(price_alert::Column::UserId.eq(user.id))
        .order_by_desc(price_alert::Column::UpdatedAt)
        .order_by_desc(price_alert::Column::Id)
        .all(&state.db)
        .await?;
    let responses = build_alert_responses(&state, alerts).await?;
    Ok(Json(json!({ "data": responses })))
}

/// Get alert channels
///
/// `GET /api/alerts/channels` -> the caller's notification delivery settings (or the empty
/// defaults when they've configured none), plus whether the email channel is available on
/// this deployment.
pub async fn get_alert_channels(
    State(state): State<AppState>,
    SessionUser(user): SessionUser,
) -> Result<Json<AlertChannels>, AppError> {
    let row = AlertChannel::find()
        .filter(alert_channel::Column::UserId.eq(user.id))
        .one(&state.db)
        .await?;
    let available = email_available(&state);
    let channels = match row {
        Some(row) => AlertChannels {
            discord_webhook_url: row.discord_webhook_url,
            discord_enabled: row.discord_enabled,
            telegram_bot_token: row.telegram_bot_token,
            telegram_chat_id: row.telegram_chat_id,
            telegram_enabled: row.telegram_enabled,
            email_enabled: row.email_enabled,
            email_available: available,
            sld_release_enabled: row.sld_release_enabled,
            set_release_enabled: row.set_release_enabled,
        },
        // No row yet: the free channels default to on, so filling a field just works. The
        // release subscriptions default off — a deliberate opt-in.
        None => AlertChannels {
            discord_webhook_url: None,
            discord_enabled: true,
            telegram_bot_token: None,
            telegram_chat_id: None,
            telegram_enabled: true,
            email_enabled: false,
            email_available: available,
            sld_release_enabled: false,
            set_release_enabled: false,
        },
    };
    Ok(Json(channels))
}

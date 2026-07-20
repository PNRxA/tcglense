use sea_orm::entity::prelude::*;

/// SeaORM entity for the `alert_channels` table (issue #525): a user's price-alert
/// **notification delivery settings**, one row per user (a unique `user_id`).
///
/// The two free channels are fully self-service and need no operator/global config: a
/// Discord **incoming webhook URL**, and a Telegram **bot token + chat id** (the user
/// makes a bot with @BotFather and reads their chat id). Email is opt-in per user
/// (`email_enabled`) and only ever delivered when the deployment also turns it on
/// (`ALERTS_EMAIL_ENABLED`) and has a real email provider configured.
///
/// The webhook URL and bot token are **credentials** — they're redacted in the manual
/// `Debug` below so a `{:?}` / log line can never leak them (mirroring how `Config` and
/// `Emailer` redact their keys). They must stay reversible (unlike a password), so they
/// are stored as-is rather than hashed. The row is created on first save (upsert per user).
#[derive(Clone, DeriveEntityModel)]
#[sea_orm(table_name = "alert_channels")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning user (`users.id`, unique). Deleting the user cascades the row away.
    pub user_id: i32,
    /// Discord incoming-webhook URL (`https://discord.com/api/webhooks/…`), or null.
    /// Host-allow-listed on save and again before every send (SSRF defence).
    pub discord_webhook_url: Option<String>,
    /// Telegram bot token (from @BotFather), or null. Paired with `telegram_chat_id`.
    pub telegram_bot_token: Option<String>,
    /// Telegram chat id to deliver to, or null. Paired with `telegram_bot_token`.
    pub telegram_chat_id: Option<String>,
    /// Whether the user opted into email alerts (delivered to their account email).
    /// Effective only when the deployment enables `ALERTS_EMAIL_ENABLED` too.
    pub email_enabled: bool,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

// Manual `Debug` so the Discord webhook URL and Telegram bot token — both credentials —
// can never leak via `{:?}` / a tracing field, matching the redaction `Config` and
// `Emailer` apply to their own secrets.
impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("alert_channel::Model")
            .field("id", &self.id)
            .field("user_id", &self.user_id)
            .field(
                "discord_webhook_url",
                &self.discord_webhook_url.as_ref().map(|_| "[redacted]"),
            )
            .field(
                "telegram_bot_token",
                &self.telegram_bot_token.as_ref().map(|_| "[redacted]"),
            )
            // The chat id is a destination, not a secret, but keep it terse.
            .field("telegram_chat_id", &self.telegram_chat_id)
            .field("email_enabled", &self.email_enabled)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

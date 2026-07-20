//! Outbound price-alert notifications over the two free, self-service channels:
//! Discord (an incoming-webhook URL) and Telegram (a bot token + chat id). Email is
//! delivered separately through the shared [`crate::email::Emailer`]; this module owns
//! only the HTTP channels.
//!
//! **SSRF is the headline risk here**: the Discord webhook URL is user-supplied, so it is
//! host-allow-listed both when the user saves it ([`validate_discord_webhook_url`]) and
//! again right before every send ([`send_discord`]) — defence in depth. The dispatch rides
//! a dedicated client ([`crate::state::AppState::notify_http`]) built with redirects
//! **disabled** and a whole-request timeout, so a validated URL can neither bounce to an
//! internal host nor hang the caller. Telegram never touches a user URL — the endpoint is
//! the fixed `api.telegram.org`, with only the bot token/chat id interpolated.
//!
//! Every send returns a [`ChannelOutcome`] instead of failing the batch: a broken channel
//! is logged and reported, never fatal to the others or to the evaluator.

use serde_json::json;
use url::{Host, Url};

/// One rendered alert, reused across channels. `body` is plain text (Discord + Telegram
/// both accept it as-is); `title` is the email subject / a bolded first line.
#[derive(Debug, Clone)]
pub struct AlertNotification {
    pub title: String,
    pub body: String,
    /// Absolute link to the card/product detail page, appended to the message.
    pub url: Option<String>,
}

impl AlertNotification {
    /// The plain-text message sent to Discord / Telegram: the body followed by the link.
    fn text(&self) -> String {
        match &self.url {
            Some(url) => format!("{}\n{url}", self.body),
            None => self.body.clone(),
        }
    }
}

/// The result of attempting one channel, so a batch can report per-channel success without
/// aborting on the first failure. `detail` carries a short reason on failure (logged +
/// surfaced to the "send test" endpoint), never a secret.
#[derive(Debug, Clone)]
pub struct ChannelOutcome {
    pub channel: &'static str,
    pub ok: bool,
    pub detail: Option<String>,
}

impl ChannelOutcome {
    pub(crate) fn ok(channel: &'static str) -> Self {
        Self {
            channel,
            ok: true,
            detail: None,
        }
    }

    pub(crate) fn fail(channel: &'static str, detail: impl Into<String>) -> Self {
        Self {
            channel,
            ok: false,
            detail: Some(detail.into()),
        }
    }
}

/// Discord webhook hosts we permit. Discord serves incoming webhooks from the main site and
/// its PTB/Canary variants (plus the legacy `discordapp.com`); nothing else is a webhook.
const DISCORD_WEBHOOK_HOSTS: &[&str] = &[
    "discord.com",
    "discordapp.com",
    "ptb.discord.com",
    "canary.discord.com",
];

/// Validate a user-supplied Discord webhook URL and return its canonical form.
///
/// Enforced: `https`, a host on [`DISCORD_WEBHOOK_HOSTS`], a `/api/webhooks/…` path, and no
/// credentials. This is the SSRF gate — a URL that passes here can only reach Discord, so
/// following it (even the dedicated no-redirect client aside) cannot hit an internal
/// service. Returns `Err(message)` a handler turns into a `422`.
pub fn validate_discord_webhook_url(raw: &str) -> Result<String, String> {
    let value = raw.trim();
    let url =
        Url::parse(value).map_err(|_| "Discord webhook URL is not a valid URL".to_string())?;
    if url.scheme() != "https" {
        return Err("Discord webhook URL must use https".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Discord webhook URL must not contain credentials".to_string());
    }
    let host_ok = match url.host() {
        Some(Host::Domain(domain)) => DISCORD_WEBHOOK_HOSTS
            .iter()
            .any(|allowed| domain.eq_ignore_ascii_case(allowed)),
        _ => false,
    };
    if !host_ok {
        return Err("Discord webhook URL must be a discord.com webhook".to_string());
    }
    if !url.path().starts_with("/api/webhooks/") {
        return Err("Discord webhook URL must be a /api/webhooks/ URL".to_string());
    }
    // Discord serves webhooks only on the default HTTPS port; an explicit port is never a
    // real webhook and pinning it out keeps the allow-list tight (defence in depth).
    if url.port().is_some() {
        return Err("Discord webhook URL must not specify a port".to_string());
    }
    Ok(url.to_string())
}

/// Validate a Telegram bot token: non-empty and free of URL-structural characters that
/// could reshape the `bot<token>/sendMessage` path (a real BotFather token is
/// `<digits>:<alnum-_->`). Shared by the save endpoint and [`send_telegram`] so the two
/// never disagree on what a usable token is.
pub fn validate_telegram_bot_token(token: &str) -> Result<(), String> {
    if token.trim().is_empty() {
        return Err("Telegram bot token must not be blank".to_string());
    }
    if token.contains(['/', ' ', '\t', '\n', '\r', '?', '#']) {
        return Err("Telegram bot token is malformed".to_string());
    }
    Ok(())
}

/// POST an alert to a Discord incoming webhook. Re-validates the URL first (defence in
/// depth — the stored value is trusted, but a bug/older row must never let an arbitrary
/// host through). The dedicated client disables redirects and carries a request timeout.
pub async fn send_discord(
    http: &reqwest::Client,
    webhook_url: &str,
    notification: &AlertNotification,
) -> ChannelOutcome {
    let url = match validate_discord_webhook_url(webhook_url) {
        Ok(url) => url,
        Err(reason) => return ChannelOutcome::fail("discord", reason),
    };
    let payload = json!({
        "username": "TCGLense",
        "content": notification.text(),
    });
    match http.post(&url).json(&payload).send().await {
        Ok(response) if response.status().is_success() => ChannelOutcome::ok("discord"),
        Ok(response) => {
            let status = response.status();
            tracing::warn!(%status, "Discord webhook rejected an alert");
            ChannelOutcome::fail("discord", format!("Discord returned {status}"))
        }
        Err(err) => {
            // The URL may embed the webhook secret, so never log the whole error/URL.
            tracing::warn!(
                is_timeout = err.is_timeout(),
                "Discord webhook request failed"
            );
            ChannelOutcome::fail("discord", "request to Discord failed")
        }
    }
}

/// POST an alert to a Telegram chat via the Bot API. The endpoint is the fixed
/// `https://api.telegram.org/bot<token>/sendMessage` — the token/chat id are the only
/// user-influenced values and they never form a host, so there is no SSRF surface.
pub async fn send_telegram(
    http: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    notification: &AlertNotification,
) -> ChannelOutcome {
    // Guard against a token with URL-structural characters that could reshape the path
    // (a real BotFather token is `<digits>:<alnum-_->`) — the same check the save endpoint runs.
    if let Err(reason) = validate_telegram_bot_token(bot_token) {
        return ChannelOutcome::fail("telegram", reason);
    }
    let endpoint = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let payload = json!({
        "chat_id": chat_id,
        "text": notification.text(),
        "disable_web_page_preview": true,
    });
    match http.post(&endpoint).json(&payload).send().await {
        Ok(response) if response.status().is_success() => ChannelOutcome::ok("telegram"),
        Ok(response) => {
            let status = response.status();
            tracing::warn!(%status, "Telegram API rejected an alert");
            ChannelOutcome::fail("telegram", format!("Telegram returned {status}"))
        }
        Err(err) => {
            tracing::warn!(is_timeout = err.is_timeout(), "Telegram request failed");
            ChannelOutcome::fail("telegram", "request to Telegram failed")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_url_validation_allows_only_discord_webhooks() {
        // Canonical accepted forms.
        for good in [
            "https://discord.com/api/webhooks/123/abc",
            "https://discordapp.com/api/webhooks/123/abc",
            "https://ptb.discord.com/api/webhooks/123/abc",
            "https://canary.discord.com/api/webhooks/9/xyz-_",
        ] {
            assert!(validate_discord_webhook_url(good).is_ok(), "{good}");
        }
        // Rejected: non-https, wrong host (SSRF), credentials, wrong path, explicit port.
        for bad in [
            "http://discord.com/api/webhooks/1/a",            // not https
            "https://evil.example.com/api/webhooks/1/a",      // wrong host
            "https://discord.com.evil.com/api/webhooks/1/a",  // lookalike host
            "https://user:pass@discord.com/api/webhooks/1/a", // credentials
            "https://discord.com/api/notwebhooks/1/a",        // wrong path
            "https://discord.com:8080/api/webhooks/1/a",      // explicit port
            "https://169.254.169.254/api/webhooks/1/a",       // metadata IP
            "not a url",
        ] {
            assert!(validate_discord_webhook_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn telegram_token_validation_rejects_url_structural_chars() {
        assert!(validate_telegram_bot_token("123456:AbC-def_123").is_ok());
        for bad in ["", "   ", "123/456", "123 456", "123?x", "123#x", "12\n3"] {
            assert!(validate_telegram_bot_token(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn notification_text_appends_the_link() {
        let n = AlertNotification {
            title: "t".into(),
            body: "Sol Ring is now $1.20".into(),
            url: Some("https://tcglense.app/cards/mtg/cards/abc".into()),
        };
        assert_eq!(
            n.text(),
            "Sol Ring is now $1.20\nhttps://tcglense.app/cards/mtg/cards/abc"
        );
        let n2 = AlertNotification { url: None, ..n };
        assert_eq!(n2.text(), "Sol Ring is now $1.20");
    }
}

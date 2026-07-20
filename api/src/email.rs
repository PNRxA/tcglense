//! Outbound transactional email (account verification, password resets).
//!
//! Two interchangeable providers, both HTTPS JSON APIs, dispatched by the
//! [`Emailer`] enum (an enum rather than a trait object, mirroring how
//! `collection_import` dispatches providers): [Resend](https://resend.com) and
//! [Cloudflare Email Service](https://developers.cloudflare.com/email-service/).
//! Configure exactly one; Resend wins if both are set (see [`Emailer::from_config`]).
//! Requests go out on the shared app HTTP client with a per-request timeout (the
//! shared client deliberately has no overall timeout).
//!
//! With no provider configured the emailer is [`Emailer::Disabled`]: sends are
//! skipped and the would-be message is logged loudly instead — offline dev and the
//! test suites keep working with zero network, and a misconfigured deploy (where
//! nobody can receive verification mail) is visible in the logs.

use std::time::Duration;

use serde_json::json;

use crate::{config::Config, error::AppError};

const RESEND_API_URL: &str = "https://api.resend.com/emails";

/// Base of Cloudflare's `client/v4` REST API. The Email Service send endpoint is
/// `/accounts/{account_id}/email/sending/send` under it.
const CLOUDFLARE_API_BASE: &str = "https://api.cloudflare.com/client/v4";

/// Whole-request deadline for one send. The shared client has no overall
/// timeout (the card-data bulk download streams for a while), so a hung email
/// API must not be able to stall an auth request indefinitely.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// One rendered outbound message.
#[derive(Debug, Clone)]
pub struct OutgoingEmail {
    pub to: String,
    pub subject: String,
    pub html: String,
    pub text: String,
}

/// A capturing sink for tests: every "sent" email lands in the shared vec so a
/// test can read the delivered link (the DB only ever holds the token's hash).
#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub struct Mailbox(std::sync::Arc<std::sync::Mutex<Vec<OutgoingEmail>>>);

#[cfg(test)]
impl Mailbox {
    pub fn emails(&self) -> Vec<OutgoingEmail> {
        self.0.lock().expect("mailbox lock").clone()
    }

    fn push(&self, email: OutgoingEmail) {
        self.0.lock().expect("mailbox lock").push(email);
    }
}

/// The slice of Cloudflare's `client/v4` response envelope we check: a send is only
/// accepted when the HTTP status is success **and** `success` is true — the v4 API can
/// return a 200 whose envelope reports `success: false`.
#[derive(serde::Deserialize)]
struct CloudflareEnvelope {
    success: bool,
}

/// The configured email sender, built once in `AppState::new`.
#[derive(Clone)]
pub enum Emailer {
    /// No email provider configured: skip the send and log the message. On a local
    /// dev host `log_body` is true so the otherwise-unrecoverable emailed link is
    /// logged; on an internet-facing host it is false, so the body (which carries a
    /// live single-use reset/verification token) is kept out of the logs.
    Disabled { log_body: bool },
    /// Send through Resend's HTTPS API with the configured key + From address.
    Resend {
        http: reqwest::Client,
        api_key: String,
        from: String,
    },
    /// Send through Cloudflare Email Service's REST API. The `account_id` rides in the
    /// send URL; the `api_token` is the bearer credential; `from` is the From address.
    Cloudflare {
        http: reqwest::Client,
        api_token: String,
        account_id: String,
        from: String,
    },
    /// Test-only: capture the message instead of sending anything.
    #[cfg(test)]
    Capture(Mailbox),
}

// Manual `Debug` so the provider credentials can never leak via `{:?}`/logs — mirrors
// the redaction the same secrets get in `Config`'s Debug impl.
impl std::fmt::Debug for Emailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Emailer::Disabled { .. } => f.write_str("Emailer::Disabled"),
            Emailer::Resend { from, .. } => f
                .debug_struct("Emailer::Resend")
                .field("api_key", &"[redacted]")
                .field("from", from)
                .finish(),
            Emailer::Cloudflare {
                account_id, from, ..
            } => f
                .debug_struct("Emailer::Cloudflare")
                .field("api_token", &"[redacted]")
                .field("account_id", account_id)
                .field("from", from)
                .finish(),
            #[cfg(test)]
            Emailer::Capture(_) => f.write_str("Emailer::Capture"),
        }
    }
}

impl Emailer {
    /// Assemble the emailer from config. Resend takes precedence so an existing
    /// Resend deployment is unaffected by the mere presence of Cloudflare variables;
    /// then Cloudflare Email Service when its pair is configured; otherwise disabled
    /// (dev/test mode). Configure exactly one provider.
    pub fn from_config(config: &Config, http: reqwest::Client) -> Self {
        if let Some(api_key) = config.resend_api_key.clone() {
            if config.cloudflare_email_api_token.is_some() {
                tracing::warn!(
                    "both RESEND_API_KEY and CLOUDFLARE_EMAIL_API_TOKEN are set; using Resend and \
                     ignoring the Cloudflare Email Service configuration"
                );
            }
            return Emailer::Resend {
                http,
                api_key,
                from: config.email_from.clone(),
            };
        }
        // The pair is validated both-or-neither at boot; matching both here is defensive
        // (a stray half-set pair falls through to Disabled rather than half-sending).
        if let (Some(api_token), Some(account_id)) = (
            config.cloudflare_email_api_token.clone(),
            config.cloudflare_account_id.clone(),
        ) {
            return Emailer::Cloudflare {
                http,
                api_token,
                account_id,
                from: config.email_from.clone(),
            };
        }
        // No provider configured. Log the full body only on a local dev host — on a
        // public one the body carries a live token that must never reach aggregated logs.
        Emailer::Disabled {
            log_body: !config.looks_like_production(),
        }
    }

    /// Whether a real email provider is configured. When `false` (no provider — dev),
    /// the emailed registration-completion link can't
    /// be delivered, so register **returns the completion token in the response
    /// body instead** (the SPA drives straight to the set-password step; no
    /// session until `POST /api/auth/complete-registration`), and login doesn't
    /// gate on email verification. `Capture` (tests) counts as enabled, so the
    /// test suites still exercise the real email-first flow (the token stays
    /// out of the response and is read from the captured email).
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Emailer::Disabled { .. })
    }

    /// Deliver one message. Callers on anti-enumeration endpoints must log and
    /// swallow the error (a surfaced failure would reveal that a send was
    /// attempted, i.e. that the account exists); see `handlers::auth`.
    pub async fn send(&self, email: OutgoingEmail) -> Result<(), AppError> {
        match self {
            Emailer::Disabled { log_body: true } => {
                // Local dev: the emailed link is otherwise unrecoverable (only the
                // token's hash is stored), so log the whole body deliberately loudly —
                // it's how the offline registration/reset journeys get their link, and
                // a dev deploy missing its provider config should be impossible to miss.
                tracing::warn!(
                    to = %email.to,
                    subject = %email.subject,
                    body = %email.text,
                    "email sending is disabled (no email provider configured); logging the message instead"
                );
                Ok(())
            }
            Emailer::Disabled { log_body: false } => {
                // Internet-facing host with no email provider (e.g. signups closed but
                // existing users can still request a reset): the body carries a live
                // single-use reset/verification token, so it must NOT be written to the
                // logs. Record only that a send was skipped — loudly, since account mail
                // isn't being delivered.
                tracing::warn!(
                    to = %email.to,
                    subject = %email.subject,
                    "email sending is disabled (no email provider configured) on an internet-facing \
                     deployment; the message body is withheld from logs because it contains a \
                     live token. Configure an email provider so account mail is delivered."
                );
                Ok(())
            }
            Emailer::Resend {
                http,
                api_key,
                from,
            } => {
                let response = http
                    .post(RESEND_API_URL)
                    .bearer_auth(api_key)
                    .json(&json!({
                        "from": from,
                        "to": [email.to],
                        "subject": email.subject,
                        "html": email.html,
                        "text": email.text,
                    }))
                    // Whole-request deadline (connect + headers + body).
                    .timeout(SEND_TIMEOUT)
                    .send()
                    .await
                    .map_err(|e| {
                        tracing::warn!(error = %e, "request to Resend failed");
                        AppError::BadGateway("failed to send email".to_string())
                    })?;

                let status = response.status();
                if !status.is_success() {
                    // Log a bounded snippet of the provider's error server-side;
                    // the client only ever sees the generic message.
                    let body = response.text().await.unwrap_or_default();
                    let snippet: String = body.chars().take(300).collect();
                    tracing::warn!(%status, body = %snippet, "Resend rejected the email");
                    return Err(AppError::BadGateway("failed to send email".to_string()));
                }
                Ok(())
            }
            Emailer::Cloudflare {
                http,
                api_token,
                account_id,
                from,
            } => {
                let url = format!("{CLOUDFLARE_API_BASE}/accounts/{account_id}/email/sending/send");
                let response = http
                    .post(&url)
                    .bearer_auth(api_token)
                    .json(&json!({
                        "from": from,
                        // Cloudflare's REST API takes a single `to` string (not an array).
                        "to": email.to,
                        "subject": email.subject,
                        "html": email.html,
                        "text": email.text,
                    }))
                    // Whole-request deadline (connect + headers + body).
                    .timeout(SEND_TIMEOUT)
                    .send()
                    .await
                    .map_err(|e| {
                        tracing::warn!(error = %e, "request to Cloudflare Email Service failed");
                        AppError::BadGateway("failed to send email".to_string())
                    })?;

                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                // Reject on a non-2xx status, or a 2xx whose envelope reports
                // `success: false`. A 2xx body that doesn't parse to the envelope is
                // trusted (treated as sent) so a benign response-shape change can't
                // silently drop every mail.
                let accepted = status.is_success()
                    && serde_json::from_str::<CloudflareEnvelope>(&body)
                        .map(|env| env.success)
                        .unwrap_or(true);
                if !accepted {
                    // Log a bounded snippet of the provider's error server-side;
                    // the client only ever sees the generic message.
                    let snippet: String = body.chars().take(300).collect();
                    tracing::warn!(
                        %status,
                        body = %snippet,
                        "Cloudflare Email Service rejected the email"
                    );
                    return Err(AppError::BadGateway("failed to send email".to_string()));
                }
                Ok(())
            }
            #[cfg(test)]
            Emailer::Capture(mailbox) => {
                mailbox.push(email);
                Ok(())
            }
        }
    }
}

/// The registration-completion message (email-first sign-up). `link` points at
/// the SPA's `/complete-registration?token=…` route, where the user chooses
/// their password; the wording mirrors the token's 24h expiry.
pub fn registration_email(to: &str, link: &str) -> OutgoingEmail {
    OutgoingEmail {
        to: to.to_string(),
        subject: "Finish creating your TCGLense account".to_string(),
        html: format!(
            "<p>Welcome to TCGLense!</p>\
             <p>Confirm this email address and choose a password to finish \
             creating your account:</p>\
             <p><a href=\"{link}\">Finish creating my account</a></p>\
             <p>The link is valid for 24 hours. If you didn't request a TCGLense \
             account, you can ignore this email.</p>"
        ),
        text: format!(
            "Welcome to TCGLense!\n\n\
             Confirm this email address and choose a password to finish creating \
             your account:\n\n{link}\n\n\
             The link is valid for 24 hours. If you didn't request a TCGLense \
             account, you can ignore this email.\n"
        ),
    }
}

/// The account-verification message. `link` points at the SPA's
/// `/verify-email?token=…` route; the wording mirrors the token's 24h expiry.
pub fn verification_email(to: &str, link: &str) -> OutgoingEmail {
    OutgoingEmail {
        to: to.to_string(),
        subject: "Verify your TCGLense email address".to_string(),
        html: format!(
            "<p>Welcome to TCGLense!</p>\
             <p>Confirm this email address to activate your account:</p>\
             <p><a href=\"{link}\">Verify my email</a></p>\
             <p>The link is valid for 24 hours. If you didn't create a TCGLense \
             account, you can ignore this email.</p>"
        ),
        text: format!(
            "Welcome to TCGLense!\n\n\
             Confirm this email address to activate your account:\n\n{link}\n\n\
             The link is valid for 24 hours. If you didn't create a TCGLense \
             account, you can ignore this email.\n"
        ),
    }
}

/// The password-reset message. `link` points at the SPA's
/// `/reset-password?token=…` route; the wording mirrors the token's 1h expiry.
pub fn password_reset_email(to: &str, link: &str) -> OutgoingEmail {
    OutgoingEmail {
        to: to.to_string(),
        subject: "Reset your TCGLense password".to_string(),
        html: format!(
            "<p>Someone asked to reset the password for this TCGLense account.</p>\
             <p><a href=\"{link}\">Choose a new password</a></p>\
             <p>The link is valid for 1 hour and can be used once. If this wasn't \
             you, you can ignore this email — your password is unchanged.</p>"
        ),
        text: format!(
            "Someone asked to reset the password for this TCGLense account.\n\n\
             Choose a new password:\n\n{link}\n\n\
             The link is valid for 1 hour and can be used once. If this wasn't \
             you, you can ignore this email — your password is unchanged.\n"
        ),
    }
}

/// A price-alert notification email (issue #525). `subject` is the short alert line and
/// `body` the human-readable message; `link` points at the card/product detail page. Kept
/// deliberately plain — an alert is transactional, not marketing.
pub fn alert_email(to: &str, subject: &str, body: &str, link: &str) -> OutgoingEmail {
    OutgoingEmail {
        to: to.to_string(),
        subject: subject.to_string(),
        html: format!(
            "<p>{body}</p>\
             <p><a href=\"{link}\">View on TCGLense</a></p>\
             <p>You're receiving this because you enabled notifications on TCGLense. \
             Manage them from the Alerts page.</p>"
        ),
        text: format!(
            "{body}\n\n{link}\n\n\
             You're receiving this because you enabled notifications on TCGLense. \
             Manage them from the Alerts page.\n"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_builders_embed_the_link_and_recipient() {
        let complete = registration_email(
            "a@example.com",
            "https://x.test/complete-registration?token=abc",
        );
        assert_eq!(complete.to, "a@example.com");
        assert!(
            complete
                .html
                .contains("https://x.test/complete-registration?token=abc")
        );
        assert!(
            complete
                .text
                .contains("https://x.test/complete-registration?token=abc")
        );

        let verify = verification_email("a@example.com", "https://x.test/verify-email?token=abc");
        assert_eq!(verify.to, "a@example.com");
        assert!(
            verify
                .html
                .contains("https://x.test/verify-email?token=abc")
        );
        assert!(
            verify
                .text
                .contains("https://x.test/verify-email?token=abc")
        );

        let reset =
            password_reset_email("a@example.com", "https://x.test/reset-password?token=abc");
        assert!(
            reset
                .html
                .contains("https://x.test/reset-password?token=abc")
        );
        assert!(
            reset
                .text
                .contains("https://x.test/reset-password?token=abc")
        );
    }

    #[tokio::test]
    async fn disabled_emailer_swallows_sends_and_capture_records_them() {
        // Both postures swallow the send as a logged no-op and count as disabled; the
        // only difference (whether the token-bearing body is logged) is not observable
        // through the return value.
        for disabled in [
            Emailer::Disabled { log_body: true },
            Emailer::Disabled { log_body: false },
        ] {
            assert!(!disabled.is_enabled());
            disabled
                .send(verification_email(
                    "a@example.com",
                    "https://x.test/v?token=t",
                ))
                .await
                .expect("disabled send is a logged no-op");
        }

        let mailbox = Mailbox::default();
        let capture = Emailer::Capture(mailbox.clone());
        capture
            .send(verification_email(
                "b@example.com",
                "https://x.test/v?token=t2",
            ))
            .await
            .expect("capture send succeeds");
        let sent = mailbox.emails();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "b@example.com");
    }

    #[test]
    fn from_config_selects_the_configured_provider() {
        use crate::test_support::test_config;
        let http = reqwest::Client::new();

        // No provider → Disabled.
        let disabled = Emailer::from_config(&test_config(), http.clone());
        assert!(matches!(disabled, Emailer::Disabled { .. }));
        assert!(!disabled.is_enabled());

        // Resend key alone → Resend.
        let resend = Emailer::from_config(
            &Config {
                resend_api_key: Some("re_live_key".to_string()),
                ..test_config()
            },
            http.clone(),
        );
        assert!(matches!(resend, Emailer::Resend { .. }));

        // A complete Cloudflare pair alone → Cloudflare.
        let cloudflare = Emailer::from_config(
            &Config {
                cloudflare_email_api_token: Some("cf_token".to_string()),
                cloudflare_account_id: Some("acct123".to_string()),
                ..test_config()
            },
            http.clone(),
        );
        assert!(matches!(cloudflare, Emailer::Cloudflare { .. }));

        // Both configured → Resend wins (documented precedence).
        let both = Emailer::from_config(
            &Config {
                resend_api_key: Some("re_live_key".to_string()),
                cloudflare_email_api_token: Some("cf_token".to_string()),
                cloudflare_account_id: Some("acct123".to_string()),
                ..test_config()
            },
            http,
        );
        assert!(matches!(both, Emailer::Resend { .. }));
    }

    #[test]
    fn cloudflare_emailer_is_enabled_and_redacts_its_token_in_debug() {
        let cloudflare = Emailer::Cloudflare {
            http: reqwest::Client::new(),
            api_token: "cf_super_secret".to_string(),
            account_id: "acct123".to_string(),
            from: "TCGLense <no-reply@tcglense.app>".to_string(),
        };
        assert!(cloudflare.is_enabled());
        let debug = format!("{cloudflare:?}");
        assert!(
            !debug.contains("cf_super_secret"),
            "token must be redacted: {debug}"
        );
        assert!(debug.contains("[redacted]"));
        // The account id (a non-secret identifier) and the From address are printed.
        assert!(debug.contains("acct123"));
        assert!(debug.contains("no-reply@tcglense.app"));
    }
}

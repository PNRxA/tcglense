//! Outbound transactional email (account verification, password resets).
//!
//! One provider today — [Resend](https://resend.com), an HTTPS JSON API —
//! dispatched by the [`Emailer`] enum, mirroring how `collection_import`
//! dispatches providers with an enum rather than a trait object. Requests go
//! out on the shared app HTTP client with a per-request timeout (the shared
//! client deliberately has no overall timeout).
//!
//! When `RESEND_API_KEY` is unset the emailer is [`Emailer::Disabled`]: sends
//! are skipped and the would-be message is logged loudly instead — offline dev
//! and the test suites keep working with zero network, and a misconfigured
//! deploy (where nobody can receive verification mail) is visible in the logs.

use std::time::Duration;

use serde_json::json;

use crate::{config::Config, error::AppError};

const RESEND_API_URL: &str = "https://api.resend.com/emails";

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

/// The configured email sender, built once in `AppState::new`.
#[derive(Clone)]
pub enum Emailer {
    /// No `RESEND_API_KEY` configured: skip the send and log the message.
    Disabled,
    /// Send through Resend's HTTPS API with the configured key + From address.
    Resend {
        http: reqwest::Client,
        api_key: String,
        from: String,
    },
    /// Test-only: capture the message instead of sending anything.
    #[cfg(test)]
    Capture(Mailbox),
}

// Manual `Debug` so the Resend API key can never leak via `{:?}`/logs — mirrors
// the redaction the same key gets in `Config`'s Debug impl.
impl std::fmt::Debug for Emailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Emailer::Disabled => f.write_str("Emailer::Disabled"),
            Emailer::Resend { from, .. } => f
                .debug_struct("Emailer::Resend")
                .field("api_key", &"[redacted]")
                .field("from", from)
                .finish(),
            #[cfg(test)]
            Emailer::Capture(_) => f.write_str("Emailer::Capture"),
        }
    }
}

impl Emailer {
    /// Assemble the emailer from config: Resend when a key is configured,
    /// otherwise disabled (dev/test mode).
    pub fn from_config(config: &Config, http: reqwest::Client) -> Self {
        match config.resend_api_key.clone() {
            Some(api_key) => Emailer::Resend {
                http,
                api_key,
                from: config.email_from.clone(),
            },
            None => Emailer::Disabled,
        }
    }

    /// Whether a real email provider is configured. When `false` (no
    /// `RESEND_API_KEY` — dev), the emailed registration-completion link can't
    /// be delivered, so register **returns the completion token in the response
    /// body instead** (the SPA drives straight to the set-password step; no
    /// session until `POST /api/auth/complete-registration`), and login doesn't
    /// gate on email verification. `Capture` (tests) counts as enabled, so the
    /// test suites still exercise the real email-first flow (the token stays
    /// out of the response and is read from the captured email).
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Emailer::Disabled)
    }

    /// Deliver one message. Callers on anti-enumeration endpoints must log and
    /// swallow the error (a surfaced failure would reveal that a send was
    /// attempted, i.e. that the account exists); see `handlers::auth`.
    pub async fn send(&self, email: OutgoingEmail) -> Result<(), AppError> {
        match self {
            Emailer::Disabled => {
                // Deliberately loud, and it includes the message body: in this
                // mode the emailed link is otherwise unrecoverable (only the
                // token's hash is stored), and a production deploy missing its
                // key should be impossible to miss in the logs.
                tracing::warn!(
                    to = %email.to,
                    subject = %email.subject,
                    body = %email.text,
                    "email sending is disabled (RESEND_API_KEY unset); logging the message instead"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_builders_embed_the_link_and_recipient() {
        let complete =
            registration_email("a@example.com", "https://x.test/complete-registration?token=abc");
        assert_eq!(complete.to, "a@example.com");
        assert!(complete.html.contains("https://x.test/complete-registration?token=abc"));
        assert!(complete.text.contains("https://x.test/complete-registration?token=abc"));

        let verify = verification_email("a@example.com", "https://x.test/verify-email?token=abc");
        assert_eq!(verify.to, "a@example.com");
        assert!(verify.html.contains("https://x.test/verify-email?token=abc"));
        assert!(verify.text.contains("https://x.test/verify-email?token=abc"));

        let reset = password_reset_email("a@example.com", "https://x.test/reset-password?token=abc");
        assert!(reset.html.contains("https://x.test/reset-password?token=abc"));
        assert!(reset.text.contains("https://x.test/reset-password?token=abc"));
    }

    #[tokio::test]
    async fn disabled_emailer_swallows_sends_and_capture_records_them() {
        let disabled = Emailer::Disabled;
        disabled
            .send(verification_email("a@example.com", "https://x.test/v?token=t"))
            .await
            .expect("disabled send is a logged no-op");

        let mailbox = Mailbox::default();
        let capture = Emailer::Capture(mailbox.clone());
        capture
            .send(verification_email("b@example.com", "https://x.test/v?token=t2"))
            .await
            .expect("capture send succeeds");
        let sent = mailbox.emails();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "b@example.com");
    }
}

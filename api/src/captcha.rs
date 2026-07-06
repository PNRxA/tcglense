//! CAPTCHA verification for the auth endpoints.
//!
//! One provider today — [Cloudflare Turnstile](https://developers.cloudflare.com/turnstile/),
//! dispatched by the [`Captcha`] enum (the same enum-not-trait shape as
//! [`crate::email::Emailer`] and `collection_import`'s provider). The token the
//! browser widget produces rides in the auth request body; the handler verifies
//! it server-side against Turnstile's `siteverify` before doing any work.
//!
//! When `TURNSTILE_SECRET_KEY` is unset the verifier is [`Captcha::Disabled`]:
//! every check passes, so offline dev and the test suites work with no widget
//! and no network. A configured verifier **requires** a valid token — a missing
//! or rejected one is a `400`, uniformly, before any account lookup (so it never
//! becomes an enumeration signal on the resend/forgot endpoints).

use std::{net::IpAddr, time::Duration};

use serde::Deserialize;
use serde_json::json;

use crate::{config::Config, error::AppError};

const TURNSTILE_VERIFY_URL: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

/// Whole-request deadline for one siteverify call. The shared client has no
/// overall timeout, so a hung Turnstile API must not stall an auth request.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(10);

/// The configured CAPTCHA verifier, built once in `AppState::new`.
#[derive(Clone)]
pub enum Captcha {
    /// No `TURNSTILE_SECRET_KEY`: every check passes (dev/test mode).
    Disabled,
    /// Verify tokens against Cloudflare Turnstile's `siteverify` endpoint.
    Turnstile {
        http: reqwest::Client,
        secret: String,
    },
    /// Test-only: pass iff a token is present and equals the expected value,
    /// so a test can exercise the enabled (token-required) path with no network.
    #[cfg(test)]
    ExpectToken(&'static str),
}

// Manual `Debug` so the Turnstile secret can never leak via `{:?}`/logs.
impl std::fmt::Debug for Captcha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Captcha::Disabled => f.write_str("Captcha::Disabled"),
            Captcha::Turnstile { .. } => f
                .debug_struct("Captcha::Turnstile")
                .field("secret", &"[redacted]")
                .finish(),
            #[cfg(test)]
            Captcha::ExpectToken(_) => f.write_str("Captcha::ExpectToken"),
        }
    }
}

/// The shape of Turnstile's `siteverify` response (only the fields we consume).
#[derive(Debug, Deserialize)]
struct SiteVerifyResponse {
    success: bool,
    #[serde(default, rename = "error-codes")]
    error_codes: Vec<String>,
}

impl Captcha {
    /// Assemble the verifier from config: Turnstile when a secret is configured,
    /// otherwise disabled (dev/test mode).
    pub fn from_config(config: &Config, http: reqwest::Client) -> Self {
        match config.turnstile_secret_key.clone() {
            Some(secret) => Captcha::Turnstile { http, secret },
            None => Captcha::Disabled,
        }
    }

    /// Whether a token is actually required (a real verifier is configured).
    /// The web app decides whether to render the widget from the paired
    /// `turnstile_site_key` served by `GET /api/config`; this is only for
    /// logging/introspection.
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Captcha::Disabled)
    }

    /// Verify the presented CAPTCHA token. `remote_ip` (the client's address, when
    /// resolvable) is forwarded to Turnstile as an extra signal.
    ///
    /// A missing/blank token against an enabled verifier, or a rejected token, is
    /// a `400 BadRequest` — deliberately NOT a 401/403, so it never collides with
    /// the login "email not verified" 403 the SPA branches on. The check runs
    /// before any account lookup, so its outcome reveals nothing about accounts.
    pub async fn verify(
        &self,
        token: Option<&str>,
        remote_ip: Option<IpAddr>,
    ) -> Result<(), AppError> {
        let (http, secret) = match self {
            Captcha::Disabled => return Ok(()),
            #[cfg(test)]
            Captcha::ExpectToken(expected) => {
                return match token {
                    Some(t) if t == *expected => Ok(()),
                    _ => Err(AppError::BadRequest("captcha verification failed".to_string())),
                };
            }
            Captcha::Turnstile { http, secret } => (http, secret),
        };

        let token = token
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| AppError::BadRequest("captcha token is required".to_string()))?;

        // Turnstile's siteverify accepts a JSON body (as well as form-encoded);
        // we use JSON since that's the reqwest feature this crate already enables.
        let body = json!({
            "secret": secret,
            "response": token,
            "remoteip": remote_ip.map(|ip| ip.to_string()),
        });

        let response = http
            .post(TURNSTILE_VERIFY_URL)
            .json(&body)
            .timeout(VERIFY_TIMEOUT)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "request to Turnstile failed");
                // Fail closed: an unverifiable token is treated as a failed check
                // rather than letting the request through.
                AppError::BadRequest("captcha verification failed".to_string())
            })?;

        let outcome: SiteVerifyResponse = response.json().await.map_err(|e| {
            tracing::warn!(error = %e, "could not parse the Turnstile response");
            AppError::BadRequest("captcha verification failed".to_string())
        })?;

        if !outcome.success {
            tracing::warn!(codes = ?outcome.error_codes, "Turnstile rejected the token");
            return Err(AppError::BadRequest(
                "captcha verification failed".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_verifier_always_passes() {
        let captcha = Captcha::Disabled;
        captcha.verify(None, None).await.expect("disabled passes");
        captcha
            .verify(Some("anything"), None)
            .await
            .expect("disabled passes with a token too");
    }

    #[tokio::test]
    async fn enabled_verifier_rejects_a_missing_token_without_network() {
        // The ExpectToken variant models an enabled verifier; a missing/blank or
        // wrong token fails the check with a 400 and never touches the network.
        let captcha = Captcha::ExpectToken("good-token");

        let err = captcha.verify(None, None).await.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
        let err = captcha.verify(Some("  "), None).await.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
        let err = captcha.verify(Some("wrong"), None).await.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));

        captcha
            .verify(Some("good-token"), None)
            .await
            .expect("a matching token passes");
    }
}

use std::env;

/// Application configuration, sourced from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    /// Lifetime of an access JWT, in minutes (short-lived).
    pub access_token_expiry_minutes: i64,
    /// Lifetime of an opaque refresh token, in days (long-lived).
    pub refresh_token_expiry_days: i64,
    /// Whether the refresh cookie is marked `Secure` (HTTPS-only).
    pub cookie_secure: bool,
    /// Network interface to bind (0.0.0.0 in dev/containers, 127.0.0.1 behind a proxy).
    pub host: String,
    pub port: u16,
}

/// Clearly-labelled, insecure default secret so `cargo run` works out of the box.
/// A warning is logged whenever this fallback is used.
const DEV_ONLY_JWT_SECRET: &str = "dev-only-insecure-jwt-secret-do-not-use-in-production";

impl Config {
    /// Build a [`Config`] from the process environment, applying sane defaults.
    pub fn from_env() -> Self {
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://tcglense.db?mode=rwc".to_string());

        let cookie_secure = env::var("COOKIE_SECURE")
            .ok()
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);

        let jwt_secret = match env::var("JWT_SECRET") {
            Ok(secret) if !secret.trim().is_empty() => secret,
            _ => {
                // COOKIE_SECURE=true signals a production (HTTPS) deployment; refuse
                // to start with a forgeable, compiled-in signing secret there.
                if cookie_secure {
                    panic!(
                        "JWT_SECRET must be set when COOKIE_SECURE is enabled. Refusing to \
                         start with the insecure dev-only fallback secret in production."
                    );
                }
                tracing::warn!(
                    "JWT_SECRET is not set; falling back to an INSECURE dev-only secret. \
                     Set JWT_SECRET before deploying to production."
                );
                DEV_ONLY_JWT_SECRET.to_string()
            }
        };

        let access_token_expiry_minutes = env::var("ACCESS_TOKEN_EXPIRY_MINUTES")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|m| *m > 0)
            .unwrap_or(15);

        let refresh_token_expiry_days = env::var("REFRESH_TOKEN_EXPIRY_DAYS")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|d| *d > 0)
            .unwrap_or(30);

        let host = env::var("HOST")
            .ok()
            .filter(|h| !h.trim().is_empty())
            .unwrap_or_else(|| "0.0.0.0".to_string());

        let port = env::var("PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8080);

        Config {
            database_url,
            jwt_secret,
            access_token_expiry_minutes,
            refresh_token_expiry_days,
            cookie_secure,
            host,
            port,
        }
    }
}

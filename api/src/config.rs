use std::env;

/// Application configuration, sourced from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub jwt_expiry_days: i64,
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

        let jwt_secret = match env::var("JWT_SECRET") {
            Ok(secret) if !secret.trim().is_empty() => secret,
            _ => {
                tracing::warn!(
                    "JWT_SECRET is not set; falling back to an INSECURE dev-only secret. \
                     Set JWT_SECRET before deploying to production."
                );
                DEV_ONLY_JWT_SECRET.to_string()
            }
        };

        let jwt_expiry_days = env::var("JWT_EXPIRY_DAYS")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|d| *d > 0)
            .unwrap_or(7);

        let port = env::var("PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8080);

        Config {
            database_url,
            jwt_secret,
            jwt_expiry_days,
            port,
        }
    }
}

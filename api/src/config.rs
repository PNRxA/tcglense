use std::{env, path::PathBuf};

/// Application configuration, sourced from environment variables.
///
/// `Debug` is implemented manually so the signing secret is never printed.
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    /// Lifetime of an access JWT, in minutes (short-lived).
    pub access_token_expiry_minutes: i64,
    /// Lifetime of an opaque refresh token, in days (long-lived).
    pub refresh_token_expiry_days: i64,
    /// Whether the refresh cookie is marked `Secure` (HTTPS-only).
    pub cookie_secure: bool,
    /// Network interface to bind. Defaults to 127.0.0.1; set 0.0.0.0 in dev/containers.
    pub host: String,
    pub port: u16,
    /// Base directory for downloaded assets; card images live under `images/`.
    pub data_dir: PathBuf,
    /// `User-Agent` sent to Scryfall (their API guidelines require a descriptive one).
    pub scryfall_user_agent: String,
    /// Whether to import card data from providers on startup (disable in tests).
    pub sync_on_startup: bool,
}

impl std::fmt::Debug for Config {
    /// Redacts `jwt_secret` so the signing key can never leak via `{:?}`/logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &self.database_url)
            .field("jwt_secret", &"[redacted]")
            .field("access_token_expiry_minutes", &self.access_token_expiry_minutes)
            .field("refresh_token_expiry_days", &self.refresh_token_expiry_days)
            .field("cookie_secure", &self.cookie_secure)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("data_dir", &self.data_dir)
            .field("scryfall_user_agent", &self.scryfall_user_agent)
            .field("sync_on_startup", &self.sync_on_startup)
            .finish()
    }
}

/// Clearly-labelled, publicly-known insecure secret for local development only.
/// It is used **only** when `ALLOW_INSECURE_DEV_SECRET=true` is set explicitly;
/// every other deployment must supply a real `JWT_SECRET` or the server refuses
/// to start. A warning is logged whenever this fallback is used.
const DEV_ONLY_JWT_SECRET: &str = "dev-only-insecure-jwt-secret-do-not-use-in-production";

/// Minimum acceptable length (in bytes) for a real `JWT_SECRET`. HS256 keys
/// shorter than this are brute-forceable offline.
const MIN_JWT_SECRET_LEN: usize = 32;

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

        // The insecure compiled-in fallback secret is *opt-in only*. By default an
        // absent JWT_SECRET fails the boot closed, so a misconfigured production
        // deploy can never silently sign tokens with a publicly-known key — even if
        // COOKIE_SECURE was forgotten (e.g. TLS terminated at a reverse proxy).
        let allow_insecure_dev_secret = env::var("ALLOW_INSECURE_DEV_SECRET")
            .ok()
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);

        let jwt_secret = match env::var("JWT_SECRET") {
            Ok(secret) if !secret.trim().is_empty() => {
                if secret == DEV_ONLY_JWT_SECRET {
                    panic!(
                        "JWT_SECRET is set to the public dev-only fallback value. Generate a \
                         unique secret, e.g. `openssl rand -hex 32`."
                    );
                }
                if secret.len() < MIN_JWT_SECRET_LEN {
                    panic!(
                        "JWT_SECRET is too short ({} bytes); use at least {MIN_JWT_SECRET_LEN} \
                         bytes of high-entropy randomness, e.g. `openssl rand -hex 32`.",
                        secret.len()
                    );
                }
                secret
            }
            _ => {
                if !allow_insecure_dev_secret {
                    panic!(
                        "JWT_SECRET must be set. Refusing to start with the public, compiled-in \
                         dev-only signing secret. Set JWT_SECRET to a unique high-entropy value, \
                         or set ALLOW_INSECURE_DEV_SECRET=true for local development only."
                    );
                }
                tracing::warn!(
                    "JWT_SECRET is not set; using the INSECURE, publicly-known dev-only secret \
                     because ALLOW_INSECURE_DEV_SECRET is enabled. NEVER enable this outside \
                     local development."
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

        // Default to loopback so an operator who only sets PORT does not expose the
        // API on every interface. Containers/dev set HOST=0.0.0.0 explicitly.
        let host = env::var("HOST")
            .ok()
            .filter(|h| !h.trim().is_empty())
            .unwrap_or_else(|| "127.0.0.1".to_string());

        let port = env::var("PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8080);

        let data_dir = env::var("DATA_DIR")
            .ok()
            .filter(|d| !d.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./data"));

        let scryfall_user_agent = env::var("SCRYFALL_USER_AGENT")
            .ok()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| {
                "TCGLense/0.1 (+https://github.com/PNRxA/tcglense)".to_string()
            });

        // Importing card data is the default; tests and offline runs disable it.
        let sync_on_startup = env::var("SYNC_ON_STARTUP")
            .ok()
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(true);

        Config {
            database_url,
            jwt_secret,
            access_token_expiry_minutes,
            refresh_token_expiry_days,
            cookie_secure,
            host,
            port,
            data_dir,
            scryfall_user_agent,
            sync_on_startup,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_the_jwt_secret() {
        let config = Config {
            database_url: "sqlite::memory:".to_string(),
            jwt_secret: "super-secret-signing-key-value".to_string(),
            access_token_expiry_minutes: 15,
            refresh_token_expiry_days: 30,
            cookie_secure: true,
            host: "127.0.0.1".to_string(),
            port: 8080,
            data_dir: std::path::PathBuf::from("./data"),
            scryfall_user_agent: "TCGLense/test".to_string(),
            sync_on_startup: false,
        };

        let rendered = format!("{config:?}");
        assert!(rendered.contains("[redacted]"));
        assert!(!rendered.contains("super-secret-signing-key-value"));
    }
}

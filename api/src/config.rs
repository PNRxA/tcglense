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
    /// Public origin where the SPA is served (e.g. `https://tcglense.app`), used to
    /// build the absolute `<loc>` URLs in the DB-backed sitemaps (see
    /// [`crate::handlers::sitemap`]). Defaults to the Vite dev origin; set it to the
    /// real site origin in production. Trailing slashes are trimmed so URL joins
    /// never double up.
    pub public_site_url: String,
    /// Base directory for downloaded assets; card images live under `images/`.
    pub data_dir: PathBuf,
    /// `User-Agent` sent to Scryfall (their API guidelines require a descriptive one).
    pub scryfall_user_agent: String,
    /// Whether to import card data from providers on startup (disable in tests).
    pub sync_on_startup: bool,
    /// How often to re-import card data after the startup import, in hours.
    /// Defaults to 24 (daily); `0` disables the periodic refresh (startup only).
    /// Only takes effect when `sync_on_startup` is enabled.
    pub sync_interval_hours: u64,
    /// Seed a small dummy offline catalog instead of importing real card data.
    /// When true this takes precedence over `sync_on_startup`/`sync_interval_hours`:
    /// the server inserts deterministic fake sets/cards on boot and performs NO
    /// network sync. For offline dev, CI, and tests; never enable in production.
    pub seed_dummy_data: bool,
}

impl std::fmt::Debug for Config {
    /// Redacts `jwt_secret` so the signing key can never leak via `{:?}`/logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &self.database_url)
            .field("jwt_secret", &"[redacted]")
            .field(
                "access_token_expiry_minutes",
                &self.access_token_expiry_minutes,
            )
            .field("refresh_token_expiry_days", &self.refresh_token_expiry_days)
            .field("cookie_secure", &self.cookie_secure)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("public_site_url", &self.public_site_url)
            .field("data_dir", &self.data_dir)
            .field("scryfall_user_agent", &self.scryfall_user_agent)
            .field("sync_on_startup", &self.sync_on_startup)
            .field("sync_interval_hours", &self.sync_interval_hours)
            .field("seed_dummy_data", &self.seed_dummy_data)
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

/// Resolve and validate the JWT signing secret from the (already-read) env value.
///
/// Returns the secret the server should sign with, or an `Err(message)` the
/// caller turns into a startup panic. Kept as a pure function (no env access, no
/// panics) so the boot-closed security policy — reject a missing, blank, public,
/// or too-short secret unless `ALLOW_INSECURE_DEV_SECRET` opts in — is directly
/// unit-testable without mutating process-global environment state.
///
/// A blank/whitespace value is treated as "absent" so an empty `JWT_SECRET=` in a
/// stray `.env` can't silently sign tokens with the public dev key.
fn resolve_jwt_secret(
    provided: Option<&str>,
    allow_insecure_dev_secret: bool,
) -> Result<String, String> {
    match provided {
        Some(secret) if !secret.trim().is_empty() => {
            if secret == DEV_ONLY_JWT_SECRET {
                return Err("JWT_SECRET is set to the public dev-only fallback value. Generate a \
                            unique secret, e.g. `openssl rand -hex 32`."
                    .to_string());
            }
            if secret.len() < MIN_JWT_SECRET_LEN {
                return Err(format!(
                    "JWT_SECRET is too short ({} bytes); use at least {MIN_JWT_SECRET_LEN} bytes \
                     of high-entropy randomness, e.g. `openssl rand -hex 32`.",
                    secret.len()
                ));
            }
            Ok(secret.to_string())
        }
        _ => {
            if !allow_insecure_dev_secret {
                return Err("JWT_SECRET must be set. Refusing to start with the public, \
                            compiled-in dev-only signing secret. Set JWT_SECRET to a unique \
                            high-entropy value, or set ALLOW_INSECURE_DEV_SECRET=true for local \
                            development only."
                    .to_string());
            }
            tracing::warn!(
                "JWT_SECRET is not set; using the INSECURE, publicly-known dev-only secret \
                 because ALLOW_INSECURE_DEV_SECRET is enabled. NEVER enable this outside \
                 local development."
            );
            Ok(DEV_ONLY_JWT_SECRET.to_string())
        }
    }
}

/// Parse a boolean-ish env var, accepting the common truthy spellings
/// (`1`/`true`/`yes`/`on`, case- and whitespace-insensitive). A set-but-not-truthy
/// value reads as `false`; only an unset var falls back to `default`.
fn env_bool(key: &str, default: bool) -> bool {
    match env::var(key) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

/// Read an env var, returning `None` if it is unset or blank/whitespace-only.
/// The returned string is the raw value (not trimmed).
fn env_trimmed(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// Parse an env var into `T`, returning `None` if unset, blank, or unparseable.
fn env_parse<T: std::str::FromStr>(key: &str) -> Option<T> {
    env_trimmed(key).and_then(|v| v.trim().parse::<T>().ok())
}

impl Config {
    /// Build a [`Config`] from the process environment, applying sane defaults.
    pub fn from_env() -> Self {
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://tcglense.db?mode=rwc".to_string());

        let cookie_secure = env_bool("COOKIE_SECURE", false);

        // The insecure compiled-in fallback secret is *opt-in only*. By default an
        // absent JWT_SECRET fails the boot closed, so a misconfigured production
        // deploy can never silently sign tokens with a publicly-known key — even if
        // COOKIE_SECURE was forgotten (e.g. TLS terminated at a reverse proxy).
        let allow_insecure_dev_secret = env_bool("ALLOW_INSECURE_DEV_SECRET", false);

        let provided_secret = env::var("JWT_SECRET").ok();
        let jwt_secret =
            match resolve_jwt_secret(provided_secret.as_deref(), allow_insecure_dev_secret) {
                Ok(secret) => secret,
                Err(message) => panic!("{message}"),
            };

        let access_token_expiry_minutes = env_parse::<i64>("ACCESS_TOKEN_EXPIRY_MINUTES")
            .filter(|m| *m > 0)
            .unwrap_or(15);

        let refresh_token_expiry_days = env_parse::<i64>("REFRESH_TOKEN_EXPIRY_DAYS")
            .filter(|d| *d > 0)
            .unwrap_or(30);

        // Default to loopback so an operator who only sets PORT does not expose the
        // API on every interface. Containers/dev set HOST=0.0.0.0 explicitly.
        let host = env_trimmed("HOST").unwrap_or_else(|| "127.0.0.1".to_string());

        let port = env_parse::<u16>("PORT").unwrap_or(8080);

        // Public origin of the SPA, used for the absolute <loc>s in the sitemaps.
        // Defaults to the Vite dev origin so dev/e2e produce valid URLs. Trailing
        // slashes are trimmed so `base + "/cards/..."` never yields a doubled slash.
        let public_site_url = env_trimmed("PUBLIC_SITE_URL")
            .map(|v| v.trim().trim_end_matches('/').to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "http://localhost:5173".to_string());

        let data_dir = env_trimmed("DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./data"));

        let scryfall_user_agent = env_trimmed("SCRYFALL_USER_AGENT")
            .unwrap_or_else(|| "TCGLense/0.1 (+https://github.com/PNRxA/tcglense)".to_string());

        // Importing card data is the default; tests and offline runs disable it.
        let sync_on_startup = env_bool("SYNC_ON_STARTUP", true);

        // Re-import cadence after the startup import. Default daily; `0` means
        // "startup only" (no periodic refresh). An unparseable value falls back to
        // the default rather than disabling refreshes silently.
        let sync_interval_hours = env_parse::<u64>("SYNC_INTERVAL_HOURS").unwrap_or(24);

        // Seed a dummy offline catalog instead of syncing real data. Parsed like the
        // other boolean flags; main.rs gives it precedence over the sync settings.
        let seed_dummy_data = env_bool("SEED_DUMMY_DATA", false);

        Config {
            database_url,
            jwt_secret,
            access_token_expiry_minutes,
            refresh_token_expiry_days,
            cookie_secure,
            host,
            port,
            public_site_url,
            data_dir,
            scryfall_user_agent,
            sync_on_startup,
            sync_interval_hours,
            seed_dummy_data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_the_jwt_secret() {
        let config = Config {
            jwt_secret: "super-secret-signing-key-value".to_string(),
            cookie_secure: true,
            ..crate::test_support::test_config()
        };

        let rendered = format!("{config:?}");
        assert!(rendered.contains("[redacted]"));
        assert!(!rendered.contains("super-secret-signing-key-value"));
    }

    // ----- JWT secret policy (boot-closed) -----

    #[test]
    fn jwt_secret_accepts_a_real_high_entropy_value() {
        let good = "0123456789abcdef0123456789abcdef"; // 32 bytes
        assert_eq!(good.len(), MIN_JWT_SECRET_LEN);
        assert_eq!(resolve_jwt_secret(Some(good), false).as_deref(), Ok(good));
    }

    #[test]
    fn jwt_secret_rejects_the_public_dev_constant_even_when_long() {
        // The dev constant is longer than the minimum, so it must be rejected on
        // identity, not length — otherwise the public key would pass the gate.
        assert!(DEV_ONLY_JWT_SECRET.len() >= MIN_JWT_SECRET_LEN);
        assert!(resolve_jwt_secret(Some(DEV_ONLY_JWT_SECRET), false).is_err());
        // Even with the insecure opt-in, an *explicitly* configured public secret
        // is still rejected (the opt-in only covers an absent secret).
        assert!(resolve_jwt_secret(Some(DEV_ONLY_JWT_SECRET), true).is_err());
    }

    #[test]
    fn jwt_secret_rejects_a_too_short_value() {
        let short = "a".repeat(MIN_JWT_SECRET_LEN - 1);
        assert!(resolve_jwt_secret(Some(&short), false).is_err());
        assert!(resolve_jwt_secret(Some(&short), true).is_err());
    }

    #[test]
    fn jwt_secret_treats_absent_or_blank_as_unset() {
        // Absent / blank / whitespace-only all fail closed without the opt-in.
        assert!(resolve_jwt_secret(None, false).is_err());
        assert!(resolve_jwt_secret(Some(""), false).is_err());
        assert!(resolve_jwt_secret(Some("   "), false).is_err());
    }

    #[test]
    fn jwt_secret_insecure_opt_in_falls_back_only_when_unset() {
        // With the explicit opt-in, an unset secret degrades to the dev constant.
        assert_eq!(
            resolve_jwt_secret(None, true).as_deref(),
            Ok(DEV_ONLY_JWT_SECRET)
        );
        assert_eq!(
            resolve_jwt_secret(Some("   "), true).as_deref(),
            Ok(DEV_ONLY_JWT_SECRET)
        );
    }
}

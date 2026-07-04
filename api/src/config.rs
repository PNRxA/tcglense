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
    /// `User-Agent` sent to TCGCSV (https://tcgcsv.com) on the one-time historic
    /// price backfill. TCGCSV blocks generic/unset User-Agents, so a descriptive one
    /// (with a contact URL/email) is required. Not a secret.
    pub tcgcsv_user_agent: String,
    /// Master switch for the one-time TCGCSV historic price backfill (see
    /// [`crate::tcgcsv`]). Default `false` (opt-in): the walk over TCGCSV's daily
    /// archives can take a while and hit an external service, so it never runs unless
    /// explicitly enabled. Set `true` to run it.
    pub price_backfill_enabled: bool,
    /// Cap on how many of the most recent archive days the price backfill downloads.
    /// `0` (the default) = every archive day since TCGCSV's first (2024-02-08); `N` =
    /// only the most recent `N` days. Useful to bound a first run.
    pub price_backfill_days: u32,
    /// `User-Agent` sent to Moxfield, when set. Moxfield's API sits behind bot
    /// protection that rejects unknown clients; they approve specific User-Agent
    /// strings on request (email support@moxfield.com). Unset = requests go out with
    /// the client's default UA (and will likely be rejected with a clear error).
    pub moxfield_user_agent: Option<String>,
    /// API key for [Resend](https://resend.com), the transactional-email provider
    /// (verification + password-reset mail). Unset = email sending is disabled:
    /// sends are skipped and logged instead (offline dev / tests). A credential —
    /// redacted in `Debug`.
    pub resend_api_key: Option<String>,
    /// `From` address on outbound email, e.g. `TCGLense <no-reply@tcglense.app>`.
    /// The domain must be verified with the email provider; the default is
    /// Resend's shared onboarding sender, which only delivers to the Resend
    /// account owner's own address (fine for dev, set a real one in production).
    pub email_from: String,
    /// Cloudflare Turnstile secret key, used to verify the CAPTCHA token the
    /// browser widget produces on the auth forms. A credential — redacted in
    /// `Debug`. Unset = CAPTCHA verification is disabled (checks pass; no widget
    /// is expected). The matching public site key lives in the web build's
    /// `VITE_TURNSTILE_SITE_KEY`.
    pub turnstile_secret_key: Option<String>,
    /// Whether to trust `X-Forwarded-For` / `Forwarded` when resolving the client
    /// IP for rate limiting. Default `false` (use the socket peer). Enable ONLY
    /// when the API sits behind a trusted reverse proxy that sets the header —
    /// trusting it when directly exposed lets a client spoof its IP and dodge the
    /// per-IP limits.
    pub trust_proxy_headers: bool,
    /// Master switch for the per-IP auth rate limiting. Default `true`; set
    /// `false` to defer entirely to an upstream WAF/proxy limiter.
    pub rate_limit_enabled: bool,
    /// Connection URL for a Redis backing the rate limiters (per-IP + per-user),
    /// e.g. `redis://127.0.0.1:6379`. Unset = the limiters run in-memory /
    /// per-process (single-instance posture). Set it so a multi-instance deploy
    /// shares one limiter state. Only plain `redis://` is supported (this build
    /// links the no-TLS `redis` feature set); a `rediss://` URL degrades to
    /// in-memory. The URL may embed a password — redacted in `Debug`. If it's set
    /// but Redis is unreachable at boot, the server starts degraded (in-memory)
    /// with a warning rather than failing (see [`crate::ratelimit::AuthRateLimiter`]).
    pub redis_url: Option<String>,
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
    /// Run the image proxy in "CDN mode": don't cache card images / set icons on
    /// local disk. The origin still fetches each asset from the provider on demand
    /// and serves it with the same `immutable` cache headers, but skips persisting
    /// it — meant for deployments fronted by a CDN that caches those responses, so
    /// the origin needs no writable image directory and is only hit on a CDN cache
    /// miss. Leave false when no CDN sits in front, or every view re-fetches
    /// upstream. See [`crate::catalog::images`].
    pub cdn_mode: bool,
    /// Optional directory of static web assets (the built SPA) for the API to serve
    /// itself. When set, any request not matched by an `/api/...` route falls back to
    /// this directory, and unknown paths resolve to its `index.html` so client-side
    /// SPA routes work. Unset (the default) = the API serves only `/api` and returns
    /// its normal 404 for everything else, so existing API-only deployments are
    /// unaffected. This powers the single-process "combined" Docker image (the API
    /// and SPA in one container); the split deployment (Caddy serving the SPA and
    /// proxying `/api`) leaves it unset. See [`crate::router`].
    pub web_root: Option<PathBuf>,
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
            .field("tcgcsv_user_agent", &self.tcgcsv_user_agent)
            .field("price_backfill_enabled", &self.price_backfill_enabled)
            .field("price_backfill_days", &self.price_backfill_days)
            // The Moxfield-approved UA is a credential (it's what their allow-list
            // keys on), so print only whether one is configured.
            .field(
                "moxfield_user_agent",
                &self.moxfield_user_agent.as_ref().map(|_| "[redacted]"),
            )
            // The Resend API key is a credential; print only whether one is set.
            .field(
                "resend_api_key",
                &self.resend_api_key.as_ref().map(|_| "[redacted]"),
            )
            .field("email_from", &self.email_from)
            // The Turnstile secret is a credential; print only whether one is set.
            .field(
                "turnstile_secret_key",
                &self.turnstile_secret_key.as_ref().map(|_| "[redacted]"),
            )
            .field("trust_proxy_headers", &self.trust_proxy_headers)
            .field("rate_limit_enabled", &self.rate_limit_enabled)
            // A Redis URL may embed a password; print only whether one is configured.
            .field("redis_url", &self.redis_url.as_ref().map(|_| "[redacted]"))
            .field("sync_on_startup", &self.sync_on_startup)
            .field("sync_interval_hours", &self.sync_interval_hours)
            .field("seed_dummy_data", &self.seed_dummy_data)
            .field("cdn_mode", &self.cdn_mode)
            .field("web_root", &self.web_root)
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
/// `pub(crate)` so the DB pool sizing in [`crate::db`] reuses the same trim/blank
/// semantics instead of hand-rolling its own numeric env parsing.
pub(crate) fn env_parse<T: std::str::FromStr>(key: &str) -> Option<T> {
    env_trimmed(key).and_then(|v| v.trim().parse::<T>().ok())
}

/// Whether this looks like an internet-facing deployment rather than local dev —
/// used *only* to decide whether to emit the production-posture startup warnings
/// (it never changes behaviour). The signal is a non-local `PUBLIC_SITE_URL` (which
/// a real deploy must set for correct sitemap/email links) or a concrete
/// non-loopback bind host. The dev-common `0.0.0.0` wildcard is treated as local so
/// container dev doesn't trip the warnings.
fn looks_like_production(host: &str, public_site_url: &str) -> bool {
    let site_is_local =
        public_site_url.contains("localhost") || public_site_url.contains("127.0.0.1");
    let host_is_local = matches!(host, "127.0.0.1" | "::1" | "localhost" | "0.0.0.0");
    !site_is_local || !host_is_local
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

        // TCGCSV blocks generic UAs; reuse the Scryfall default's shape as a fallback.
        let tcgcsv_user_agent = env_trimmed("TCGCSV_USER_AGENT")
            .unwrap_or_else(|| "TCGLense/0.1 (+https://github.com/PNRxA/tcglense)".to_string());

        // The one-time historic price backfill is opt-in (off by default); `0` days = all archives.
        let price_backfill_enabled = env_bool("PRICE_BACKFILL_ENABLED", false);
        let price_backfill_days = env_parse::<u32>("PRICE_BACKFILL_DAYS").unwrap_or(0);

        // Moxfield only serves approved User-Agents (see the struct field); no default.
        let moxfield_user_agent = env_trimmed("MOXFIELD_USER_AGENT");

        // Unset = email sending disabled (sends are logged instead) — see the field.
        let resend_api_key = env_trimmed("RESEND_API_KEY");

        // Resend's shared onboarding sender only delivers to the account owner's
        // own address — enough for dev; production sets a verified-domain sender.
        let email_from = env_trimmed("EMAIL_FROM")
            .unwrap_or_else(|| "TCGLense <onboarding@resend.dev>".to_string());

        // Unset = CAPTCHA disabled (checks pass) — see the field.
        let turnstile_secret_key = env_trimmed("TURNSTILE_SECRET_KEY");

        // Only trust proxy IP headers when explicitly opted in (see the field);
        // rate limiting is on by default.
        let trust_proxy_headers = env_bool("TRUST_PROXY_HEADERS", false);
        let rate_limit_enabled = env_bool("RATE_LIMIT_ENABLED", true);

        // Unset = the rate limiters run in-memory / per-process (see the field).
        let redis_url = env_trimmed("REDIS_URL");

        // Importing card data is the default; tests and offline runs disable it.
        let sync_on_startup = env_bool("SYNC_ON_STARTUP", true);

        // Re-import cadence after the startup import. Default daily; `0` means
        // "startup only" (no periodic refresh). An unparseable value falls back to
        // the default rather than disabling refreshes silently.
        let sync_interval_hours = env_parse::<u64>("SYNC_INTERVAL_HOURS").unwrap_or(24);

        // Seed a dummy offline catalog instead of syncing real data. Parsed like the
        // other boolean flags; main.rs gives it precedence over the sync settings.
        let seed_dummy_data = env_bool("SEED_DUMMY_DATA", false);

        // Skip on-disk image caching when a CDN fronts the origin (see the field docs).
        let cdn_mode = env_bool("CDN_MODE", false);

        // Optional static-SPA directory the API serves itself (the single-process
        // combined image); unset = the API serves only /api (see the field docs).
        let web_root = env_trimmed("WEB_ROOT").map(PathBuf::from);

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
            tcgcsv_user_agent,
            price_backfill_enabled,
            price_backfill_days,
            moxfield_user_agent,
            resend_api_key,
            email_from,
            turnstile_secret_key,
            trust_proxy_headers,
            rate_limit_enabled,
            redis_url,
            sync_on_startup,
            sync_interval_hours,
            seed_dummy_data,
            cdn_mode,
            web_root,
        }
    }

    /// The set of insecure-production-posture warnings that apply to this config.
    ///
    /// These are hardening gaps that are *fine* in local dev (which is why they are
    /// defaults) but risky on an internet-facing deploy, and nothing forces the
    /// operator to close them. Returned as a list (empty in a normal local-dev
    /// posture) so the triggered set is unit-testable without capturing log output;
    /// [`Self::warn_insecure_production_posture`] is the thin logging wrapper `main`
    /// calls. Each warning is advisory only — it never changes behaviour.
    fn production_posture_warnings(&self) -> Vec<&'static str> {
        if !looks_like_production(&self.host, &self.public_site_url) {
            return Vec::new();
        }

        let mut warnings = Vec::new();

        // Finding 1: an un-`Secure` refresh cookie rides plaintext HTTP.
        if !self.cookie_secure {
            warnings.push(
                "COOKIE_SECURE is false but this looks like a production deployment: the \
                 long-lived refresh-token cookie will be sent over plaintext HTTP and can be \
                 intercepted (session takeover). Set COOKIE_SECURE=true and serve the API over \
                 HTTPS.",
            );
        }

        // Finding 2: no CAPTCHA on the auth endpoints (per-IP limit only, no account
        // lockout) — distributed credential-stuffing across many IPs isn't capped.
        if self.rate_limit_enabled && self.turnstile_secret_key.is_none() {
            warnings.push(
                "TURNSTILE_SECRET_KEY is not set but this looks like a production deployment: the \
                 auth endpoints have no CAPTCHA, so login/registration are guarded only by the \
                 per-IP rate limit (distributed credential-stuffing across many IPs is not capped \
                 per account). Set TURNSTILE_SECRET_KEY (and the web VITE_TURNSTILE_SITE_KEY).",
            );
        }

        // Finding 3: the in-memory limiter's per-IP keyspace is only swept every 6h,
        // so a flood from many source IPs can grow memory unboundedly between sweeps.
        if self.rate_limit_enabled && self.redis_url.is_none() {
            warnings.push(
                "REDIS_URL is not set but this looks like a production deployment: the rate \
                 limiters run in-memory and their per-IP keyspace is only swept every 6 hours, so \
                 a flood from many source IPs can grow memory unboundedly between sweeps. Set \
                 REDIS_URL (its keys self-evict) for any internet-facing deploy, or front the API \
                 with a WAF.",
            );
        }

        warnings
    }

    /// Emit a loud startup warning for each insecure-production-posture gap (see
    /// [`Self::production_posture_warnings`]). No-op in a normal local-dev posture.
    pub fn warn_insecure_production_posture(&self) {
        for message in self.production_posture_warnings() {
            tracing::warn!("{message}");
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

    // ----- Insecure-production-posture startup warnings -----

    #[test]
    fn production_detection_treats_local_dev_as_non_production() {
        // The shipped dev defaults (loopback host, localhost site URL) are not prod.
        assert!(!looks_like_production("127.0.0.1", "http://localhost:5173"));
        // A container binding 0.0.0.0 with a still-local site URL isn't flagged.
        assert!(!looks_like_production("0.0.0.0", "http://localhost:5173"));
        assert!(!looks_like_production("::1", "http://127.0.0.1:8080"));
        // A real public site URL, or a concrete non-loopback bind host, is prod.
        assert!(looks_like_production("0.0.0.0", "https://tcglense.app"));
        assert!(looks_like_production("10.0.0.5", "http://localhost:5173"));
    }

    #[test]
    fn a_local_dev_posture_emits_no_posture_warnings() {
        // Dev defaults: insecure cookie / no CAPTCHA / no Redis are all fine locally,
        // so nothing is warned about (the warnings would just train operators to
        // ignore them).
        let config = Config {
            host: "127.0.0.1".to_string(),
            public_site_url: "http://localhost:5173".to_string(),
            cookie_secure: false,
            turnstile_secret_key: None,
            redis_url: None,
            rate_limit_enabled: true,
            ..crate::test_support::test_config()
        };
        assert!(config.production_posture_warnings().is_empty());
    }

    #[test]
    fn a_production_posture_with_every_gap_warns_about_all_three() {
        let config = Config {
            host: "0.0.0.0".to_string(),
            public_site_url: "https://tcglense.app".to_string(),
            cookie_secure: false,
            turnstile_secret_key: None,
            redis_url: None,
            rate_limit_enabled: true,
            ..crate::test_support::test_config()
        };
        let warnings = config.production_posture_warnings();
        assert_eq!(warnings.len(), 3, "cookie + captcha + redis: {warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("COOKIE_SECURE")));
        assert!(warnings.iter().any(|w| w.contains("TURNSTILE_SECRET_KEY")));
        assert!(warnings.iter().any(|w| w.contains("REDIS_URL")));
    }

    #[test]
    fn a_hardened_production_posture_emits_no_warnings() {
        let config = Config {
            host: "0.0.0.0".to_string(),
            public_site_url: "https://tcglense.app".to_string(),
            cookie_secure: true,
            turnstile_secret_key: Some("turnstile-secret".to_string()),
            redis_url: Some("redis://127.0.0.1:6379".to_string()),
            rate_limit_enabled: true,
            ..crate::test_support::test_config()
        };
        assert!(config.production_posture_warnings().is_empty());
    }

    #[test]
    fn disabling_rate_limiting_suppresses_the_captcha_and_redis_warnings() {
        // With RATE_LIMIT_ENABLED=false the operator has deferred to an upstream
        // WAF, so the CAPTCHA/Redis warnings (which are about that layer) don't
        // apply — but the cookie warning, which is transport-level, still does.
        let config = Config {
            host: "0.0.0.0".to_string(),
            public_site_url: "https://tcglense.app".to_string(),
            cookie_secure: false,
            turnstile_secret_key: None,
            redis_url: None,
            rate_limit_enabled: false,
            ..crate::test_support::test_config()
        };
        let warnings = config.production_posture_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("COOKIE_SECURE"));
    }
}

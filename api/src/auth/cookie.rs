use axum_extra::extract::cookie::{Cookie, SameSite};

use crate::config::Config;

/// Name of the httpOnly cookie carrying the opaque refresh token.
pub const REFRESH_COOKIE_NAME: &str = "tcglense_refresh";

/// Path the refresh cookie is scoped to. Restricting it to the auth routes means
/// the browser only attaches it to `/api/auth/*` requests (refresh / logout).
const REFRESH_COOKIE_PATH: &str = "/api/auth";

/// Build the `Set-Cookie` carrying a fresh refresh token.
///
/// HttpOnly + SameSite=Lax + a narrow Path mitigate XSS exfiltration and CSRF;
/// the `Secure` flag is driven by config so local http dev still works.
pub fn build_refresh_cookie(plaintext: String, config: &Config) -> Cookie<'static> {
    Cookie::build((REFRESH_COOKIE_NAME, plaintext))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path(REFRESH_COOKIE_PATH)
        .secure(config.cookie_secure)
        .max_age(time::Duration::days(config.refresh_token_expiry_days))
        .build()
}

/// A bare cookie (correct name + path) used with `CookieJar::remove` to emit a
/// removal `Set-Cookie` that clears `tcglense_refresh` on logout / failed refresh.
pub fn removal_cookie() -> Cookie<'static> {
    Cookie::build((REFRESH_COOKIE_NAME, ""))
        .path(REFRESH_COOKIE_PATH)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn config(cookie_secure: bool) -> Config {
        Config {
            database_url: "sqlite::memory:".to_string(),
            jwt_secret: "test-secret-key-for-cookie-unit-tests-0123".to_string(),
            access_token_expiry_minutes: 15,
            refresh_token_expiry_days: 30,
            cookie_secure,
            host: "127.0.0.1".to_string(),
            port: 8080,
            public_site_url: "http://localhost:5173".to_string(),
            data_dir: std::path::PathBuf::from("./data"),
            scryfall_user_agent: "TCGLense/test".to_string(),
            sync_on_startup: false,
            sync_interval_hours: 24,
            seed_dummy_data: false,
        }
    }

    #[test]
    fn refresh_cookie_is_hardened() {
        let cookie = build_refresh_cookie("opaque-refresh-token".to_string(), &config(true));

        assert_eq!(cookie.name(), REFRESH_COOKIE_NAME);
        assert_eq!(cookie.value(), "opaque-refresh-token");
        // HttpOnly: invisible to JS, so an XSS can't exfiltrate the refresh token.
        assert_eq!(cookie.http_only(), Some(true));
        // SameSite=Lax: mitigates CSRF on /refresh and /logout.
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
        // Narrow path: the browser only attaches it to /api/auth/* requests.
        assert_eq!(cookie.path(), Some(REFRESH_COOKIE_PATH));
        // Secure follows config; here it is on.
        assert_eq!(cookie.secure(), Some(true));
        // Bounded lifetime matching the refresh-token expiry.
        assert_eq!(cookie.max_age(), Some(time::Duration::days(30)));
    }

    #[test]
    fn refresh_cookie_secure_flag_follows_config() {
        // Local http dev (COOKIE_SECURE=false) must still be able to set the cookie.
        let cookie = build_refresh_cookie("t".to_string(), &config(false));
        assert_eq!(cookie.secure(), Some(false));
    }

    #[test]
    fn removal_cookie_targets_the_refresh_cookie_exactly() {
        // Must match name + path so the browser actually clears the live cookie.
        let cookie = removal_cookie();
        assert_eq!(cookie.name(), REFRESH_COOKIE_NAME);
        assert_eq!(cookie.value(), "");
        assert_eq!(cookie.path(), Some(REFRESH_COOKIE_PATH));
    }
}

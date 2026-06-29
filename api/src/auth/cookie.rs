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

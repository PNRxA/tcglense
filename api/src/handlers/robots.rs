//! `robots.txt`, served from the API so its `Sitemap:` line is an **absolute** URL built
//! from `PUBLIC_SITE_URL` at runtime — the same reason the sitemaps moved to the API
//! (issue #294). The web build previously emitted `robots.txt` at build time, but its
//! `Sitemap:` line was `VITE_SITE_URL`-relative and came out as a bare `Sitemap:
//! /sitemap.xml` whenever that build arg was unset (invalid per the sitemap protocol,
//! which requires an absolute URL). Served at the site root (`/robots.txt`); the split
//! Caddyfiles + the Vite dev/preview proxy forward it here, like the sitemap.

use axum::{
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};

use crate::state::AppState;

/// Paths kept out of the index: the auth + signed-in app pages (also `noindex` at
/// runtime, but declared here for non-JS crawlers) and the email-flow routes whose query
/// strings carry secret tokens. Mirrors the set the web build's `robots.txt` used.
const DISALLOW: &[&str] = &[
    "/login",
    "/register",
    "/profile",
    "/collection",
    "/wishlist",
    "/complete-registration",
    "/forgot-password",
    "/reset-password",
    "/verify-email",
];

/// `Cache-Control` for `robots.txt`: it changes only on redeploy, so let a shared cache
/// hold it (mirrors the sitemap's long TTL). Set by the handler so the public cache layer
/// preserves it.
pub const ROBOTS_CACHE_CONTROL: &str =
    "public, max-age=3600, s-maxage=86400, stale-while-revalidate=604800";

/// `GET /robots.txt` -> the crawl policy, with an absolute `Sitemap:` line against
/// [`crate::config::Config::public_site_url`] (trailing slash already trimmed).
pub async fn robots_txt(State(state): State<AppState>) -> Response {
    let base = &state.config.public_site_url;
    let mut body = String::from("User-agent: *\nAllow: /\n");
    for path in DISALLOW {
        body.push_str("Disallow: ");
        body.push_str(path);
        body.push('\n');
    }
    body.push_str(&format!("\nSitemap: {base}/sitemap.xml\n"));
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, ROBOTS_CACHE_CONTROL),
        ],
        body,
    )
        .into_response()
}

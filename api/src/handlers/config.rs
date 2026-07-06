use axum::{Json, extract::State, response::IntoResponse};
use serde::Serialize;

use crate::state::AppState;

/// Public, unauthenticated runtime configuration the SPA needs before it renders
/// the auth forms — chiefly the Cloudflare Turnstile *site* key (a public value,
/// safe to expose). Served at runtime so the published web bundle needs no rebuild
/// to point at a different Turnstile key: the key is set on the API
/// (`TURNSTILE_SITE_KEY`) and fetched by the SPA here, instead of being baked into
/// the bundle at build time as the old `VITE_TURNSTILE_SITE_KEY` arg was.
#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct PublicConfig {
    /// The Cloudflare Turnstile public site key, or `null` when CAPTCHA is disabled
    /// (no `TURNSTILE_SECRET_KEY`/`TURNSTILE_SITE_KEY` set) — the SPA then skips the
    /// widget. The API is the source of truth for whether a token is required.
    pub turnstile_site_key: Option<String>,
}

/// `GET /api/config` -> `200 { "turnstile_site_key": "..." | null }`.
///
/// Registered in the private (no-store) router group: the value only changes on a
/// redeploy and the payload is tiny, so there is nothing to cache. Panic-free.
pub async fn public_config(State(state): State<AppState>) -> impl IntoResponse {
    Json(PublicConfig {
        turnstile_site_key: state.config.turnstile_site_key.clone(),
    })
}

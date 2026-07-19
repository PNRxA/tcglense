//! Thin async HTTP client over the TCGLense JSON API.
//!
//! Injects the stored credential as `Authorization: Bearer <token>`, transparently
//! refreshes an expired session access token once on a `401` (re-presenting the
//! opaque `tcglense_refresh` cookie), persists any rotated tokens back to the config
//! file, and maps the API's `{ "error": string }` bodies to friendly errors.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use reqwest::{Method, StatusCode, header};
use serde::de::DeserializeOwned;
use tokio::sync::Mutex;

use crate::config::{Auth, Config};
use crate::models::AuthResponse;

const REFRESH_COOKIE: &str = "tcglense_refresh";

/// A request body.
pub enum Body {
    Empty,
    Json(serde_json::Value),
    Text(String, &'static str),
}

struct State {
    auth: Option<Auth>,
    /// Where to persist rotated session tokens; `None` for ephemeral credentials
    /// supplied on the command line (never written back).
    persist_to: Option<PathBuf>,
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    state: Arc<Mutex<State>>,
}

impl Client {
    pub fn new(
        base_url: String,
        auth: Option<Auth>,
        persist_to: Option<PathBuf>,
    ) -> Result<Client> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("tcglense-cli/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(15))
            .build()?;
        Ok(Client {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            state: Arc::new(Mutex::new(State { auth, persist_to })),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn current_auth(&self) -> Option<Auth> {
        self.state.lock().await.auth.clone()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn bearer(&self) -> Option<String> {
        self.state
            .lock()
            .await
            .auth
            .as_ref()
            .map(|a| a.bearer().to_string())
    }

    fn build(
        &self,
        method: &Method,
        path: &str,
        query: &[(&str, String)],
        body: &Body,
        bearer: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let mut req = self.http.request(method.clone(), self.url(path));
        if !query.is_empty() {
            req = req.query(query);
        }
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        match body {
            Body::Empty => req,
            Body::Json(v) => req.json(v),
            Body::Text(t, ct) => req.header(header::CONTENT_TYPE, *ct).body(t.clone()),
        }
    }

    /// Send a request, refreshing a stale session token once on `401`.
    async fn send(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Body,
    ) -> Result<reqwest::Response> {
        let bearer = self.bearer().await;
        let resp = self
            .build(&method, path, query, &body, bearer.as_deref())
            .send()
            .await
            .map_err(|e| transport_error(&self.base_url, e))?;

        if resp.status() != StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }
        // A 401 with a session credential: try one refresh, then replay.
        if !self.is_session().await {
            return Ok(resp);
        }
        if self.refresh().await.is_err() {
            return Ok(resp);
        }
        let bearer = self.bearer().await;
        let resp = self
            .build(&method, path, query, &body, bearer.as_deref())
            .send()
            .await
            .map_err(|e| transport_error(&self.base_url, e))?;
        Ok(resp)
    }

    async fn is_session(&self) -> bool {
        matches!(self.state.lock().await.auth, Some(Auth::Session { .. }))
    }

    /// Exchange the refresh cookie for a fresh access token, persisting the rotation.
    async fn refresh(&self) -> Result<()> {
        let refresh_token = {
            let guard = self.state.lock().await;
            match &guard.auth {
                Some(Auth::Session { refresh_token, .. }) => refresh_token.clone(),
                _ => bail!("no session to refresh"),
            }
        };
        let resp = self
            .http
            .post(self.url("/api/auth/refresh"))
            .header(header::COOKIE, format!("{REFRESH_COOKIE}={refresh_token}"))
            .send()
            .await
            .map_err(|e| transport_error(&self.base_url, e))?;
        if !resp.status().is_success() {
            bail!("session expired; run `tcglense login` again");
        }
        let new_refresh = extract_refresh_cookie(&resp).unwrap_or(refresh_token);
        let body: AuthResponse = resp.json().await?;
        let auth = Auth::Session {
            access_token: body.access_token,
            refresh_token: new_refresh,
            user: Some(body.user),
        };
        self.store_auth(Some(auth)).await?;
        Ok(())
    }

    /// Update the in-memory credential and persist it when a config path is set.
    async fn store_auth(&self, auth: Option<Auth>) -> Result<()> {
        let mut guard = self.state.lock().await;
        guard.auth = auth.clone();
        if let Some(path) = guard.persist_to.clone() {
            let mut cfg = Config::load(&path)?;
            cfg.auth = auth;
            cfg.save(&path)?;
        }
        Ok(())
    }

    // -- typed helpers -------------------------------------------------------

    pub async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let resp = self.send(Method::GET, path, query, Body::Empty).await?;
        decode_json(resp).await
    }

    pub async fn post_json<T: DeserializeOwned>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<T> {
        let resp = self.send(Method::POST, path, &[], Body::Json(body)).await?;
        decode_json(resp).await
    }

    pub async fn put_json<T: DeserializeOwned>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<T> {
        let resp = self.send(Method::PUT, path, &[], Body::Json(body)).await?;
        decode_json(resp).await
    }

    /// A PUT/POST whose success body we don't need (2xx incl. 204).
    pub async fn send_no_content(&self, method: Method, path: &str, body: Body) -> Result<()> {
        let resp = self.send(method, path, &[], body).await?;
        expect_success(resp).await?;
        Ok(())
    }

    pub async fn delete(&self, path: &str) -> Result<()> {
        self.send_no_content(Method::DELETE, path, Body::Empty)
            .await
    }

    /// GET a probe endpoint, returning `(status, body)` without treating a non-2xx
    /// (e.g. readiness `503`) as an error — the status *is* the information.
    pub async fn probe(&self, path: &str) -> Result<(u16, String)> {
        let resp = self.send(Method::GET, path, &[], Body::Empty).await?;
        let status = resp.status().as_u16();
        let text = resp.text().await?;
        Ok((status, text))
    }

    /// GET returning the raw response bytes (image proxy).
    pub async fn get_bytes(&self, path: &str, query: &[(&str, String)]) -> Result<Vec<u8>> {
        let resp = self.send(Method::GET, path, query, Body::Empty).await?;
        let resp = expect_success(resp).await?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// GET returning the raw response body as text (CSV export, OpenAPI document).
    pub async fn get_text(&self, path: &str, query: &[(&str, String)]) -> Result<String> {
        let resp = self.send(Method::GET, path, query, Body::Empty).await?;
        let resp = expect_success(resp).await?;
        Ok(resp.text().await?)
    }

    /// POST a raw text body (CSV upload) with query params, decoding a JSON reply.
    pub async fn post_text<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
        text: String,
        content_type: &'static str,
    ) -> Result<T> {
        let resp = self
            .send(Method::POST, path, query, Body::Text(text, content_type))
            .await?;
        decode_json(resp).await
    }

    // -- auth flows ----------------------------------------------------------

    /// Email + password login (the web auth path). Captures the refresh cookie,
    /// stores the session, and persists it.
    pub async fn login(
        &self,
        email: &str,
        password: &str,
        captcha: Option<&str>,
    ) -> Result<crate::models::User> {
        let mut body = serde_json::json!({ "email": email, "password": password });
        if let Some(c) = captcha {
            body["captcha_token"] = serde_json::Value::String(c.to_string());
        }
        let resp = self
            .send(Method::POST, "/api/auth/login", &[], Body::Json(body))
            .await?;
        self.consume_auth_response(resp).await
    }

    /// Finish an email-first registration with the completion token.
    pub async fn complete_registration(
        &self,
        token: &str,
        password: &str,
        username: Option<&str>,
        captcha: Option<&str>,
    ) -> Result<crate::models::User> {
        let mut body = serde_json::json!({ "token": token, "password": password });
        if let Some(u) = username {
            body["username"] = serde_json::Value::String(u.to_string());
        }
        if let Some(c) = captcha {
            body["captcha_token"] = serde_json::Value::String(c.to_string());
        }
        let resp = self
            .send(
                Method::POST,
                "/api/auth/complete-registration",
                &[],
                Body::Json(body),
            )
            .await?;
        self.consume_auth_response(resp).await
    }

    /// Shared handler for the two endpoints that return `{access_token, user}` +
    /// a refresh cookie and thereby start a session.
    async fn consume_auth_response(&self, resp: reqwest::Response) -> Result<crate::models::User> {
        let resp = expect_success(resp).await?;
        let refresh = extract_refresh_cookie(&resp)
            .ok_or_else(|| anyhow!("server did not return a refresh cookie"))?;
        let body: AuthResponse = resp.json().await?;
        let user = body.user.clone();
        self.store_auth(Some(Auth::Session {
            access_token: body.access_token,
            refresh_token: refresh,
            user: Some(body.user),
        }))
        .await?;
        Ok(user)
    }

    /// Store an API key credential (verifying it first via `/api/auth/me`).
    pub async fn set_api_key(&self, key: String, scope: Option<String>) -> Result<()> {
        self.store_auth(Some(Auth::ApiKey { key, scope })).await
    }

    /// Revoke the local session server-side (best effort) and clear the credential.
    pub async fn logout(&self) -> Result<()> {
        let session_refresh = {
            let guard = self.state.lock().await;
            match &guard.auth {
                Some(Auth::Session { refresh_token, .. }) => Some(refresh_token.clone()),
                _ => None,
            }
        };
        if let Some(refresh) = session_refresh {
            let _ = self
                .http
                .post(self.url("/api/auth/logout"))
                .header(header::COOKIE, format!("{REFRESH_COOKIE}={refresh}"))
                .send()
                .await;
        }
        self.store_auth(None).await
    }
}

async fn decode_json<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
    let resp = expect_success(resp).await?;
    let bytes = resp.bytes().await?;
    serde_json::from_slice(&bytes).map_err(|e| {
        let preview = String::from_utf8_lossy(&bytes);
        let preview: String = preview.chars().take(200).collect();
        anyhow!("could not decode API response: {e}; body was: {preview}")
    })
}

/// Turn a non-2xx response into a friendly error, parsing `{ "error": string }`.
async fn expect_success(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let retry_after = resp
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let body = resp.text().await.unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| {
            if body.trim().is_empty() {
                status
                    .canonical_reason()
                    .unwrap_or("request failed")
                    .to_string()
            } else {
                body.chars().take(300).collect()
            }
        });

    let hint = match status {
        StatusCode::UNAUTHORIZED => {
            " (not authenticated — run `tcglense login` or `tcglense auth key <tcgl_…>`)"
        }
        StatusCode::FORBIDDEN => {
            " (forbidden — the credential lacks the required scope; writes need a read_write key, and key management needs a session login)"
        }
        StatusCode::TOO_MANY_REQUESTS => match &retry_after {
            Some(s) => return Err(anyhow!("rate limited (429): {message}; retry after {s}s")),
            None => " (rate limited)",
        },
        _ => "",
    };
    Err(anyhow!("{} ({}){}", message, status.as_u16(), hint))
}

/// Pull the `tcglense_refresh` value out of the response's `Set-Cookie` headers.
fn extract_refresh_cookie(resp: &reqwest::Response) -> Option<String> {
    for hv in resp.headers().get_all(header::SET_COOKIE) {
        let Ok(s) = hv.to_str() else { continue };
        for part in s.split(';') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix(&format!("{REFRESH_COOKIE}="))
                && !v.is_empty()
            {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn transport_error(base_url: &str, e: reqwest::Error) -> anyhow::Error {
    if e.is_connect() || e.is_timeout() {
        anyhow!(
            "could not reach the API at {base_url}: {e}\nIs the server running? Set the URL with `tcglense --url <URL> …` or `tcglense config url <URL>`."
        )
    } else {
        anyhow!("request to {base_url} failed: {e}")
    }
}

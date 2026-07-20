//! Supported display currencies and the cached USD exchange-rate feed.
//!
//! Catalog prices and valuations stay canonical USD. The SPA converts those values for
//! display with this daily reference-rate snapshot, while a user's chosen ISO code lives
//! on their account. Keeping conversion at the display edge avoids changing price sorting,
//! bulk-threshold semantics, or the historic rows whenever rates move.

use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::{sync::Mutex, time::Instant};

use crate::error::AppError;

pub const DEFAULT_CURRENCY: &str = "USD";

/// A deliberately small set of currencies covering TCGLense's main English-speaking and
/// European audiences. Every non-USD entry is available from Frankfurter's daily feed.
pub const SUPPORTED_CURRENCIES: &[&str] = &["USD", "AUD", "CAD", "EUR", "GBP", "JPY", "NZD"];

/// The upstream daily FX feed (Frankfurter). `pub(crate)` so the dataset-mirror origin can
/// re-serve it verbatim at `/api/mirror/currency`
/// ([`crate::handlers::mirror::currency_proxy`]), letting mirror consumers pull rates from the
/// main server instead of contacting the provider — the same posture as the card datasets.
pub(crate) const RATES_URL: &str =
    "https://api.frankfurter.dev/v2/rates?base=USD&quotes=AUD,CAD,EUR,GBP,JPY,NZD";
const REFRESH_AFTER: Duration = Duration::from_secs(12 * 60 * 60);
// Daily feeds legitimately pause for weekends and holidays, but an indefinitely stale
// conversion is worse than an explicitly-labelled USD fallback. Seven days comfortably
// covers those gaps while bounding how old a displayed conversion can become.
const MAX_STALE_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const RETRY_AFTER_ERROR: Duration = Duration::from_secs(5 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub fn is_supported(code: &str) -> bool {
    SUPPORTED_CURRENCIES.contains(&code)
}

pub fn validate(code: &str) -> Result<&str, AppError> {
    let code = code.trim();
    if is_supported(code) {
        Ok(code)
    } else {
        Err(AppError::Validation(format!(
            "currency must be one of {}",
            SUPPORTED_CURRENCIES.join(", ")
        )))
    }
}

/// The browser-facing rate snapshot. `rates` always includes `USD: 1` and one positive,
/// finite USD conversion rate for every other supported currency.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct CurrencyRatesResponse {
    pub base: String,
    pub as_of: String,
    pub rates: BTreeMap<String, f64>,
}

#[derive(Debug, Deserialize)]
struct ProviderRate {
    date: String,
    base: String,
    quote: String,
    rate: f64,
}

struct CachedRates {
    fetched_at: Instant,
    snapshot: CurrencyRatesResponse,
}

#[derive(Default)]
struct RatesState {
    cache: Option<CachedRates>,
    failed_at: Option<Instant>,
    refreshing: bool,
}

/// Single-flight, bounded-stale cache for the daily upstream feed. A cold/too-old cache
/// waits for one guarded upstream request; an expired-but-usable snapshot is returned
/// immediately while one background refresh runs.
pub struct CurrencyRates {
    state: Mutex<RatesState>,
    rates_url: String,
}

impl Default for CurrencyRates {
    fn default() -> Self {
        Self {
            state: Mutex::new(RatesState::default()),
            rates_url: RATES_URL.to_string(),
        }
    }
}

impl CurrencyRates {
    /// Build the rate cache from application config, choosing **where** the daily FX feed is
    /// fetched.
    ///
    /// A default self-host (a dataset-mirror *consumer*) pulls the rates from the TCGLense
    /// mirror at `{DATASET_MIRROR_URL}/api/mirror/currency` — so, exactly like the card
    /// datasets (see [`crate::datasets`]), it never contacts the upstream FX provider directly.
    /// The mirror **origin** (`MIRROR_ENABLED`) and any instance run with
    /// `SYNC_FROM_UPSTREAM=true` fetch the feed straight from the provider ([`RATES_URL`]); the
    /// origin then re-serves it to consumers via its `/api/mirror/currency` route. The
    /// stale/fallback behaviour is unchanged either way: an unreachable source falls open to a
    /// labelled USD conversion.
    pub fn from_config(config: &crate::config::Config) -> Self {
        // The origin is the source of truth (it re-serves what it fetches), and an explicit
        // upstream posture bypasses the mirror — both go straight to the provider. Everyone
        // else reads the feed from the mirror, like every other dataset.
        let rates_url = if config.sync_from_upstream || config.mirror_enabled {
            RATES_URL.to_string()
        } else {
            // Must match the `/api/mirror/currency` route registered in [`crate::router`] (and
            // the origin's [`crate::handlers::mirror::currency_proxy`]). Trim a trailing slash
            // defensively — like the sibling [`crate::datasets::SyncSource::new`] — so the join
            // stays well-formed even though `Config` already trims the stored value.
            format!(
                "{}/api/mirror/currency",
                config.dataset_mirror_url.trim_end_matches('/')
            )
        };
        Self {
            state: Mutex::new(RatesState::default()),
            rates_url,
        }
    }

    /// Test-only accessor for the resolved feed URL, so a test outside this module (the
    /// `AppState::new` wiring test) can assert where a consumer/origin points without
    /// exposing the field in production code.
    #[cfg(test)]
    pub(crate) fn rates_url(&self) -> &str {
        &self.rates_url
    }

    #[cfg(test)]
    fn with_url(rates_url: String) -> Self {
        Self {
            state: Mutex::new(RatesState::default()),
            rates_url,
        }
    }

    pub async fn latest(
        self: &std::sync::Arc<Self>,
        client: &reqwest::Client,
    ) -> Result<CurrencyRatesResponse, AppError> {
        let mut state = self.state.lock().await;
        if let Some(cached) = state.cache.as_ref()
            && cached.fetched_at.elapsed() < REFRESH_AFTER
        {
            return Ok(cached.snapshot.clone());
        }

        let stale = state
            .cache
            .as_ref()
            .filter(|cached| cached.fetched_at.elapsed() <= MAX_STALE_AGE)
            .map(|cached| cached.snapshot.clone());

        // Stale-while-revalidate: an otherwise usable request must not inherit the
        // provider's latency. One caller marks the refresh in flight and spawns it; every
        // caller (including that first one) receives the last-good snapshot immediately.
        if let Some(snapshot) = stale {
            let in_backoff = state
                .failed_at
                .is_some_and(|failed_at| failed_at.elapsed() < RETRY_AFTER_ERROR);
            if !in_backoff && !state.refreshing {
                state.refreshing = true;
                let rates = std::sync::Arc::clone(self);
                let client = client.clone();
                tokio::spawn(async move { rates.refresh(client).await });
            }
            return Ok(snapshot);
        }

        // No usable snapshot exists. Back off a failed cold start/too-old refresh so a
        // burst of requests cannot take turns hammering an unavailable provider.
        if state
            .failed_at
            .is_some_and(|failed_at| failed_at.elapsed() < RETRY_AFTER_ERROR)
        {
            return Err(unavailable());
        }

        // Hold the lock across a cold fetch deliberately. Followers have no safe stale
        // value to receive, so they wait for and reuse this one single-flight result.
        match fetch_rates(client, &self.rates_url).await {
            Ok(snapshot) => {
                state.cache = Some(CachedRates {
                    fetched_at: Instant::now(),
                    snapshot: snapshot.clone(),
                });
                state.failed_at = None;
                Ok(snapshot)
            }
            Err(err) => {
                state.failed_at = Some(Instant::now());
                // A previously-good snapshot is safer than making every monetary display
                // fall back to USD during a temporary provider outage.
                if let Some(cached) = state
                    .cache
                    .as_ref()
                    .filter(|cached| cached.fetched_at.elapsed() <= MAX_STALE_AGE)
                {
                    tracing::warn!(error = %err, "currency-rate refresh failed; serving stale rates");
                    Ok(cached.snapshot.clone())
                } else {
                    tracing::warn!(error = %err, "currency-rate fetch failed");
                    Err(unavailable())
                }
            }
        }
    }

    async fn refresh(self: std::sync::Arc<Self>, client: reqwest::Client) {
        let result = fetch_rates(&client, &self.rates_url).await;
        let mut state = self.state.lock().await;
        state.refreshing = false;
        match result {
            Ok(snapshot) => {
                state.cache = Some(CachedRates {
                    fetched_at: Instant::now(),
                    snapshot,
                });
                state.failed_at = None;
            }
            Err(err) => {
                state.failed_at = Some(Instant::now());
                tracing::warn!(error = %err, "currency-rate refresh failed; retaining bounded stale snapshot");
            }
        }
    }
}

fn unavailable() -> AppError {
    AppError::BadGateway("currency conversion is temporarily unavailable".to_string())
}

async fn fetch_rates(
    client: &reqwest::Client,
    rates_url: &str,
) -> Result<CurrencyRatesResponse, String> {
    let rows = client
        .get(rates_url)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?
        .json::<Vec<ProviderRate>>()
        .await
        .map_err(|err| err.to_string())?;

    parse_rates(rows)
}

fn parse_rates(rows: Vec<ProviderRate>) -> Result<CurrencyRatesResponse, String> {
    let mut rates = BTreeMap::from([(DEFAULT_CURRENCY.to_string(), 1.0)]);
    let mut as_of = String::new();

    for row in rows {
        if row.base != DEFAULT_CURRENCY
            || !is_supported(&row.quote)
            || row.quote == DEFAULT_CURRENCY
        {
            continue;
        }
        if !row.rate.is_finite() || row.rate <= 0.0 {
            return Err(format!("invalid {} rate", row.quote));
        }
        as_of = as_of.max(row.date);
        rates.insert(row.quote, row.rate);
    }

    if as_of.is_empty() {
        return Err("missing rate date".to_string());
    }
    for code in SUPPORTED_CURRENCIES {
        if !rates.contains_key(*code) {
            return Err(format!("missing {code} rate"));
        }
    }

    Ok(CurrencyRatesResponse {
        base: DEFAULT_CURRENCY.to_string(),
        as_of,
        rates,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicU16, AtomicU64, Ordering},
        },
        time::Duration,
    };

    use axum::{Router, http::StatusCode};
    use reqwest::Client;
    use tokio::time::{sleep, timeout};

    use super::*;

    struct ProviderControl {
        hits: AtomicU64,
        status: AtomicU16,
        delay_ms: AtomicU64,
    }

    async fn spawn_provider() -> (String, Arc<ProviderControl>) {
        let control = Arc::new(ProviderControl {
            hits: AtomicU64::new(0),
            status: AtomicU16::new(200),
            delay_ms: AtomicU64::new(0),
        });
        let handler_control = control.clone();
        let body = serde_json::json!([
            { "date": "2026-07-15", "base": "USD", "quote": "AUD", "rate": 1.52 },
            { "date": "2026-07-15", "base": "USD", "quote": "CAD", "rate": 1.37 },
            { "date": "2026-07-15", "base": "USD", "quote": "EUR", "rate": 0.86 },
            { "date": "2026-07-15", "base": "USD", "quote": "GBP", "rate": 0.75 },
            { "date": "2026-07-15", "base": "USD", "quote": "JPY", "rate": 158.4 },
            { "date": "2026-07-15", "base": "USD", "quote": "NZD", "rate": 1.66 }
        ])
        .to_string();
        let app = Router::new().fallback(move || {
            let control = handler_control.clone();
            let body = body.clone();
            async move {
                control.hits.fetch_add(1, Ordering::SeqCst);
                let delay = control.delay_ms.load(Ordering::SeqCst);
                if delay > 0 {
                    sleep(Duration::from_millis(delay)).await;
                }
                let status = StatusCode::from_u16(control.status.load(Ordering::SeqCst))
                    .expect("valid provider status");
                (status, [("content-type", "application/json")], body)
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rate provider");
        let addr = listener.local_addr().expect("rate provider address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}/rates"), control)
    }

    async fn wait_for_hits(control: &ProviderControl, expected: u64) {
        timeout(Duration::from_secs(1), async {
            while control.hits.load(Ordering::SeqCst) < expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("provider hit did not arrive");
    }

    async fn wait_for_refresh(rates: &CurrencyRates) {
        timeout(Duration::from_secs(1), async {
            loop {
                if !rates.state.lock().await.refreshing {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("background refresh did not finish");
    }

    async fn age_cache(rates: &CurrencyRates, age: Duration) {
        let mut state = rates.state.lock().await;
        state.cache.as_mut().expect("primed cache").fetched_at = Instant::now() - age;
    }

    fn row(quote: &str, rate: f64) -> ProviderRate {
        ProviderRate {
            date: "2026-07-14".to_string(),
            base: "USD".to_string(),
            quote: quote.to_string(),
            rate,
        }
    }

    #[test]
    fn from_config_points_a_consumer_at_the_mirror_and_the_origin_upstream() {
        use crate::config::Config;
        // Default self-host (a dataset-mirror consumer): rates are pulled from the mirror,
        // never the upstream FX provider directly.
        let consumer = CurrencyRates::from_config(&Config {
            sync_from_upstream: false,
            mirror_enabled: false,
            dataset_mirror_url: "https://mirror.example".to_string(),
            ..crate::test_support::test_config()
        });
        assert_eq!(
            consumer.rates_url,
            "https://mirror.example/api/mirror/currency"
        );
        // A trailing slash on the mirror base is trimmed, so the join never doubles up.
        let trailing = CurrencyRates::from_config(&Config {
            sync_from_upstream: false,
            mirror_enabled: false,
            dataset_mirror_url: "https://mirror.example/".to_string(),
            ..crate::test_support::test_config()
        });
        assert_eq!(
            trailing.rates_url,
            "https://mirror.example/api/mirror/currency"
        );
        // Explicit upstream posture: straight to the provider.
        let upstream = CurrencyRates::from_config(&Config {
            sync_from_upstream: true,
            ..crate::test_support::test_config()
        });
        assert_eq!(upstream.rates_url, RATES_URL);
        // The mirror origin is the source of truth (it re-serves what it fetches), so it also
        // fetches straight from the provider.
        let origin = CurrencyRates::from_config(&Config {
            mirror_enabled: true,
            ..crate::test_support::test_config()
        });
        assert_eq!(origin.rates_url, RATES_URL);
    }

    #[test]
    fn validates_supported_currency_codes_exactly() {
        assert_eq!(validate("AUD").unwrap(), "AUD");
        assert!(validate("aud").is_err());
        assert!(validate("BTC").is_err());
    }

    #[test]
    fn parses_a_complete_positive_snapshot() {
        let snapshot = parse_rates(vec![
            row("AUD", 1.52),
            row("CAD", 1.37),
            row("EUR", 0.86),
            row("GBP", 0.75),
            row("JPY", 158.4),
            row("NZD", 1.66),
        ])
        .unwrap();

        assert_eq!(snapshot.base, "USD");
        assert_eq!(snapshot.as_of, "2026-07-14");
        assert_eq!(snapshot.rates["USD"], 1.0);
        assert_eq!(snapshot.rates["AUD"], 1.52);
    }

    #[test]
    fn rejects_missing_or_invalid_supported_rates() {
        assert!(parse_rates(vec![row("AUD", 1.52)]).is_err());
        assert!(
            parse_rates(vec![
                row("AUD", f64::NAN),
                row("CAD", 1.37),
                row("EUR", 0.86),
                row("GBP", 0.75),
                row("JPY", 158.4),
                row("NZD", 1.66),
            ])
            .is_err()
        );
    }

    #[tokio::test]
    async fn cold_fetch_is_singleflight_and_fresh_cache_is_reused() {
        let (url, control) = spawn_provider().await;
        control.delay_ms.store(25, Ordering::SeqCst);
        let rates = Arc::new(CurrencyRates::with_url(url));
        let client = Client::new();

        let mut calls = Vec::new();
        for _ in 0..8 {
            let rates = rates.clone();
            let client = client.clone();
            calls.push(tokio::spawn(
                async move { rates.latest(&client).await.unwrap() },
            ));
        }
        for call in calls {
            assert_eq!(call.await.unwrap().rates["AUD"], 1.52);
        }
        assert_eq!(control.hits.load(Ordering::SeqCst), 1);

        assert_eq!(rates.latest(&client).await.unwrap().rates["AUD"], 1.52);
        assert_eq!(control.hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn expired_cache_returns_stale_immediately_and_refreshes_once() {
        let (url, control) = spawn_provider().await;
        let rates = Arc::new(CurrencyRates::with_url(url));
        let client = Client::new();
        rates.latest(&client).await.unwrap();
        age_cache(&rates, REFRESH_AFTER + Duration::from_secs(1)).await;
        control.delay_ms.store(250, Ordering::SeqCst);

        let mut calls = Vec::new();
        for _ in 0..8 {
            let rates = rates.clone();
            let client = client.clone();
            calls.push(tokio::spawn(
                async move { rates.latest(&client).await.unwrap() },
            ));
        }
        timeout(Duration::from_millis(100), async {
            for call in calls {
                assert_eq!(call.await.unwrap().rates["AUD"], 1.52);
            }
        })
        .await
        .expect("stale readers waited for the upstream refresh");

        wait_for_hits(&control, 2).await;
        wait_for_refresh(&rates).await;
        assert_eq!(control.hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failed_refresh_backs_off_and_never_serves_beyond_the_stale_limit() {
        let (url, control) = spawn_provider().await;
        let rates = Arc::new(CurrencyRates::with_url(url));
        let client = Client::new();
        rates.latest(&client).await.unwrap();

        control.status.store(500, Ordering::SeqCst);
        age_cache(&rates, REFRESH_AFTER + Duration::from_secs(1)).await;
        assert_eq!(rates.latest(&client).await.unwrap().rates["AUD"], 1.52);
        wait_for_hits(&control, 2).await;
        wait_for_refresh(&rates).await;

        // The five-minute error backoff reuses a still-bounded stale snapshot without a
        // third provider call.
        assert_eq!(rates.latest(&client).await.unwrap().rates["AUD"], 1.52);
        tokio::task::yield_now().await;
        assert_eq!(control.hits.load(Ordering::SeqCst), 2);

        // Once the snapshot is too old, even the backoff path must refuse it and let the
        // browser render its explicit USD fallback.
        age_cache(&rates, MAX_STALE_AGE + Duration::from_secs(1)).await;
        assert!(matches!(
            rates.latest(&client).await,
            Err(AppError::BadGateway(_))
        ));
        assert_eq!(control.hits.load(Ordering::SeqCst), 2);

        // After the backoff expires, a too-old cache is treated like a cold cache: retry
        // once, then remain unavailable rather than leaking the expired conversion.
        rates.state.lock().await.failed_at = Some(Instant::now() - RETRY_AFTER_ERROR);
        assert!(matches!(
            rates.latest(&client).await,
            Err(AppError::BadGateway(_))
        ));
        assert_eq!(control.hits.load(Ordering::SeqCst), 3);
    }
}

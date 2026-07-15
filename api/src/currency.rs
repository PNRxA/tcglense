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

const RATES_URL: &str =
    "https://api.frankfurter.dev/v2/rates?base=USD&quotes=AUD,CAD,EUR,GBP,JPY,NZD";
const REFRESH_AFTER: Duration = Duration::from_secs(12 * 60 * 60);
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
}

/// Single-flight, stale-on-error cache for the daily upstream feed. The mutex is held across
/// the cold fetch on purpose: at most one request reaches the provider when a process starts
/// or the snapshot expires; followers wait and reuse that result.
#[derive(Default)]
pub struct CurrencyRates {
    state: Mutex<RatesState>,
}

impl CurrencyRates {
    pub async fn latest(
        &self,
        client: &reqwest::Client,
    ) -> Result<CurrencyRatesResponse, AppError> {
        let mut state = self.state.lock().await;
        if let Some(cached) = state.cache.as_ref()
            && cached.fetched_at.elapsed() < REFRESH_AFTER
        {
            return Ok(cached.snapshot.clone());
        }

        // Back off a failed cold start/refresh. Without this, every queued request would
        // take its own turn retrying the unavailable provider as soon as the mutex opened.
        if let Some(failed_at) = state.failed_at
            && failed_at.elapsed() < RETRY_AFTER_ERROR
        {
            return state.cache.as_ref().map_or_else(
                || {
                    Err(AppError::BadGateway(
                        "currency conversion is temporarily unavailable".to_string(),
                    ))
                },
                |cached| Ok(cached.snapshot.clone()),
            );
        }

        match fetch_rates(client).await {
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
                if let Some(cached) = state.cache.as_ref() {
                    tracing::warn!(error = %err, "currency-rate refresh failed; serving stale rates");
                    Ok(cached.snapshot.clone())
                } else {
                    tracing::warn!(error = %err, "currency-rate fetch failed");
                    Err(AppError::BadGateway(
                        "currency conversion is temporarily unavailable".to_string(),
                    ))
                }
            }
        }
    }
}

async fn fetch_rates(client: &reqwest::Client) -> Result<CurrencyRatesResponse, String> {
    let rows = client
        .get(RATES_URL)
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
    use super::*;

    fn row(quote: &str, rate: f64) -> ProviderRate {
        ProviderRate {
            date: "2026-07-14".to_string(),
            base: "USD".to_string(),
            quote: quote.to_string(),
            rate,
        }
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
}

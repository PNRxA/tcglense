use axum::{Json, extract::State};

use crate::{currency::CurrencyRatesResponse, error::AppError, state::AppState};

/// `GET /api/currencies` -> the cached latest USD reference rates used by the SPA for
/// display-only conversion. Catalog values remain USD on the wire and in storage.
pub async fn currency_rates(
    State(state): State<AppState>,
) -> Result<Json<CurrencyRatesResponse>, AppError> {
    Ok(Json(state.currency_rates.latest(&state.http).await?))
}

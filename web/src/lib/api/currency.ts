import { request } from './client'
import type { CurrencyRatesResponse } from './generated'

export type { CurrencyRatesResponse } from './generated'

/** Latest cached reference rates, expressed as units of each currency per USD. */
export function getCurrencyRates(): Promise<CurrencyRatesResponse> {
  return request<CurrencyRatesResponse>('/api/currencies')
}

import type { AlertFinish, AlertTargetKind } from '@/lib/api/alerts'

// The price-alert finish vocabulary (issue #525, etched in #676) — the client mirror of the
// API's `handlers::alerts::validate_finish`, whose test pins the same table as the spec here.
// A card can be priced in three finishes (Scryfall's `usd` / `usd_foil` / `usd_etched`), a
// sealed product in two (TCGCSV is USD-only and has no etched price), and an alert watches
// exactly one column: an `etched` alert on a card with no etched price is *unpriced*, never
// priced at its foil. The detail pages therefore offer each finish only when the target is
// priced in it, so the picker can't arm an alert the evaluator would never be able to judge.

/** Every finish the API accepts, in the order the pickers list them. */
export const ALERT_FINISHES: readonly AlertFinish[] = ['nonfoil', 'foil', 'etched']

/** The finishes an alert may name per target kind (the server's `validate_finish`). */
export const ALERT_FINISHES_BY_KIND: Record<AlertTargetKind, readonly AlertFinish[]> = {
  card: ['nonfoil', 'foil', 'etched'],
  product: ['nonfoil', 'foil'],
}

/** Display label per finish (the alert dialog's picker and the alert list share it). */
export const ALERT_FINISH_LABELS: Record<AlertFinish, string> = {
  nonfoil: 'Regular',
  foil: 'Foil',
  etched: 'Etched',
}

/** The USD price fields a target carries, one per finish it can be priced in. `usd_etched`
 * is optional so a sealed product's `ProductPrices` (regular + foil only) fits too. */
export interface FinishPrices {
  usd: string | null
  usd_foil: string | null
  usd_etched?: string | null
}

/**
 * The finishes a card is actually priced in, so the alert dialog offers only those — a
 * regular-only card shows no finish picker, an etched printing offers its third finish. A
 * fully unpriced card falls back to `nonfoil` so an alert can still be armed for when a price
 * arrives (the evaluator skips it until then).
 */
export function cardAlertFinishes(prices: FinishPrices | null | undefined): AlertFinish[] {
  const finishes: AlertFinish[] = []
  if (prices?.usd != null) finishes.push('nonfoil')
  if (prices?.usd_foil != null) finishes.push('foil')
  if (prices?.usd_etched != null) finishes.push('etched')
  return finishes.length ? finishes : ['nonfoil']
}

/**
 * The single finish a sealed product's alert watches. Products are finish-less in practice
 * (TCGCSV is effectively single-price and the product chart is single-series), so the dialog
 * shows no picker: prefer the regular column, and use foil only when it's the sole priced one.
 */
export function productAlertFinishes(prices: FinishPrices | null | undefined): AlertFinish[] {
  if (prices?.usd == null && prices?.usd_foil != null) return ['foil']
  return ['nonfoil']
}

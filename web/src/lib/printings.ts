import type { Card } from '@/lib/api'

/** The set/collector/rarity metadata shared by visual printing tiles and the scanner's
 * compact selector. Set name stays separate because both surfaces give it more prominence. */
export function printingMetadataLabel(card: Card): string {
  const parts = [`${card.set_code.toUpperCase()} · #${card.collector_number}`]
  if (card.rarity) parts.push(card.rarity)
  return parts.join(' · ')
}

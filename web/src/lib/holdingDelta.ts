import type { CollectionQuantities } from '@/lib/api'

// Writes to a card holding are ABSOLUTE (see `useOwnedCountEditor`), so every surface that
// wants to say "what did this change actually do" has to subtract two count pairs itself.
// The scanner needs that answer twice over — the match panel, to make the finish the scanned
// copy landed on obvious *before* it commits, and the session log, to say what each committed
// row added — and the two must word it identically or the panel and the history disagree about
// the same card. So the subtraction, the wording, and the "which finish is this about" question
// live here once.

/** The regular/foil change between two absolute holdings — the same field names the holding
 * carries, so a caller can index it with the key of the row it's rendering. */
export interface HoldingDelta {
  quantity: number
  foil_quantity: number
}

/** What `to` changed relative to `from` (negative where a count went down). */
export function holdingDelta(from: CollectionQuantities, to: CollectionQuantities): HoldingDelta {
  return {
    quantity: to.quantity - from.quantity,
    foil_quantity: to.foil_quantity - from.foil_quantity,
  }
}

/** Whether the delta is about foil copies — true only when the foil count moved and the
 * regular one didn't. Drives which USD price a surface shows for the change: a foil-only add
 * is worth the foil price, and anything touching the regular count is not. */
export function holdingDeltaIsFoil(delta: HoldingDelta): boolean {
  return delta.foil_quantity !== 0 && delta.quantity === 0
}

function signed(value: number): string {
  return value > 0 ? `+${value}` : String(value)
}

/** A compact chip label for the change: `+1 foil`, `+2 regular`, `+1 regular, +1 foil`, or
 * null when nothing moved (there is no chip to draw then). */
export function holdingDeltaLabel(delta: HoldingDelta): string | null {
  const parts: string[] = []
  if (delta.quantity !== 0) parts.push(`${signed(delta.quantity)} regular`)
  if (delta.foil_quantity !== 0) parts.push(`${signed(delta.foil_quantity)} foil`)
  return parts.length ? parts.join(', ') : null
}

function copies(count: number): string {
  return count === 1 ? 'copy' : 'copies'
}

/** One "<verb> 1 regular copy and 2 foil copies" clause, for the counts moving in one
 * direction. Each finish carries its own noun so the plural always agrees with the number
 * beside it. */
function clause(verb: string, regular: number, foil: number): string {
  const parts: string[] = []
  if (regular > 0) parts.push(`${regular} regular ${copies(regular)}`)
  if (foil > 0) parts.push(`${foil} foil ${copies(foil)}`)
  return `${verb} ${parts.join(' and ')}`
}

/** The same change as a sentence, for the tentative match panel: `Adding 1 foil copy`,
 * `Removing 2 regular copies`, `Adding 1 regular and 1 foil copy`. Null when nothing moved.
 * Mixed signs (one finish up, the other down) are spelled out as two clauses so a correction
 * that moves the scanned copy from regular to foil reads as exactly that. */
export function holdingDeltaSummary(delta: HoldingDelta): string | null {
  const clauses: string[] = []
  if (delta.quantity > 0 || delta.foil_quantity > 0) {
    clauses.push(clause('Adding', Math.max(delta.quantity, 0), Math.max(delta.foil_quantity, 0)))
  }
  if (delta.quantity < 0 || delta.foil_quantity < 0) {
    clauses.push(
      clause(
        clauses.length ? 'removing' : 'Removing',
        Math.max(-delta.quantity, 0),
        Math.max(-delta.foil_quantity, 0),
      ),
    )
  }
  return clauses.length ? clauses.join(', ') : null
}

// Turning the `{…}` symbols Scryfall stores in card text (`{W}`, `{2}`, `{T}`,
// `{W/U}`, `{G/U/P}`, …) into icons. We map each symbol to a class from the bundled
// `mana-font` icon font (Andrew Gioia, MIT — imported once in `main.ts`) rather than
// hotlinking Scryfall's symbol SVGs, keeping the app self-contained/offline.
//
// A `{…}` token we don't recognise is left as its literal text, so unusual tokens
// (or a future Scryfall symbol) degrade gracefully instead of vanishing.

/** A run of plain text between symbols. */
export interface TextToken {
  type: 'text'
  value: string
}

/** One `{…}` symbol resolved to a mana-font class + a screen-reader label. */
export interface SymbolToken {
  type: 'symbol'
  /** The mana-font glyph class, e.g. `ms-w`, `ms-tap`, `ms-2w`. */
  className: string
  /** Human-readable label for `aria-label`/`title`, e.g. "White mana", "Tap". */
  label: string
}

export type ManaToken = TextToken | SymbolToken

// The mana-cost / oracle-text subset of mana-font's icon set we render. Values are
// the final `ms-<suffix>` suffixes; anything not here stays literal text.
const NUMERIC = Array.from({ length: 21 }, (_, i) => String(i)) // {0}…{20}
const KNOWN: ReadonlySet<string> = new Set<string>([
  // colours, colourless, and the standalone symbols
  'w',
  'u',
  'b',
  'r',
  'g',
  'c',
  's',
  'e',
  'p',
  'x',
  'y',
  'z',
  'tap',
  'untap',
  'chaos',
  'acorn',
  'tk',
  'planeswalker',
  'half',
  'infinity',
  // generic (numeric) mana
  ...NUMERIC,
  '100',
  '1000000',
  // two-colour hybrid
  'wu',
  'wb',
  'ub',
  'ur',
  'br',
  'bg',
  'rg',
  'rw',
  'gw',
  'gu',
  // colourless hybrid
  'cw',
  'cu',
  'cb',
  'cr',
  'cg',
  // "twobrid" (2-generic-or-a-colour)
  '2w',
  '2u',
  '2b',
  '2r',
  '2g',
  // Phyrexian
  'wp',
  'up',
  'bp',
  'rp',
  'gp',
  // hybrid Phyrexian
  'wup',
  'wbp',
  'ubp',
  'urp',
  'brp',
  'bgp',
  'rgp',
  'rwp',
  'gwp',
  'gup',
])

// Tokens whose mana-font suffix differs from their normalised Scryfall code.
const ALIASES: Readonly<Record<string, string>> = {
  t: 'tap',
  q: 'untap',
  a: 'acorn',
  pw: 'planeswalker',
  '½': 'half',
  '∞': 'infinity',
}

const COLOR_NAMES: Readonly<Record<string, string>> = {
  w: 'White',
  u: 'Blue',
  b: 'Black',
  r: 'Red',
  g: 'Green',
  c: 'Colorless',
  s: 'Snow',
}

/** Normalise a raw symbol body (`W/U`, `T`, `2`) to a mana-font suffix, or null when
 *  it isn't a symbol we render. */
function toSuffix(body: string): string | null {
  const code = body.toLowerCase().replace(/\//g, '')
  const suffix = ALIASES[code] ?? code
  return KNOWN.has(suffix) ? suffix : null
}

// Labels for the standalone symbols that aren't a single colour or a number. Note
// several of these aren't mana at all (tap, chaos die, acorn stamp, ticket), so they
// must not be labelled "… mana".
const SPECIAL_LABELS: Readonly<Record<string, string>> = {
  tap: 'Tap',
  untap: 'Untap',
  chaos: 'Chaos',
  acorn: 'Acorn',
  tk: 'Ticket',
  planeswalker: 'Planeswalker',
  e: 'Energy',
  p: 'Phyrexian mana',
  half: 'Half mana',
  infinity: 'Infinity mana',
}

/** A readable label for accessibility, e.g. `{W}` → "White mana", `{T}` → "Tap",
 *  `{W/U}` → "White/Blue hybrid mana". */
function labelFor(body: string, suffix: string): string {
  const special = SPECIAL_LABELS[suffix]
  if (special) return special
  if (suffix === 'x' || suffix === 'y' || suffix === 'z')
    return `${suffix.toUpperCase()} generic mana`
  if (/^\d+$/.test(suffix)) return `${suffix} generic mana`
  const color = COLOR_NAMES[suffix]
  if (color) return `${color} mana`
  // hybrid / twobrid / Phyrexian ({W/U}, {2/W}, {W/P}, {G/U/P}): name each part.
  const parts = body.split('/').map((part) => {
    const key = part.toLowerCase()
    if (key === 'p') return 'Phyrexian'
    return COLOR_NAMES[key] ?? part
  })
  return `${parts.join('/')} hybrid mana`
}

const SYMBOL_RE = /\{([^{}]+)\}/g

/** Split card text into plain-text runs and recognised symbols. An unrecognised
 *  `{…}` token stays embedded in the surrounding text so nothing is lost. */
export function parseManaText(text: string): ManaToken[] {
  const tokens: ManaToken[] = []
  let last = 0
  for (const match of text.matchAll(SYMBOL_RE)) {
    const raw = match[0]
    const body = match[1] ?? '' // capture group is required, so always present
    const suffix = toSuffix(body)
    if (suffix === null) continue // leave this `{…}` folded into the next text run
    const start = match.index ?? 0
    if (start > last) tokens.push({ type: 'text', value: text.slice(last, start) })
    tokens.push({ type: 'symbol', className: `ms-${suffix}`, label: labelFor(body, suffix) })
    last = start + raw.length
  }
  if (last < text.length) tokens.push({ type: 'text', value: text.slice(last) })
  return tokens
}

/** Strip the braces from every `{…}` symbol, leaving its body as plain text
 *  (`"Ward {2}"` -> `"Ward 2"`).
 *
 *  For the text-only surfaces that can't render icons — a meta description, a JSON-LD
 *  `description` — where leaving the braces in would put literal `{2}` in a search
 *  snippet. Anywhere a component can render, use `ManaSymbols` instead. */
export function stripManaBraces(text: string): string {
  return text.replace(SYMBOL_RE, '$1')
}

/** Build the `{…}` text for a list of colour letters (`color_identity`/`colors`,
 *  e.g. `["W","U"]`) so it can be rendered through the same symbol path. */
export function colorLettersToText(letters: readonly string[]): string {
  return letters.map((letter) => `{${letter}}`).join('')
}

/** The slice of a `Card` {@link displayManaCost} reads — structural, so this stays a
 *  dependency-free text module (and a test needn't fabricate a whole printing). */
export interface ManaCostCard {
  mana_cost: string | null
  faces: readonly { mana_cost: string | null }[]
}

/**
 * The mana cost to show for a printing where there's room for exactly one.
 *
 * Scryfall stores **no top-level `mana_cost`** for the layouts whose faces each carry
 * their own — `transform`, `modal_dfc`, `reversible_card` — so a surface reading
 * `card.mana_cost` alone leaves the cell blank for every transforming Saga and MDFC in a
 * deck: "The Legend of Kyoshi // Avatar Kyoshi" has `{4}{G}{G}` on its front face and
 * nothing at the top level. Every other layout states a top-level cost and keeps it
 * untouched, whatever it means there: split and adventure cards carry the *combined*
 * `{1}{R} // {1}{U}`, a flip card carries its front face's cost alone (`{1}{U}` for Erayo,
 * Soratami Ascendant — the flipped half never has one), and the handful of adventure
 * *lands* carry only the adventure half's (`{2}{B}` for Midgar, City of Mako).
 *
 * The fallback is the first face that states a cost — the front face, which is the half
 * you cast from hand and the same half a compact row already shows the type line of. A
 * face with an empty cost (a transformed creature, an MDFC's back land) is skipped
 * rather than winning, so a card with no cost on either side — Westvale Abbey // Ormendahl,
 * Profane Prince — still answers `null` and renders nothing at all.
 */
export function displayManaCost(card: ManaCostCard): string | null {
  if (card.mana_cost) return card.mana_cost
  return card.faces.find((face) => face.mana_cost)?.mana_cost ?? null
}

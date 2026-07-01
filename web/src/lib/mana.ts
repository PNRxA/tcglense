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

/** Build the `{…}` text for a list of colour letters (`color_identity`/`colors`,
 *  e.g. `["W","U"]`) so it can be rendered through the same symbol path. */
export function colorLettersToText(letters: readonly string[]): string {
  return letters.map((letter) => `{${letter}}`).join('')
}

// String-similarity helpers for the card scanner, tuned to the specific ways OCR mangles
// Magic card text. OCR of small, stylised card fonts rarely fails randomly — it swaps
// visually-near glyphs (O/0, I/l/1, S/5, B/8, G/6, Z/2, T/7) and jitters spacing. These
// helpers fold those confusions so a *slightly* wrong read still resolves to the right
// catalog name (by re-ranking a returned pool) or the right printing (a near-miss set
// code), while never being trusted to auto-commit on their own — the user still confirms.

/** Visual-confusion groups: characters OCR routinely swaps for one another (lowercase
 * letters plus the digits that mimic them). A substitution *within* a group is cheap when
 * scoring OCR text; across groups it's a full edit. */
const CONFUSION_GROUPS = ['o0', 'il1j', 's5', 'b8', 'g69', 'z2', 't7', 'uv']

/** Discount for substituting one confusable glyph for another vs. a full unrelated edit. */
const CONFUSION_COST = 0.3

/** char -> its group's canonical key, for chars that belong to a confusion group. */
const GROUP_OF: Map<string, string> = (() => {
  const m = new Map<string, string>()
  for (const group of CONFUSION_GROUPS) for (const ch of group) m.set(ch, group.charAt(0))
  return m
})()

/** Best single-guess letter for a digit an OCR probably meant as a letter — used to
 * *build* a recovery substring query (which needs one concrete spelling, not a cost).
 * Digits with no strong letter twin are left as-is. */
const DIGIT_TO_LETTER: Record<string, string> = {
  '0': 'o',
  '1': 'i',
  '2': 'z',
  '5': 's',
  '6': 'g',
  '7': 't',
  '8': 'b',
  '9': 'g',
}

/** Length of the leading slice used to seed the fuzzy recovery pool query. Long enough to
 * keep the (capped, starts-with-first) suggestion list small, short enough to survive a
 * misread further along the name. */
const POOL_PREFIX_LEN = 5

/** Shortest prefix worth querying — below this the pool is too broad to be useful. */
const MIN_POOL_PREFIX = 4

/** Fold a string to the form we compare on: strip diacritics, lowercase, and keep only
 * alphanumerics, so spacing/punctuation jitter between OCR frames is ignored. */
export function canonical(s: string): string {
  return s
    .normalize('NFKD')
    .replace(/\p{M}/gu, '')
    .toLowerCase()
    .replace(/[^a-z0-9]/g, '')
}

/** Replace digits an OCR likely misread for letters with their letter twin — used to
 * retry a query when the raw (digit-bearing) read matched nothing. */
export function deconfuseDigits(s: string): string {
  return s.replace(/[0-9]/g, (d) => DIGIT_TO_LETTER[d] ?? d)
}

/** Substitution cost: 0 if identical, a discount if the two glyphs are confusable, else 1. */
function subCost(a: string, b: string): number {
  if (a === b) return 0
  const ga = GROUP_OF.get(a)
  if (ga !== undefined && ga === GROUP_OF.get(b)) return CONFUSION_COST
  return 1
}

/** Levenshtein distance with a pluggable substitution cost (rows kept, O(min) memory). */
function editDistance(a: string, b: string, cost: (x: string, y: string) => number): number {
  const n = a.length
  const m = b.length
  if (!n) return m
  if (!m) return n
  let prev = Array.from({ length: m + 1 }, (_, j) => j)
  for (let i = 1; i <= n; i++) {
    const cur = [i]
    const ca = a.charAt(i - 1)
    for (let j = 1; j <= m; j++) {
      cur[j] = Math.min(
        prev[j]! + 1,
        cur[j - 1]! + 1,
        prev[j - 1]! + cost(ca, b.charAt(j - 1)),
      )
    }
    prev = cur
  }
  return prev[m]!
}

/** Plain (unit-cost) edit distance — for short, closed comparisons like set codes. */
export function levenshtein(a: string, b: string): number {
  return editDistance(a, b, (x, y) => (x === y ? 0 : 1))
}

/** Similarity in `0..1` between two strings as OCR would read them: 1 is identical after
 * folding, and confusable-glyph swaps barely move it (so `Lightn1ng` ~ `Lightning`). */
export function ocrSimilarity(a: string, b: string): number {
  const x = canonical(a)
  const y = canonical(b)
  if (!x.length || !y.length) return 0
  const dist = editDistance(x, y, subCost)
  return Math.max(0, 1 - dist / Math.max(x.length, y.length))
}

/** Order candidate names by how closely they match the OCR read, best first. Stable: ties
 * keep the server's order (which already surfaces starts-with matches first). */
export function rankNames(ocrName: string, names: string[]): string[] {
  return names
    .map((name, index) => ({ name, index, score: ocrSimilarity(ocrName, name) }))
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((entry) => entry.name)
}

/** A short leading substring to seed a wider recovery pool when every exact-substring query
 * missed (a letter was misread). Digit swaps are undone first; null when there's too little
 * clean text to query usefully. */
export function namePoolPrefix(cleaned: string): string | null {
  const prefix = deconfuseDigits(cleaned).replace(/\s+/g, ' ').trimStart().slice(0, POOL_PREFIX_LEN).trim()
  return prefix.length >= MIN_POOL_PREFIX ? prefix : null
}

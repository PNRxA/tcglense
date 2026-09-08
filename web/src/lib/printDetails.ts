// Human labels for the print-detail slugs the card page renders (issue #673): the frame
// effects, promo types and finishes Scryfall tags a printing with. They arrive as bare
// lowercase slugs (`extendedart`, `stepandcompleat`, `nonfoil`) — unreadable as printed —
// so each list below spells the ones a player would recognise from the card in hand.
//
// The maps are deliberately partial: upstream adds a promo type or a frame effect with
// every set, and a card page must never render a raw slug *or* drop a fact it doesn't
// know, so anything unlisted falls back to `sentenceCase` (a readable approximation)
// rather than being hidden. Nothing here is derived from the catalog — a label is only
// ever a nicer spelling of the slug it keys on.

/** Readable fallback for an unmapped slug: word separators become spaces and the first
 * letter is capitalised (`unknown-effect` → "Unknown effect"). A run-together slug stays
 * run together (`boosterfun` → "Boosterfun") — it reads as a name, not as noise. */
export function sentenceCase(slug: string): string {
  const words = slug.replace(/[-_]+/g, ' ').trim()
  if (!words) return ''
  return words.charAt(0).toUpperCase() + words.slice(1)
}

/** Frame effects — the treatment applied over the frame (`frame_effects`). */
const FRAME_EFFECT_LABELS: Record<string, string> = {
  showcase: 'Showcase',
  extendedart: 'Extended art',
  legendary: 'Legendary crown',
  inverted: 'Inverted',
  borderless: 'Borderless',
  fullart: 'Full art',
  etched: 'Etched',
  colorshifted: 'Colorshifted',
  companion: 'Companion',
  devoid: 'Devoid',
  draft: 'Draft',
  fandfc: 'Fan double-faced',
  lesson: 'Lesson',
  miracle: 'Miracle',
  nyxtouched: 'Nyx-touched',
  shatteredglass: 'Shattered glass',
  snow: 'Snow',
  spree: 'Spree',
  sunmoondfc: 'Sun and moon',
  tombstone: 'Tombstone',
  waxingandwaningmoondfc: 'Waxing and waning moon',
}

export function frameEffectLabel(slug: string): string {
  return FRAME_EFFECT_LABELS[slug] ?? sentenceCase(slug)
}

/** Promo types — how the printing was handed out, plus the foil *treatments* upstream
 * files here rather than under the finishes. */
const PROMO_TYPE_LABELS: Record<string, string> = {
  prerelease: 'Prerelease',
  buyabox: 'Buy-a-Box',
  sldbonus: 'Secret Lair bonus',
  boosterfun: 'Booster Fun',
  promopack: 'Promo pack',
  datestamped: 'Date stamped',
  playpromo: 'Play promo',
  judgegift: 'Judge gift',
  arenaleague: 'Arena League',
  fnm: 'Friday Night Magic',
  gameday: 'Game Day',
  storechampionship: 'Store Championship',
  bundle: 'Bundle',
  setpromo: 'Set promo',
  release: 'Release promo',
  themepack: 'Theme booster',
  wizardsplaynetwork: 'Wizards Play Network',
  surgefoil: 'Surge foil',
  rainbowfoil: 'Rainbow foil',
  galaxyfoil: 'Galaxy foil',
  halofoil: 'Halo foil',
  stepandcompleat: 'Step-and-Compleat foil',
  textured: 'Textured foil',
  oilslick: 'Oil slick',
  confettifoil: 'Confetti foil',
  neonink: 'Neon ink',
  gilded: 'Gilded',
  serialized: 'Serialized',
}

export function promoTypeLabel(slug: string): string {
  return PROMO_TYPE_LABELS[slug] ?? sentenceCase(slug)
}

/** Finishes the printing exists in (`finishes`) — the wire's three slugs. */
const FINISH_LABELS: Record<string, string> = {
  nonfoil: 'Regular',
  foil: 'Foil',
  etched: 'Etched foil',
}

export function finishLabel(slug: string): string {
  return FINISH_LABELS[slug] ?? sentenceCase(slug)
}

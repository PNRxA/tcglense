// Human labels for the sealed-product `product_type` slugs the API derives (see
// `api/src/tcgcsv/classify.rs`). The backend stores a small, fixed vocabulary of
// snake_case slugs powering a plain-equality filter; here we map each to a readable
// label for the type dropdown and the product detail/tile. An unrecognised slug
// (e.g. a vocabulary the frontend hasn't caught up with) is humanised generically
// rather than shown raw, so the UI never renders `collector_display` literally.

const PRODUCT_TYPE_LABELS: Record<string, string> = {
  collector_display: 'Collector Booster Box',
  collector_pack: 'Collector Booster Pack',
  play_display: 'Play Booster Box',
  play_pack: 'Play Booster Pack',
  set_display: 'Set Booster Box',
  set_pack: 'Set Booster Pack',
  draft_display: 'Draft Booster Box',
  draft_pack: 'Draft Booster Pack',
  prerelease: 'Prerelease Pack',
  commander_deck: 'Commander Deck',
  secret_lair: 'Secret Lair',
  bundle: 'Bundle',
  case: 'Case',
  starter: 'Starter',
  display: 'Booster Box',
  pack: 'Booster Pack',
  other: 'Other',
}

/** Humanise an unknown slug: split on `_`, capitalise each word (`play_pack` →
 * `Play Pack`). Blank in, blank out. */
function humanise(slug: string): string {
  return slug
    .split('_')
    .filter(Boolean)
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ')
}

/** A readable label for a product-type slug, or a humanised fallback for one not in
 * the known vocabulary. */
export function productTypeLabel(slug: string): string {
  return PRODUCT_TYPE_LABELS[slug] ?? humanise(slug)
}

// The booster "family" name for each booster product-type slug (pack + box forms share
// one label). Mirrors the API's `booster_family` (see `api/src/tcgcsv/classify.rs`): a card
// exclusive to a family can't be pulled from any other, and the API flags such cards on the
// product-cards read — this labels the section that groups them (e.g. "Collector Booster
// exclusives"). A non-booster slug has no family, so no exclusive section is shown.
const BOOSTER_FAMILY_LABELS: Record<string, string> = {
  collector_pack: 'Collector Booster',
  collector_display: 'Collector Booster',
  play_pack: 'Play Booster',
  play_display: 'Play Booster',
  set_pack: 'Set Booster',
  set_display: 'Set Booster',
  draft_pack: 'Draft Booster',
  draft_display: 'Draft Booster',
  pack: 'Booster',
  display: 'Booster',
}

/** The booster-family label for a booster product-type slug (e.g. `collector_pack` →
 * `Collector Booster`), or `null` for a non-booster product. */
export function boosterFamilyLabel(slug: string): string | null {
  return BOOSTER_FAMILY_LABELS[slug] ?? null
}

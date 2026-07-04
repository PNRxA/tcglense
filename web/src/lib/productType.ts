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

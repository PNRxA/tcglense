// Pure geometry for the card scanner's alignment guide and OCR crop regions. The user
// aligns a physical card to a card-shaped guide box centred in the camera viewport; the
// OCR then reads two fixed sub-regions of that box (the title bar and the bottom-left
// info line) rather than trying to detect the card in the frame. Both the on-screen
// overlay and the pixel crops are derived from these functions, so they always line up.

/** A rectangle in whatever unit the caller works in (CSS px or video-frame px). */
export interface Rect {
  left: number
  top: number
  width: number
  height: number
}

/** A fractional rectangle, each field in `0..1` relative to some parent rect. */
export type FractionalRect = Rect

/** Standard MTG card width:height — 63×88 mm ≈ 61:85, the same ratio CardImage's frame
 * uses, so a printing fills the guide box edge-to-edge. */
export const CARD_ASPECT = 61 / 85

/** Fraction of the viewport's smaller axis left as breathing room around the guide box. */
export const GUIDE_MARGIN = 0.06

/**
 * The largest 61:85 card-shaped rectangle that fits, centred, inside a `width`×`height`
 * viewport with a proportional margin. Works in any unit — pass CSS pixels for the
 * overlay, video-frame pixels for the OCR crop.
 */
export function guideRect(width: number, height: number, margin = GUIDE_MARGIN): Rect {
  const availWidth = width * (1 - 2 * margin)
  const availHeight = height * (1 - 2 * margin)
  // Height-limited when the available box is wider than a card, else width-limited.
  const heightLimited = availWidth / availHeight > CARD_ASPECT
  const h = heightLimited ? availHeight : availWidth / CARD_ASPECT
  const w = heightLimited ? availHeight * CARD_ASPECT : availWidth
  return {
    left: (width - w) / 2,
    top: (height - h) / 2,
    width: w,
    height: h,
  }
}

/** Title bar (card name), as fractions of the card. Kept inside the mana cost on the
 * right and the frame border on the left, and shallow enough to miss the art below. */
export const NAME_REGION: FractionalRect = { left: 0.05, top: 0.036, width: 0.8, height: 0.086 }

/** Bottom-left info block (collector number over set code / language), as fractions of
 * the card — the strip modern frames print the set + number in. */
export const SET_REGION: FractionalRect = { left: 0.028, top: 0.9, width: 0.6, height: 0.088 }

/** Resolve a fractional sub-region against an absolute parent rect (e.g. the guide box). */
export function regionInRect(region: FractionalRect, rect: Rect): Rect {
  return {
    left: rect.left + region.left * rect.width,
    top: rect.top + region.top * rect.height,
    width: region.width * rect.width,
    height: region.height * rect.height,
  }
}

/** A rect as a CSS `%` inset object for absolutely positioning the overlay within its
 * (guide-box-sized) container. */
export function rectToPercentStyle(region: FractionalRect): Record<string, string> {
  return {
    left: `${region.left * 100}%`,
    top: `${region.top * 100}%`,
    width: `${region.width * 100}%`,
    height: `${region.height * 100}%`,
  }
}

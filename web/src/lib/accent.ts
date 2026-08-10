// The design system's accent presets — the brand hue the primary/ring tokens paint.
//
// Mirrors `api/src/accent.rs` (`SUPPORTED_ACCENTS`), like `lifeLayout.ts` mirrors the
// layout slugs: a slug added on one side only is either rejected by the API or renders
// as the default. The colour values themselves live in `src/assets/main.css` as
// `[data-accent='<slug>']` token blocks (every preset's light/dark pairs are
// WCAG-AA-checked — see docs/design-system.md), which is why only preset slugs exist:
// a free-form colour could not carry those guarantees. `swatch` is the preset's vivid
// (dark-mode primary) hex, used only to paint the settings picker dots.

export const ACCENT_OPTIONS = [
  { value: 'pink', label: 'Pink', swatch: '#f471a8' },
  { value: 'ember', label: 'Ember', swatch: '#f28e42' },
  { value: 'violet', label: 'Violet', swatch: '#ab91f2' },
  { value: 'teal', label: 'Teal', swatch: '#39bab4' },
  { value: 'blue', label: 'Blue', swatch: '#58a5e4' },
  { value: 'green', label: 'Green', swatch: '#5ebc7b' },
] as const

export type Accent = (typeof ACCENT_OPTIONS)[number]['value']

export const DEFAULT_ACCENT: Accent = 'pink'

export function isAccent(value: unknown): value is Accent {
  return ACCENT_OPTIONS.some((option) => option.value === value)
}

# Design system — "Ember & Nightfoil"

The app's visual identity, defined entirely as design tokens in
[`web/src/assets/main.css`](../web/src/assets/main.css) and consumed through ordinary
Tailwind utilities (`bg-primary`, `text-success`, `shadow-lift`, …). This document is the
rationale and the rulebook; the CSS file is the single source of values. Change them
together.

## Identity

- **Light mode is warm paper**: card-stock whites (`--background` ≈ oklch 0.983 hue 85),
  ink-brown foreground, warm gray borders. **Dark mode is "nightfoil"**: a violet-cast
  near-black canvas (hue ≈ 285) with the same warm foreground, so card artwork reads like
  foils on a dark table.
- **The brand hue is ember orange** (`--primary`, hue ≈ 45–55) — the magnifying-lens warmth
  of the old `#e8833a` PWA theme-color, now actually used: links, solid buttons, focus
  rings (`--ring`), hover borders (`hover:border-ring/60`), the wordmark's "Lense", icon
  wells, and the home page's tinted panels all pick it up through the tokens. Light mode
  darkens it for AA text-on-white; dark mode brightens it and flips the foreground pair.
- **Type**: Inter Variable for UI/body (`--font-sans`), **Bricolage Grotesque Variable**
  for headings (`--font-heading`, applied to `h1–h4` in the base layer and to the
  wordmark via the `font-heading` utility). Both are self-hosted `@fontsource` packages
  imported in `web/src/main.ts` — nothing on the critical path fetches third-party.
- Radius stays `--radius: 0.625rem`; the identity comes from color and type, not novelty
  corners.

## Token vocabulary

Beyond the stock shadcn set, the system defines (light and dark values each, all consumed
as Tailwind colors):

| Token | Meaning | Not for |
|---|---|---|
| `success` (+`-foreground`) | good / up / legal / won / connected | decorating things that aren't state |
| `warning` (+`-foreground`) | caution / restricted / triggered / unfinished | foil printings (see `foil`) |
| `info` (+`-foreground`) | neutral notice | a fourth chart series |
| `destructive` | bad / banned / down / dead (stock token, now the only red) | — |
| `foil` | the gold shimmer marking a *foil printing* (scan surfaces) | warnings — same hue family, different meaning |
| `rarity-uncommon/rare/mythic` | rarity chips (silver / gold / ember); common stays `bg-muted` | — |
| `shadow-card` / `shadow-lift` | resting / hover elevation, theme-swapped (light casts warm ink; dark needs true black at high opacity to read) | ad-hoc `dark:shadow-[…]` literals |

**The chip idiom** (how every status/domain tint is built): `bg-<token>/15` fill,
`border-<token>/40` border, `text-<token>` text — the token itself is the *text-strength*
color per theme, so no `dark:` companion is ever needed. Solid badges use
`bg-<token> text-<token>-foreground`.

**Never hard-code a palette class (`emerald`/`amber`/`red`/…) for state.** The 2026-08
migration folded ~150 of those onto the tokens above. Deliberately still literal:
server-supplied MTG mana hexes (`DeckStatBars`), camera-overlay chrome
(`ScanCameraSurface` — composited over live video, theme-independent), the footer's rose
heart, and `KeywordKindChip`'s taxonomy hues (a candidate for `info`-family tokens later).

## Accessibility receipts

Every token pairing was checked when the values were chosen (2026-08); re-run the checks
when changing values:

- **WCAG AA (≥ 4.5:1)** holds for: foreground/muted-foreground on background, card and
  muted; `*-foreground` on every solid fill (primary, success, warning, info,
  destructive-with-white in light); and every status/rarity token *as text* on the
  background in both themes.
- **The chart palette passes a CVD validator in both themes** — lightness band, chroma
  floor, adjacent-pair colorblind separation (worst deutan ΔE ≥ 10), normal-vision floor,
  and ≥ 3:1 contrast against the card surface. Hues correspond 1↔2↔… across themes
  (stock shadcn's didn't): 1 ember, 2 teal, 3 violet, 4 gold, 5 blue.
- Because the palette is validated **in token order**, `lifeSeries.ts`'s `SEAT_COLORS`
  takes the tokens in that order — re-shuffling it re-creates adjacent-hue clashes the
  validator ruled out (the old order put violet beside blue: deutan ΔE 1.1,
  indistinguishable).

## Coupled artifacts — change these together

- `web/index.html`: the three `<meta name="theme-color">` values mirror
  `--primary`/`--background`; the inline FOUC script mirrors `stores/theme.ts`
  (`tcglense_theme` key + `.dark` class).
- `web/src/components/ui/chart/index.ts` `THEMES` keys off the `.dark` class selector.
- Specs pin status classes (`CardLegalities`, `DeckLegalityBanner`, `ScanMatchPanel`):
  class renames move the specs in lockstep; pure token *value* changes break nothing
  (vitest never computes CSS).
- Dark mode is `.dark` on `<html>` (`@custom-variant dark` in main.css) — changing the
  mechanism touches all of the above plus `theme.spec.ts`.

## Recurring recipes (today's canon, for consistency — not yet components)

- **Linked tile**: `bg-card hover:border-ring/60 hover:bg-accent/40 rounded-xl border
  p-5 transition-colors` + `bg-muted size-12 rounded-lg` icon well (with the ember ring,
  hovers now warm the border — that's intended).
- **Section card**: `bg-card rounded-xl border p-4 shadow-sm`.
- **Stat row**: uppercase `text-xs text-muted-foreground` label over
  `font-semibold tabular-nums` value.
- **Sticky frosted bar**: `bg-background/85 sticky -mx-4 border-b px-4 backdrop-blur`.

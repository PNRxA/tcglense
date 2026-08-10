# Design system — "Nightfoil" + a switchable accent

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
- **The brand hue is a user-switchable accent, pink by default** (`--primary`/`--ring`):
  links, solid buttons, focus rings, hover borders (`hover:border-ring/60`), the
  wordmarks in header and footer, icon wells, and the home page's tinted panels all pick
  it up through the tokens. Light mode runs each hue dark enough for AA text-on-white;
  dark mode brightens it and flips the foreground pair. See "Accent presets" below for
  how the choice flows.
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

## Accent presets

The brand hue is a **server-side account setting** limited to a validated preset list —
free-form colors are deliberately rejected, because every preset ships with the AA
receipts below and an arbitrary hex could not.

- **Vocabulary**: `pink` (default) · `ember` · `violet` · `teal` · `blue` · `green`,
  defined twice and pinned by tests on both sides, like the life-counter layout slugs:
  `api/src/accent.rs` (`SUPPORTED_ACCENTS`, what `PUT /api/auth/accent` accepts, 422
  otherwise) and `web/src/lib/accent.ts` (`ACCENT_OPTIONS`, what the settings picker
  offers and the store validates). A slug added on one side only is either rejected by
  the API or renders as the default.
- **Storage**: `users.accent` (migration 70, default `pink`), riding the `User` DTO like
  `currency`; the update handler takes `WritableUser` (read-only API key = 403) and the
  route is in the OpenAPI `INTENTIONALLY_UNDOCUMENTED` allow-list with the other
  account-preference flows.
- **Application**: `stores/accent.ts` resolves account-accent-wins-over-local and stamps
  `data-accent` on `<html>`; `main.css`'s `[data-accent='<slug>']` blocks override only
  the brand trio (`--primary`, `--primary-foreground` in dark, `--ring`; the sidebar
  mirrors reference `var(--primary)` so they follow for free). The resolved accent is
  mirrored into `localStorage['tcglense_accent']`, which the `index.html` no-FOUC script
  stamps pre-mount — signed-in users get their accent on first paint, before auth
  restores. The picker lives on the authed settings page only; a device that has never
  seen a signed-in accent shows the default, and the mirror deliberately survives
  logout (like the theme choice), so the device keeps its last-seen look. The inline
  no-FOUC slug list is a third copy of the vocabulary, pinned by `accent.spec.ts`.
- **CSS ordering is load-bearing**: a preset's light block (`:root[data-accent=…]`,
  specificity 0-2-0) outranks the bare `.dark` block (0-1-0), so every preset's
  `.dark[data-accent=…]` twin must come after it — never add a light block without its
  twin (the comment above the blocks says the same).
- **Adding a preset** means: both slug lists (+ their pinning tests), a light/dark pair in
  `main.css` proven AA in *three roles* — fill under its `--primary-foreground`, link
  text on `--background`, **and text on its own tinted chip** (`bg-primary/15
  text-primary`, the strictest: light hues need L ≈ 0.48–0.52) — a swatch hex in
  `ACCENT_OPTIONS`, and the receipts noted here.
- **What the accent must never touch**: status tokens, rarity/foil, and the chart palette
  — charts are CVD-validated as a fixed set (below), which a per-user hue swap would
  silently invalidate.

## Accessibility receipts

Every token pairing was checked when the values were chosen (2026-08); re-run the checks
when changing values:

- **WCAG AA (≥ 4.5:1)** holds for: foreground/muted-foreground on background, card and
  muted; `*-foreground` on every solid fill (primary, success, warning, info,
  destructive-with-white in light); every status/rarity token *as text* on the
  background in both themes; and every accent's `text-primary` in all three roles
  (solid fill, link on background, and on its own `/15` tinted chip).
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
  p-5 transition-colors` + `bg-muted size-12 rounded-lg` icon well (the ring is the
  accent hue, so hovers tint the border toward the user's accent — that's intended).
- **Section card**: `bg-card rounded-xl border p-4 shadow-sm`.
- **Stat row**: uppercase `text-xs text-muted-foreground` label over
  `font-semibold tabular-nums` value.
- **Sticky frosted bar**: `bg-background/85 sticky -mx-4 border-b px-4 backdrop-blur`.

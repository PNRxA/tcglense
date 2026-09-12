# Don't break these — load-bearing invariants

The full statement of every invariant the always-loaded `AGENTS.md` lists in one line
each: what the rule is, which seams enforce it, and the couplings a change has to keep in
step. Read the section for the area you are touching **before** changing it; the *why*
behind each rule is [`tradeoffs.md`](./tradeoffs.md) and the exact wire shapes are
[`api-contracts.md`](./api-contracts.md). Learned a new one? Add it here, under its area,
and add the one-line summary to `AGENTS.md`.

## Auth, email, API keys and rate limiting

- Auth answers **generically** (register/resend/forgot reveal nothing; login's
  dummy-hash verify on unknown users is timing equalization, not dead code).
  Password rules are validated **before** an email token is consumed. A missing/bad
  CAPTCHA token is deliberately **400** (never 401/403).
- No email provider configured = the local email dev bypass (register returns the
  completion token; login skips the verified gate). A provider is either
  `RESEND_API_KEY` or the Cloudflare Email Service pair (`CLOUDFLARE_EMAIL_API_TOKEN`
  + `CLOUDFLARE_ACCOUNT_ID`); configure exactly one (Resend wins if both are set).
  Internet-facing configurations refuse to enable signups without a provider, a
  non-default `EMAIL_FROM`, and Turnstile; keep those bypasses local only.
- **API-key scope is enforced by extractor choice, not HTTP method:** reads take
  `AuthUser` (session JWT or any `tcgl_` key), writes take `WritableUser` (read-only
  key = **403**), key management (`/api/auth/api-keys`) takes `SessionUser` (JWT
  only — a key can't mint/list/revoke keys). A bad/expired/revoked key is **401**.
  Keep the per-user rate limiter key-aware (`ratelimit/per_user.rs` resolves `tcgl_` → user)
  or keyed traffic bypasses the quota. Store only the SHA-256 hash; the plaintext is
  shown **once**.
- Rate limiting **fails open** (Redis outage, unresolvable IP); CAPTCHA fails
  **closed**. `TRUST_PROXY_HEADERS=true` only behind a proxy that overwrites
  `X-Forwarded-For` (else clients spoof their IP).

## Collection and wish list

- Collection and wish list are **independent tables** that share the ts-rs DTOs in
  `handlers/shared/holdings.rs` — editing a shared card shape changes both wire surfaces.
  Card holdings use **external** card ids; both counts zero deletes the row. A filter on
  *what is held* (the copy-count filter, `min_copies`/`max_copies`/`finish`, issue #677) is a
  `ListParams` field resolved once in `resolve_holdings_list` and applied inside each twin's
  query builder — **never a `q:` leaf**: the search compiler is shared with the public,
  CDN-cached catalog listing and must not learn per-user state (`is:foil` matches the
  catalog's finishes, not the user's). Landing in the seam is what lets the `.txt` export and
  the grouped views inherit it, and the SPA mirrors the one URL grammar for it in
  `lib/holdingsFilter.ts`. **The breakdown** (`GET /api/{collection,wishlist}/{game}/breakdown`,
  issue #680 — value by rarity / colour / type / finish + the top holdings by *held* value) is
  the twins' third analytics read and lives in the seam too: `handlers/shared/breakdown.rs`
  folds a `BreakdownRow` (the `SummaryRow` widened by four facet columns, projected through
  `narrow_breakdown_rows` on top of the summary's own column list) and embeds
  `summarize_holdings` over the same rows, so its `summary` **is** the header's; each twin
  contributes only its entity query. It rides `analytics_cache` like value history and movers,
  but keyed per **surface** (`HoldingsSurface::{Collection,Wishlist}`) — every wish-list card
  write must `bump_surface_holdings(Wishlist, …)`, or the cached wish-list breakdown outlives
  the edit — and the per-user `analytics` bucket. The type bucket reads the type line's *first
  card type* through `shared::type_line::primary_type` (the same supertype table the Archidekt
  CSV export splits on). Both surfaces
  also hold sealed products in independent `collection_product_items` /
  `wishlist_product_items` tables (`/api/{collection,wishlist}/{game}/products*`, external
  TCGplayer ids on the wire, same both-zero-deletes rule) through the lower shared seams:
  `handlers/shared/product_holdings.rs`, `lib/api/product-holdings.ts`, and
  `composables/productHoldingQueries.ts`. Collection import/export
  remain card-only; collection value history and movers include both card and sealed-product
  holdings. **Public sharing exposes sealed products too:** the read-only
  `/api/u/{handle}/{game}/products{,/summary,/sets}` reads mirror the authed collection product
  endpoints (`collection::owned_product_{summary,sets}`/`owned_products_page` wrap the same
  `CollectionProductRepository`), gated by the identical per-game visibility flag; the public
  landing (`PublicCollectionView`) renders them through the shared `ProductHoldingSection`
  (public mode = a `handle` prop) and the read-only `PublicProductBrowseView` (a `readonly`
  `ProductGrid`, owner's counts as a static badge).

## Sealed products and booster odds

- **No number on a sealed product's page is a count of a copy's physical cards** — with two
  exceptions, a per-pack *expectation* and one seeded *simulation*, both qualified on the wire
  (the booster bullet below). Nothing the card sections carry is such a datum, so none of them
  may be worded as containment. Of the
  card-section keys only `contains` is a guarantee; `exclusive` is a **subset** of the
  `booster` pull pool (never added to it) and `variable` is a randomized configuration, so a
  total summed across sections is a *pool* size —
  a booster with a 600-card pool holds ~15 of them, which is what "Cards in this product (600)"
  used to claim. `sealed_contents` also has **no quantity column**, so a section total counts
  **distinct cards** (a precon's 30 Forests are one row); only `ProductComponent.quantity` counts
  pieces, which is why "N items in the box" sums quantities rather than line items.
  `web/src/lib/productCounts.ts` is the single seam that folds a manifest into per-certainty
  counts and words the heading + the overview chips from them (shared so the two can't disagree);
  it **mirrors** `CardSection::classify`'s key set, display order, and unknown-key→`variable`
  fallback, like `lib/legality.ts` mirrors the format table — a fifth key or a reordering lands on
  both sides. A label that names a pool must be true of the block it heads: the `booster` section
  is the pool's *shared remainder* whenever `exclusive` was split out above it — and its blurb
  describes what *these* boosters open, never "what the other boosters have" (the remainder spans
  special printings the main boosters don't carry, so that reading was wrong). The exclusive
  split itself is judged **only for a product whose own `product_type` is a booster family**: a
  bundle never gets an `exclusive` section, however premium the booster it wraps — its pool rows
  can be direct (nameless `sealed` refs attribute nothing), which made the old contained-family
  split surface "Exclusive to Collector Boosters" on bundle pages the inherited-hiding can't
  touch (issue #646 follow-up). **Exclusivity is a stored column, never re-derived in a read**
  (`sealed_contents.exclusive`, `m..085`): it is a cross-product fact — decided by every
  sibling booster's whole pull pool — so judging it per request scanned that pool on every page
  turn (7.2s over 7,968 rows in production), and it is a *sort* key too, so no page could narrow
  it. `catalog::sealed_exclusives` stamps it **per sync tick, and at boot only on the no-sync
  path** — wired exactly like `precon_values` rather than into the ETag-gated derivation,
  because the judgement also reads
  `products.product_type` — TCGCSV's, which moves on its own sweep — so a rebuild-time fold
  would go stale on a reclassification (and needing no `DERIVATION_VERSION` bump is the other
  half of that choice). The rebuild writes the `false` default; the pass owns the column, and
  a fixture that asserts *either* side of the split must run the pass or it passes vacuously.
  The read still intersects the flag with the plain view's **collapsed** membership — a card a
  product both guarantees and can pull is `contains`, and must not lead the booster pool.
  **Sections split by source, and the split starts at ingest:** the MTGJSON walk stamps every
  membership row inherited through a nested `sealed` reference with the top-level component's
  name (`sealed_contents.component`, same string as the `sealed_components` row, `NULL` for a
  product's own cards). The manifest then renders an **unlisted** sub-product (a bundle's land
  pack — no catalog listing to click through to) as its own named per-certainty sections
  (`component` on the wire, paged via `?component=`), and flags a plain section `inherited`
  when every card came through a **listed** sub-product. The SPA hides inherited
  `booster`/`exclusive` sections — that pool lives on the linked child's page, one click away
  in "What's in the box" — through `visibleProductSections` in `lib/productCounts.ts`, shared
  by ProductCards and ProductOverview so the chips can never count a pool the sections hid.
  An inherited `contains` stays visible (hiding a guarantee loses information); the flat
  (`?section`-less) `/cards` list stays whole-product and per-card-deduped for API consumers.
  **The two exceptions are an expectation and a simulation, and they ride the same seam.**
  Since #682 the API does hold each booster's own sheet configuration, so
  `PackEv.cards_per_pack` — the cards an
  **average** pack deals — and the expected values beside it are genuine per-pack numbers, the
  only ones on the page, and a `PackOpening`'s totals are one seeded roll of the dice. Neither
  may be worded as contents or as a promise, and `productCounts.ts` words both:
  `expectedValueHeading` ("per pack" vs "per copy", always "on average"), `cardsPerPackLabel`
  (a fractional expectation printed as the range it really is — `14–15 cards per pack` — never
  a fake decimal), `oddsLabel` ("1 in 24 packs"; "most packs" under 1.5, where a ratio stops
  informing), `boosterLabel`, `evVersusPrice` / `openingVersusPrice` (a *share of* today's
  price, never a gain), `openingSummary` (past tense, singular to this run). Its spec pins that
  no such string reads as containment and that every money figure keeps its qualifier, and each
  response's server-authored `caveats` are meant to be **shown**.
- **Booster odds** — a sealed product's expected value (`GET .../products/{id}/ev`) and its
  seeded pack opener (`.../open?seed&copies`), issue #682 — are three catalog tables plus two
  pure engines (`handlers/catalog/boosters/{ev,open}.rs`). The tables are **rebuilt wholesale**
  by the MTGJSON sealed sync in `sealed_contents`' own transaction — `booster_configs` (a
  booster's pack variants), `booster_sheets` (each slot's weighted pool) and `sealed_packs`
  (which boosters one copy of a product opens, and how many) — so a row id is not stable and
  never reaches the wire, and a change to how the walk flattens a quantity, keeps a variant or
  weights a sheet needs a `DERIVATION_VERSION` bump like a precon slug does (the sync is
  otherwise ETag-gated, so a pure code change takes effect no other way). Nine couplings.
  `booster_sheets.total_weight` counts **every** parsed card, *including* the ones our catalog
  couldn't resolve — that gap is the read's `priced_share` and a caveat, never re-normalised
  away, because re-normalising over what survived would silently inflate every remaining card's
  odds. A **`fixed`** sheet's slot takes its first `count` cards **in stored order**, so
  `booster_sheets.cards` is upstream's order and nothing may sort it. `sealed_packs.quantity` is
  flattened at ingest (a box's `sealed` reference × its `count`, a case of boxes on top of that;
  two sibling references to one pack **sum**, while two `sealedProduct` entries sharing a
  TCGplayer id take the **max** — summing them would sell a 36-pack box as 72 — and the walk's
  path stack, seeded with the product's own `uuid`, guards a self-reference). A configuration
  with no variants, or zero total weight, **opens empty** rather than failing: that is a bug
  upstream, not a bad request, and a public catalog read must not `500` on it. The opener and
  the deck goldfish share **one** generator (`handlers/shared/rng.rs`) and its stream **is a
  wire contract** — the reference-stream test pins SplitMix64's constants, since every shared
  hand and every shared opening depends on them; so is the opener's own derivation (state per
  `(seed, pack ordinal)`, warmed once), which is what makes pack *n* the same pack whatever
  `copies` was. A **seedless** `/open` must stay `no-store` (a random roll is not a function of
  its URL, and the route sits in the CDN-cached catalog group), while a seeded one is ordinary
  cacheable catalog. The bounds — **36 packs** and **1200 cards**, the latter measured off each
  pack's fattest variant — are checked **before** anything is drawn and answer `422`, because a
  `quantity` and a slot count are both ingested data. A **foil** sheet prices at the card's foil
  price and **never** falls back to the regular one (that is a different card's price). And
  `/ev` is CDN-cacheable and answers `{ data: null }`, **not** `404`, for a product with no
  booster data — a commander deck, a randomised `variable` pack, anything MTGJSON doesn't
  describe: nothing to say is not a failure, and an error on every such product is worse than a
  hidden panel.

## Decks

- **Decks** (`/api/decks/{game}*`, issue #363) are a **container** surface — many per user,
  in `decks`/`deck_sections`/`deck_cards`/`deck_folders` — **not** a collection/wishlist twin,
  so they don't ride `makeHoldingApi`/`makeHoldingQueries`; they live beside it and only reuse
  the *lower* seams (`deck_card::Model` impls `HoldingCounts`; the `Card` DTO). A `deck_card`
  has **no `user_id`** (it hangs off `deck_id`), so every deck route must `load_deck` to prove
  ownership first — a deck that isn't the caller's is **404, not 403**. Per-deck sharing is an
  `is_public` **column** on the deck row (not a `collection_visibility`-style table — a deck is
  1:1 with the shareable unit), public at `/api/u/{handle}/decks/{id}` (username-first `409`,
  reusing #361's `resolve_public_user`). A **maybeboard** is likewise a column
  (`deck_sections.is_maybeboard`, issue #570), never a name match: its cards are still stored,
  returned, and edited normally, but every "what is this deck" reader skips them — `summary`
  (vs its sibling `maybeboard_summary`), the list's `card_count`, `needed`, and, client-side,
  legality + analytics. A new such reader must split on the flag too, or the deck page's header
  and the deck list will disagree. The **name** (`is_maybeboard_section_name`) only *seeds* the
  flag where a section is born from untyped text — deck import, and migration 62's backfill of
  pre-flag decks — so a renamed maybeboard stays out and a section merely called "Considering"
  stays in. The deck **list header** carries two *derived* facets beside `card_count` —
  `color_identity` and `commanders` (`handlers/decks/facets.rs`) — and **a deck's colours are
  its command zone's when it has one, the union over its deck proper otherwise**: a Commander
  deck is Mardu because its commander is, not because the 99 happen to play black yet. It must
  keep borrowing *both* of the analysis module's answers, never re-deciding either: which
  sections are the zone is `rules::deck_zone`'s (like the goldfish library's), and whether that
  zone **leads** the deck is `rules::format_leads_with_command_zone`'s — every deck is seeded
  with a `Commander` section, so in a format without a command zone the cards in it are just
  part of the 60, as `evaluate_deck_rules` already treats them. Sideboards are outside the
  union too (`card_count` counts them; colours don't), and `commanders` is capped + name-deduped
  because neither list endpoint paginates. Every `DeckResponse` producer (authed list, public
  list, import, rename, folder move) builds through the one `deck_headers`/`deck_header` seam —
  a third derived field belongs there, not at a call site.
  **The deck pages' analysis stack sits behind one collapsible** (`components/decks/DeckOverview.vue`,
  on the owner, public and precon pages): collapsed by default to a chip strip whose every chip is a
  pure function (`lib/deckOverview.ts`) of the response the matching panel renders — never a second
  read, never a second rule — so the strip can't disagree with the stack it hides; its toggle is
  **persisted** (`stores/deckView`'s `overviewExpanded`) while each panel's own "Details" stays
  per-mount, on purpose (`docs/tradeoffs.md` §Decks). A tenth panel goes inside the slot (with a glance in
  that lib if it has a verdict to state before being asked), or stands outside like the add-cards
  box because it belongs to the list; a control that lives inside must be reset on the overview's
  `collapse` event (the role filter is), and URL-addressed state inside it must open the overview
  (the owner view does for `?compare=`).
  **Deck analysis is server-side** (issue #596): composition + draw odds
  (`/stats`), the legality verdict (`/legality`), the estimated Commander bracket
  (`/bracket`), the tokens the deck makes (`/tokens`), the mana base (`/mana`, issue #670),
  and a seeded sample hand
  (`/goldfish`) all live in `handlers/decks/analysis/`, so a CLI gets what the deck page shows; each is
  mirrored under `/api/u/{handle}/decks/{id}/…` **through the same `analyse_*` core**, so a
  shared deck and its owner's copy can never disagree. All six are **`GET`s taking
  `AuthUser`** — they write nothing, so a read-only key must be able to call them.
  **The mana base is a citation, not a model** (`analysis::mana`): the sources-needed numbers
  are Frank Karsten's 2022 summary table held as a data constant with its source (a test pins
  every number), applied with his **one** stated rule of thumb (gold cards +1) plus three
  simplifications that are this module's own and are named as such in every response's
  caveats — hybrid/Phyrexian/twobrid pips reported but never counted against a colour (which
  is *narrower* than Karsten, who asks for the table number in *combined* sources across a
  hybrid's colours; that union requirement is not computed), a cost past its pip group's last
  row judged as that row (an over-estimate, flagged `clamped`), and an X-cost spell listed but
  never decisive (his advice for one is "the lands you expect to tap", a fact only the player
  has) — and nothing else, never re-simulated. Demand is the library **plus** the command zone
  and supply the library alone (a commander's pips count; its section is never a source);
  which sections are the zone is `rules::deck_zone`'s answer and whether it leads the format
  is `rules::format_leads_with_command_zone`'s, the pair the facets borrow — in a format with
  no command zone the seeded `Commander` section supplies mana like the rest of the 60. Two
  couplings: `cards.produced_mana` stores "produces nothing" as `""` (`scryfall::map`,
  backfilled by `m..079` without touching `updated_at`) so a NULL can mean "not checked yet" —
  the `token_parts` stance — and the `produces:` search leaf's colourless branch reads both
  spellings; and `CardFacts::mana_cost` falls back to the first face's cost, read off the
  front half of a split card only.
  **Tokens are a provider fact, not a grammar** (`analysis::tokens`): what a card makes is
  read off `cards.token_parts` — Scryfall's `all_parts`, filtered at ingest to `token`
  components plus emblems (which upstream files as `combo_piece`, told apart by the printed
  type line) — the same stance legality takes on the legality object. Four couplings.
  The relation is **oracle-level** (every printing carries it), so no union across printings
  is needed; the referenced **id is set-specific**, so tokens group by the *token printing's*
  own `oracle_id` and never by name — Wurmcoil Engine's two Wurms share a name and a type
  line, and merging them would tell a player to bring one. An **unresolvable reference is
  placed in a second pass**, joining a resolved group only when exactly one shares its name
  and type, so deck order can't split one token into two and an ambiguous case is never
  guessed. And **NULL is not `[]`**: `token_parts` is NULL on any row not rewritten since the
  column arrived, so `map::token_parts` writes `[]` for a card that makes none and the read
  reports the NULLs as `unchecked_count` rather than answering "this deck makes no tokens".
  Nothing may state **how many** of a token to bring: "create a Treasure" and "create X
  Treasures" are the same relation upstream, so the response counts *cards*, and the SPA
  panel's tests pin that it never words a token quantity. It counts **cards**, so the sources
  fold by name (`fold_sources_by_name`) like `rules`'s `NameFold` does — a deck row addresses a
  printing, and a split playset is one card making one token, not two.
  **Legality is two modules, not one:** `analysis::legality` judges each card against the
  format's Scryfall data, `analysis::rules` judges the deck (size, copy limit, command zone,
  colour identity) and the former composes the latter, so a new check belongs in the rules
  module, not a third one. Its zone split reads the section **name** (`Commander`,
  `Sideboard`, …) because a `deck_card` has no board role; keep those spellings in step with
  `deck_import::parser`'s. A rule that matches a **card name** goes through `rules::answers_to`,
  never `facts.name` directly: the catalog stores the *printing*'s name, and a Secret Lair
  reversible printing repeats one card either side of a `//` ("Okaun, Eye of Chaos // Okaun, Eye
  of Chaos") — a spelling no other card's oracle text ever uses, which is how a published,
  legal precon came to be told its two "Partner with" commanders couldn't lead together.
  **A commander is not only a creature** — rule 903.3 takes "a creature card, a Vehicle card, or
  a Spacecraft card with one or more power/toughness boxes", so `can_lead` reads that box off the
  row (`CardFacts::has_power_toughness_box`) and not the station reminder text that explains it.
  It is what separates the seven legendary Spacecraft that lead a deck from The Eternity
  Elevator, which never becomes a creature — a distinction "any legendary Spacecraft" would lose.
  Every deck-wide rule is skipped rather than guessed when the format
  has no profile or the command zone is empty, and "not finished yet" is a `warning` severity —
  a half-built deck must never be reported as illegal. The rules module's one submodule,
  `rules::rulebreaker`, reads the commanders that **rewrite** those rules for their own deck
  (MBC's **Rulebreaker** keyword: Whtz lifts the maximum deck size, the other seven widen
  colour identity for the cards they name). It is a **grammar over the card's own text**, not
  a list of ids — the same principle as `card_copy_limit` reading "any number of cards named"
  — gated on the `Rulebreaker` keyword line, which every phrase it keys on is exclusive to.
  Four couplings. The effects are read off the **command zone only** ("a deck with *this*
  commander"), so the same card in the 99 — or in a format with no command zone, where every
  deck still carries a seeded `Commander` section — grants nothing; tests pin both halves,
  because reading them off the whole deck instead leaves every other test in the module green.
  A Rulebreaker the grammar **can't** parse stands the widened rules down rather than
  reporting a deck illegal against rules the card may have lifted — and a descriptor list is
  therefore read **whole or not at all**: stopping mid-list would keep the descriptors already
  read *and* drop the rest, which is both too generous and, on the half it drops, a false "in
  breach". Tolabow's "one colour of your choice" is spent on the colours that save the most
  cards, because the player chooses after building; the search is bounded by the five colours
  and resolves each named card's needs once, so neither the deck nor its copy counts enters
  the exponent. Finally, effects are **deduplicated and collected per distinct card**, never
  per deck row: every effect is later tested against every card name, and a deck's
  command-zone row count is caller-controlled (section names are unique only
  case-sensitively, so all 200 sections can read as `Commander`) — holding one copy per row
  made a public `…/legality` read 137x slower on a deck built to do it. The format table + the breach-severity
  order are **mirrored** in `web/src/lib/legality.ts` (a dropdown must not wait on a request)
  with tests pinning both sides, like `lifeLayout.ts`; `GET /api/games/{game}/formats`
  publishes the server's copy. The default library the odds and the goldfish shuffle is derived
  from `rules::deck_zone`, **never a second list of names** — a section the stats called
  non-library while the rules called it the command zone would deal an Oathbreaker deck its own
  oathbreaker. The **goldfish is stateless**: the hand is a pure function of
  `(seed, mulligans, what was bottomed, how many drawn)`, all in the query string, so there is
  no session table and a hand is reproducible from a URL — which is why its shuffle is a
  hand-rolled SplitMix64 + Fisher–Yates rather than `rand`, whose generators don't promise a
  stable stream across versions. Two consequences of that statelessness are load-bearing:
  the seedless public mirror answers **`no-store`** (a random seed makes the response not a
  function of its URL, so a CDN would pin one visitor's roll for everyone), and the shuffle
  is **bounded** — it materialises one slot per *copy* and a deck row's counts are
  caller-controlled, so an oversized library is a `422`, never an allocation. For the same
  reason the command-zone check counts copies instead of expanding them; **nothing on these
  paths may go per-copy.**
  **Card roles are a grammar, not a list** (`analysis::roles`, issue #671): "ten ramp, ten
  draw, eight removal" is counted off each card's rules text through the **same clause grammar
  the bracket's signals read** — `analysis/signals/` (mod.rs the shared grammar, `bracket.rs`
  and `roles.rs` the predicates; the tutor role *is* `signals::bracket::is_tutor`), so a ninth
  role or a fifth bracket category extends that module rather than starting a third copy of
  "does this clause say X". Same stance as the bracket: **every predicate declines when unsure**
  (a land search to hand isn't ramp, "Whenever you draw a card" isn't draw, a target *you
  control* is a flicker not removal, `Hexproof` on its own line protects nothing but itself),
  the counted cards ride the response, and a card name is matched through `rules::own_names` /
  `answers_to` — never `facts.name` — so a reversible printing's `Name // Name` still answers.
  Maybeboards out, command zone in; both of those and the bracket count through the one
  `analysis::fold_by_name` seam (representative = smallest external id, so a precon and its
  copy answer byte-identically). The `card_roles` map is keyed by **printing**, because the
  deck page filters rows by the printing they hold; the per-role `cards` lists are capped, the
  counts never are, and `unclassified_count` is on the wire so the bars can't be read as a
  partition of the deck — and it holds **every** row, maybeboards included, because the list a
  page narrows by it shows them (the counts don't). Three route mirrors, like every analysis read.
  **The bracket estimate is a floor, not a verdict** (`analysis::bracket`): it reports the
  lowest of Wizards' rungs the deck's cards don't rule out and is **only ever 2, 3 or 4** —
  1 (Exhibition) and 5 (cEDH) are claims about *intent*, so asserting either from a list would
  be inventing a fact. It answers `null` for every format but `commander`, the one the ladder
  is defined for. Only two categories decide the number (any mass land denial, or more than
  three Game Changers, is bracket 4; one to three Game Changers is 3); extra turns and tutors
  are **reported and never decisive**, because what brackets 2 and 3 actually forbid —
  *chaining* extra turns — isn't in the list, and a caveat saying so ships with every response.
  Game Changers are read off the catalog's `game_changer` column (Wizards' curated list,
  published on the card); the other three are a **grammar over oracle text**
  (`analysis/signals/bracket.rs`) built on `rules`'s `ability_lines`/`has_word` rather than a second
  copy of them — same stance as the construction rules, since a false positive costs a player
  two brackets: every predicate declines when unsure, and every counted card rides the
  response so the number can be audited. The ladder's labels **ship in the payload** instead
  of being mirrored client-side like the format table above: the panel that draws them doesn't
  exist until the response lands, so a mirror would buy nothing and could drift.
  **Pricing is the sixth analysis read** (`/pricing`, issue #672) and takes the same shape as
  the five: a `GET` on `AuthUser`, mirrored at `/api/u/{handle}/decks/{id}/pricing` through the
  one `analyse_pricing` core. Its `total_usd` **is** `summary.total_value_usd` — the same deck
  proper rows through the same `Valuation`, never a second fold — and `cheapest_total_usd` is
  that total minus the summed savings, so the three numbers can't disagree. **Cheapest is
  judged at the row's own finish split** (`usd × quantity + usd_foil × foil_quantity`), because
  the swap preserves it: the drops surface's cheapest-single-copy question would name a
  cheap-foil/dear-nonfoil printing for a nonfoil row and make the deck dearer, and a printing
  unpriced in a finish the row holds is no candidate at all. A **saving needs both sides
  priced** (a held printing unpriced in a held finish is a floor), ties stay on the held
  printing, and `null` is "unpriced", never `"0.00"`. Candidates come from
  `handlers/shared/cheapest.rs`, which excludes **folded foil-★** rows — their foil price is
  already on the base, so a star could only tie, and a swap that took the tie would land on a
  printing no grid shows; that seam is also the Secret Lair drops' "cheapest prints" total, so
  a change to what counts as a candidate moves both. The swap itself stays the existing
  `WritableUser` `PUT …/cards/{id}/printing` — "swap all" is that same write batched
  client-side (`useChangeDeckCardPrintingsMutation`, sequential, one invalidation), **never a
  new bulk write**, so "same gameplay card", the finish split and the count merge are validated
  in one place. Deck writes must invalidate the analysis query family
  client-side (`invalidateDeckAnalysis`, `['deck-pricing', …]` included); it doesn't sit under
  the `['deck', …]` key.
  **Combos are a dataset, not a grammar** (`/combos`, issue #683 — the ninth analysis read, three
  route mirrors like the rest): which cards go infinite together is a fact about *several* cards,
  so it is read off the synced Commander Spellbook database (`combos` + `combo_pieces`, keyed by
  **oracle id** — `CardFacts::oracle_id`, any printing matches), never a grammar over rules text.
  The provider is `spellbook/` and **only the mirror origin fetches upstream** (a ~650 MB JSON
  document, streamed through `spellbook::stream`'s splitter, never buffered); every other
  instance imports the origin's compact gzipped-JSONL re-serve (`/api/mirror/spellbook/combos`,
  `COMBOS_SYNC_ENABLED`); the origin itself asks upstream only every
  `COMBOS_UPSTREAM_INTERVAL_DAYS` (30 — their terms say sparse), a failed run retrying sooner — the Secret Lair stance, and like those two it is **never fatal to the
  sync tick** (the mirror answers 404 until its origin has imported once). Both paths write
  through the one `replace_combos` swap, as does the dummy seed. Four rules the read decides once (`classify`):
  maybeboards out, sideboard + command zone in; a `must_be_commander` piece counts only from the
  command zone of a format that leads with one (the same two `rules` answers the facets borrow — in a format without one such a combo is unreachable and never listed);
  a **template** ("any sac outlet") is always one missing card, so such a combo is never
  "complete"; `almost` (one card short) is filtered to the commander's colours. `available: false`
  is "no data synced", never "no combos" — the `token_parts` NULL stance. The bracket estimate
  deliberately does **not** read the table (its floor-not-verdict contract, `docs/tradeoffs.md`).
  Attribution is a term of use: every combo carries its `url`, every response its `source` +
  `source_url`, and the SPA panels name and link Commander Spellbook.
  **Suggestions are the analysis read with no public mirror** (`/suggestions`,
  issue #684): the cards the caller already owns that the deck could play — collection ∩ colour
  identity ∩ format legality ∩ not already in the deck — ranked by `cards.edhrec_rank` and grouped
  by role. It **reads the caller's collection**, so it is never mirrored under `/api/u/{handle}`
  or the precons, and it is honest about what the rank is: EDHREC's *global* popularity (the
  per-commander tables have no bulk export and would be a scrape), never synergy, and the first
  caveat on every response says so — the SPA keeps that line in view even collapsed. Every
  answer it needs is borrowed, never re-decided: the colour identity is the facets' rule over the
  loaded deck (`suggestions::deck_colour_identity` — the command zone's when
  `format_leads_with_command_zone` and it holds a card, else the union over the deck proper with
  the sideboard out; `None` = no colour filter, `Some([])` = colourless), legality is
  `legality::status_of`, the role is `roles::roles_of` (so "in deck N · you own M more" counts
  both sides with one grammar), and "already in the deck" is the shopping list's `identity_key`
  over **every** section, maybeboards included. A filter the server didn't apply is stated on the
  wire (`color_identity`/`format_key` null) and in `caveats`, never implied. **Bounded two ways:**
  the collection scan selects the narrow columns only and folds by identity, then only the
  `SCAN_CAP` most popular survivors are loaded in full for the role grammar — `candidate_count`
  stays exact, `scanned_count` says how many were classified, and every list on the wire is
  **ids into one `cards` pool** (a card filling three roles is serialised once; nine lists of
  full `Card`s was a body the analytics cache's memory bound couldn't hold). The body is memoised in
  `analytics_cache` under the holdings version, the price epoch and the day like value history,
  **plus a fingerprint of the deck's format + rows** (`read::deck_fingerprint`), because those two
  counters don't cover a deck edit; client-side it must be invalidated by **both** deck writes
  (`invalidateDeckAnalysis`) and collection writes (`holdingQueries`'
  `invalidateCollectionAnalytics` gate, collection-only), since the answer reads both. The "Add"
  on a suggestion is the add-cards box's own engine (`useDeckCardAdder`: automatic-by-type filing,
  optimistic counts, the existing absolute-count `PUT …/cards/{id}`), never a second way to file a
  card.
  **Every deck clone goes through one seam** (`decks::copy::insert_deck_with_cards`): the public
  copy, the owner's own duplicate (`POST /api/decks/{game}/{deck_id}/copy`, issue #674 — `load_deck`
  first, lands in the source's folder, answers a `Deck` header through `deck_header`) and the precon
  copy all write through it, so the deck cap, the transaction and the chunked insert are stated
  once; a deck-sourced clone also reads its sections through `copy::source_sections`. **The deck
  diff** (`GET …/{deck_id}/diff/{other_id}`, `decks::diff`) is a pure fold over two `DeckDetail`s
  that **folds by card name** across printings and finishes — the `analysis::fold_by_name`
  identity the bracket and mana base count by (not the precon copy's `push_folded`, which folds by
  printing id within a section for the `(deck_id, card_id, section_id)` unique constraint) — or a
  playset split across two arts reads as a removal plus an addition; a finish-only change is its
  own kind (`finish`), never hidden and never counted as a card change. Both decks are ownership-checked (either foreign is 404), maybeboards ride the
  per-section view flagged and stay out of the deck-wide `cards`/`summary`, and the SPA's wording
  lives in `web/src/lib/deckDiff.ts`; the panel keys its pick on `?compare=` so a comparison is a
  link (the first pick pushes history, a re-pick replaces it, and a `No comparison` item clears it),
  and an id the deck list doesn't offer is treated as no pick once the list has loaded.
  **Adding a deck or a precon to the collection** (`POST /api/decks/{game}/{deck_id}/collection`,
  `POST /api/decks/{game}/precons/{slug}/collection`, and someone's public deck at
  `POST /api/u/{handle}/decks/{deck_id}/collection`) is the bridge *back* to the holdings
  surface — "I bought this" — and it is the collection importer's `merge`
  (`collection_import::merge_holdings`, the provider-less half of `reconcile_holdings`), never a
  loop over the per-card `PUT`: the foil-★ fold, the by-card aggregation and the one-transaction
  apply are stated once. It is **additive and not idempotent on purpose** (a second call is a
  second copy of the deck; the SPA's `AddToCollectionButton` confirms first — `docs/tradeoffs.md`
  §Decks has the rejected alternatives), skips a deck's maybeboards (the `needed`/`summary`
  split — the public-deck route through the same `deck_rows` read as the owner's), takes every
  board of a precon, and all three entry points share
  `decks::to_collection::add_rows_to_collection` — a fourth source belongs there, and lands in
  the per-user **`Import`** rate-limit class (`ratelimit/per_user.rs`'s `from_path` keys on the
  `…/collection` tail; its classification test pins both paths), because it does the import's work.
  Deck **import/export** (issue #389) lives in the sibling
  `deck_import/` pipeline: categories/boards become exact sections and a new deck is written
  whole, never through the `collection_items` reconcile engine. It reuses the lower provider
  throttling, foil, and card-resolution seams; imports are capped at 2000 source rows and return
  a lightweight deck header; Moxfield live URLs keep the collection import gate.

## Preconstructed decks

- **Preconstructed decks** (`/api/games/{game}/precons*`) are the **catalog** side of the deck
  idea, not a second user surface: rows derived from MTGJSON's per-set `decks[]` during the
  sealed sync (`mtgjson::precons` — the same fetch, the same parse, and the same
  `model::Indexes` the membership + composition passes use; a fourth copy of any of the three
  would re-walk a 600 MB document for data that already arrived). So the three reads are
  anonymous and live in the router's **`public`** group beside `products`, and the writes —
  copying one into your decks, and adding its cards to your collection — are authed under
  `/api/decks/{game}/precons/{slug}/{copy,collection}`.
  The tables are **rebuilt wholesale** every sync, so a row id is not stable and never reaches
  the wire: **`slug` is the identity**, derived deterministically (sets walked in sorted order,
  numeric suffix on collision) — a change to how it's derived needs a `DERIVATION_VERSION` bump,
  since the sync is otherwise ETag-gated. **A deck upstream has a product for but no card
  list yet rides `mtgjson::precon_overlay`** — a committed file of decklists derived *as if*
  MTGJSON had listed them, merged **after** the real walk so an upstream row always wins the
  base slug. MTGJSON does ship such dangling references: `SLD`'s Secret Lair Commander Deck
  Hatsune Miku declares `contents.deck = [{name: "Hatsune Miku"}]` and `cardCount: 100` with no
  such deck in `decks[]`. An entry **retires itself** — it stands down as soon as a derived row
  in that set carries its TCGplayer product id *or* its name (two tests, because upstream can
  publish a list without the `sealedProductUuids` link, or under a name we didn't predict), so
  the file shrinks by deletion at leisure and never shadows real data. Name the entry what
  upstream will (the dangling reference's own name, not the product's retail one) or the slug
  moves under a live URL when it lands. It's **data, not a second derivation** — an entry becomes
  an ordinary `RawPrecon` and travels the one ingest path — so it needs no facet/pricing work,
  but its content hash rides `DERIVATION_VERSION` (`derivation_version()`), or an ETag-gated sync
  would never pick up an edit. Transcribing one is the careful part: `deck_cards` addresses a
  **printing**, so where the publisher names a set but no collector number, take that set's plain
  printing (black-bordered, non-promo, no frame effects) — the fancy treatments are the
  collector-booster variants, not what a precon ships. The browse tile's facets (`card_count`,
  `color_identity`, `face_card_id`) are folded **at ingest** into columns, by the deck list's
  own colour rule (command zone if there is one, else the mainboard, never the sideboard) —
  a public CDN-cached list must not pay a per-row card scan, and the two must not disagree.
  The tile's **price is the one facet that is a column but *not* ingest's**: card prices move
  on ticks the ETag-gated rebuild doesn't run on, so the rebuild writes `price_cents` NULL and
  `catalog::precon_values::refresh_precon_values` re-folds it from the live prices every sync
  tick + once at boot on the no-sync path (the `cards.folded_onto_id` model — both wirings are
  load-bearing, and no `DERIVATION_VERSION` bump was needed because derivation never computes
  it). It folds the deck proper through the shared `Valuation` — the same fold as the detail's
  `summary` — so the tile's `price_usd` and the page can't disagree, `sort=price` is a plain
  `ORDER BY … NULLS LAST`, and NULL stays "unpriced", never `$0.00`. The deck list's
  `value_usd` is the same idea per user (`deck_values_by_deck`, in the `deck_headers` seam).
  The browse is a set-tile **landing** (`/decks/{game}/precons`, the deck mirror of
  `/cards/{game}`, whose tiles **nest a set's related sub-sets** the way `groupSets` nests them
  there) and **three route shapes**, each offered only the groupings that answer something
  (`?view=`, validated against *that route's* option list — a mode a route hides can't be
  reached by hand-writing the query either): `/precons/all` groups **by set** (default) or not
  at all, since the type split pours every set's decks into ~40 buckets and loses the one
  landmark a precon has; `/precons/sets/{code}` groups **by type** (default) or not at all —
  136 of 295 sets ship more than one deck type, and by-set on one set is a single group;
  and `/precons/sets/{code}?related=1` — the landing's grouped "All N decks" link, spanning the
  set's whole catalog group through the shared `load_group_set_codes` seam — offers all three,
  defaulting to **by type**, because it is the one shape holding several sets. All read through
  **one filter builder** server-side
  (`filtered_query`), so a filter can only change the layout, never the matches; a grouping may
  reorder (by-type leads with the biggest category), which is why the test that pins this
  compares deck *sets*, not sequences. The nav registry carries precons in **Catalog**, not
  "Your library" — published game data, like a card — and it's the one item whose landing
  (`/precons`) and per-game rows (`/decks/{game}/precons`) sit under different prefixes, so both
  come from `lib/precons.ts`'s `preconsPath`.
  A precon row is a **single finish**, and a board may list one printing in **both** (every
  Jumpstart theme, every bundle land pack): two rows by design, since the ingest keys on
  `(card, finish)`. Everything that turns those rows into *deck* rows must therefore **fold by
  card** — `deck_cards` is unique on `(deck_id, card_id, section_id)`, so emitting the pair
  separately isn't a duplicate tile, it's a failed insert and a 500 on the copy. Both sides do
  it (`precons::copy`'s `push_folded`, `web/src/lib/precons.ts`), which is also what makes the
  page and the deck you copy from it show the same counts.
  **Board → section is decided once, in the copy**: the command zone becomes a section named
  exactly `Commander` and the sideboard exactly `Sideboard`, because those spellings are what
  `decks::analysis::rules` reads a deck's zones off, and the mainboard is filed through
  `deck_import::categorize::preset_section` rather than a second copy of that table. A precon
  row is a **single finish** (that's how a decklist reads) and folds into the deck card's
  regular/foil pair. The copy rides `decks::copy`'s `insert_deck_with_cards` seam — both copies
  hold internal card ids already — and only ever sets a `format` the deck *type* states
  (`Commander Deck` → `commander`; a type that states no format gets none, or the page would
  judge a 30-card theme deck against Commander's rules).
  **A Secret Lair drop is not a preconstructed deck** and never reaches this surface: MTGJSON
  files one under a set's `decks[]` because a drop is a fixed card list, but that's a
  *product's contents* — nothing in it is a deck anyone plays. `mtgjson::precons`'
  `NOT_A_DECK_TYPES` drops them **at derivation**, before card resolution and before a slug is
  claimed (a same-named drop walked first would otherwise take the base slug and push the real
  deck onto `-2`), so no count can disagree with a listing — facets, the landing's per-set tile,
  the browse totals and the group headings all count the same rows. They were 712 of 2,986 rows
  and buried `sld`'s 8 real precons. A drop is already modelled properly on the sealed side, as
  a product with `sealed_contents`. Excluding a category is a derivation change, so it needs a
  `DERIVATION_VERSION` bump like any other. The SPA **mirrors** the board vocabulary in
  `web/src/lib/precons.ts` (tests pin both sides, like `lifeLayout.ts`) and adapts boards into
  sections so the precon page renders through the *deck* display engine, not a second one.
  The reader's own holdings ride that page too (issue #707): the "you own N / want N" chips
  come from `composables/useDeckOwnership.ts`, the overlay the owner deck page reads, over the
  adapted entries — a second, authed request that is empty while signed out, never a per-user
  field on the public, CDN-cached precon payload.

## Price alerts and release heads-ups

- **Price alerts** (`/api/alerts*`, issue #525) are **session-only** (`SessionUser` — never an
  API key: the channel settings hold delivery credentials) and **allow-listed out of the
  OpenAPI doc** (an account/session-flow surface, like username/currency). The engine is
  `crate::alerts` (evaluation) + `crate::notifications` (Discord/Telegram dispatch); email
  reuses `email::Emailer`. It's **edge-triggered**: fire once on the rising edge, re-arm when
  the price crosses back — don't make it re-notify every tick. Evaluate against the **live
  price column** on `cards`/`products` (compare in cents via `valuation::price_cents`), not the
  history tables; targets are stored by internal id and are orphan-tolerant. A user-supplied
  Discord webhook URL is **host-allow-listed** (`notifications::validate_discord_webhook_url`)
  at save **and** send, and dispatched over `AppState::notify_http` (**redirects disabled** +
  timeout) — an SSRF guard; don't route it through the redirect-following shared `http`.
  The webhook URL + Telegram bot token are **credentials**: keep them redacted in `Debug`
  (`alert_channel::Model` hand-writes it). Email is **off by default** (`ALERTS_EMAIL_ENABLED`,
  costs money at scale) and additionally needs `RESEND_API_KEY`. The evaluator scales to
  millions of alerts by **keyset-paginating** the armed set (memory is O(batch), never "load
  every alert + target") and **narrowing** each pass by a `since` cursor to alerts whose own
  row or whose target's `updated_at` changed. That narrowing is correct **only** while every
  live price write bumps the target's `updated_at`: the catalog upsert's "changed guard" does
  (bumps on a real datum change), and `scryfall::enrich_foil_variant_prices` was fixed to stamp
  it too (★-variant foil prices arrive only through that path) — **any new writer of
  `cards`/`products.price_usd*` must bump `updated_at` or narrowing silently misses fires.**
  Two more couplings: an undelivered met alert is `touch`ed so the retry-next-pass contract
  survives narrowing, and the `since` cursor advances **only** when `evaluate_all` returns
  `true` (a mid-scan DB error must re-scan, not skip).
- **Release heads-ups** (`crate::release_alerts`, `RELEASE_ALERTS_ENABLED`) are two per-user
  opt-ins on the **same `alert_channels` row** (`sld_release_enabled` / `set_release_enabled`,
  both default **off** — subscriptions, unlike the channel on/off flags that default on) that
  fire a **day-before** heads-up over the shared channel fan-out
  (`notifications::deliver_to_user`, extracted so price alerts and release alerts deliver
  through one path — don't re-fork it). **Edge-triggered via the `release_notifications`
  ledger**, one row per `(user, kind, ref_key)` written **only on successful delivery** (an
  undeliverable heads-up retries next pass, same latch-on-delivery contract as price alerts).
  Dates are **derived, not newly ingested**: a Secret Lair drop's date is the earliest
  `released_at` among its cards grouped by the runtime drop table; a set's is
  `card_sets.released_at`. Regular sets are **one notification per theme** — top-level only
  (`parent_set_code IS NULL`), a curated set-type allow-list, non-digital, never `sld` (drops
  handle that per-drop). **`box` is on that allow-list on purpose:** The Zeta Set (`slz`) is a
  Secret Lair-line release Scryfall filed as its own top-level `box` set — no parent set, no
  cards in `sld` — so the per-drop path (which reads `sld` cards only) never sees it and the
  set path is its only heads-up; the `sld`-parented spin-offs (`slu`, `slc`) stay out via the
  top-level filter, and every other top-level `box` set in the catalog released before 2023.
  **A set the set path admits whose code carries the family's `sl` prefix is a Secret Lair
  release of its own** (`ReleaseKind::SecretLairSet`, MTG only): linked to its own set page and
  delivered to **either** opt-in, worded per recipient — a Secret Lair release to anyone holding
  that opt-in, a new set to a set-only subscriber. A user holding both flags gets it once per
  pass (one `OR` audience query over their single `alert_channels` row) and once ever (the
  ledger is keyed by the release, `set` / code, not by the opt-in that matched). The prefix
  only ever *upgrades* a set that already passed the filters; it never rescues one they drop
  (`slci`).
  Session-only channel settings, like price alerts; the two flags ride the `AlertChannels` DTO,
  so they're already in the OpenAPI `INTENTIONALLY_UNDOCUMENTED` group.
  **What counts as a release is decided once, in `catalog::releases`** (issue #679) — the set
  predicate (`announceable_sets`), the `sl`-prefix upgrade (`is_secret_lair_release`) and the
  per-drop date derivation off `sld` alone (`sld_drops_releasing`) — and **two surfaces read
  it**: the alert engine above, and the public **release calendar**
  (`GET /api/games/{game}/releases?from&to`, `handlers/catalog/releases.rs`; the SPA's
  `/releases/{game}`, a month view). The calendar is the page behind the heads-ups, so it must
  list exactly what they would notify about — a second filter on either side is the bug. It is
  a *fact page*: every date is the catalog's own, nothing per-user rides the read (CDN/ETag
  cached in the public group; the SPA asks for a **month-aligned** window so one URL serves a
  whole month), a set nests the precons and sealed products of its whole catalog **group**
  (root + `parent_set_code` children — a Commander precon lives in the `…c` child), a drop's
  products are attributed through the cards they **contain** (never by name or date — a
  superdrop releases many drops on one day), and nothing on it words a spoiler or a preview.
  The nested `Set` is the `/sets` payload dressed the same way (`has_subtypes`, the folded
  `card_count`), so a set can't publish two counts. The "get a heads-up" button deep-links to
  `/alerts#release-headsups` (`RELEASE_HEADS_UP_ANCHOR` in `lib/releases.ts`, the id the
  alert settings' release section carries; the router scrolls a hash on a new page to its
  element).

## Tools: the life counter

- **Tools** (`/api/tools/{game}/...`) is a *namespace*, not a surface: play aids backed by the
  caller's own rows, grouped so a second tool adds a path segment rather than a new top-level
  route family (the API mirror of the SPA's `/tools` section, placed the way `/keywords` is).
  Today it holds the **life counter** — a container surface like decks (`life_sessions` /
  `life_session_players` / `life_events`), not a holdings twin, so it rides no `makeHoldingApi`.
  A seat and an event have **no `user_id`** (they hang off `session_id`), so every seat/event
  route must `load_session` first — a foreign id is **404, not 403**. Four invariants:
  **(1) a finished session is immutable** — every life/seat/undo write gates on
  `require_active` and answers **409**, because a recorded result already counts towards the
  per-deck record; start a rematch (`from_session_id`) instead. **(2) `life` is written in
  exactly two places** — a tap appends one event and moves the seat by its delta, and an undo
  re-folds the seat's whole chain through the pure `life/replay.rs` fold (which is why the fold
  honours `set` as an absolute and `adjust` as relative, and clamps rather than overflowing);
  nothing else may write it — and that survived the arrival of a **second counter axis** (#595)
  precisely because the other counters got **no column**. **(3) a seat names what was played in one of two mutually exclusive
  ways** — `deck_id` (one of *yours*, which is what builds a record) or `commander_card_id` (for the
  opponents whose deck you'll never have); both at once is a **422**, because a deck already knows
  its commander and the pair would surface as a wrong record rather than an error. **(4) both links
  are FK-less and orphan-tolerant** (the call `price_alerts.card_id` makes) — deleting a played deck,
  or a re-import dropping a card row, must neither fail nor delete history, so reads report the link
  absent and `life/stats.rs` inner-joins `decks` *scoped to the caller*. A **rematch** distinguishes
  a *copied* reference (dropped once it stops resolving, so an old pod stays re-playable) from an
  *explicit* one (still a `404`).
  **Counters beyond life** (#595 — poison/energy/experience, plus commander damage keyed by the
  *source* seat) ride `life_events.counter` + `source_player_id` rather than five more seat columns,
  so invariant 2 holds unchanged: they're folded out of the history by `replay_seat` (one chain per
  `(counter, source)`, each from its own start — `starting_life` for life, `0` for the rest — and
  within its own bounds, where only `life` may go negative), and a commander-damage tap deliberately
  does **not** move the target's life (the player reconciles it, as at a real table). Damage is per
  commander, so 7 from one and 6 from another is never 13 from either — that's what the 21 threshold
  is measured against. The source link is FK-less and orphan-tolerant like invariant 4's pair: a seat
  that leaves cascades away with *its own* events but not with the damage it dealt. Which counters a
  game tracks is `life_sessions.counters` (CSV, defaulted from `format`, `life` implicit and never
  listed); writing an untracked one is a **422**, switching one off keeps its recorded values (the
  SPA shows any counter still holding one), and a lethal threshold (21 damage, 10 poison) only ever
  *suggests* a result — a session must never finish itself, since a recorded result is immutable.
  The `layout` slug vocabulary + the per-count layout and per-seat rotation defaults are
  **mirrored** in `web/src/lib/lifeLayout.ts` with tests pinning both sides — as is the counter
  vocabulary in `web/src/lib/lifeCounters.ts`; a slug added on one
  side only is either rejected by the API or renders as something other than its name. The SPA
  batches taps into **one** committed change per run (`composables/useLifeTaps.ts`, keyed by the
  same `(seat, counter, source)` chain the server folds, so a 7-point commander hit is one row) and
  deliberately does **not** retry a failed commit — a request that failed in transit may still
  have applied, so re-sending could double the loss.

## External ids, shopping lists and exports

- **External ids are per-printing provider data, and a bulk-buy link is rows, not a URL**
  (issues #686/#292). `cards` holds Scryfall's `tcgplayer_id`/`tcgplayer_etched_id` (the TCGCSV
  join key) and, since `m..083`, `multiverse_ids` (comma-joined, one per face), `mtgo_id`,
  `mtgo_foil_id`, `arena_id`, `cardmarket_id` — all provider columns, so `flush_cards`' deny-lists
  stay untouched and a NULL means "no mapping" (an Arena id exists only for an Arena printing).
  They ride **`CardDetailResponse` only**, never the shared `Card`, and are never unioned from a
  folded foil-★ star (a different product). Three readers: the Archidekt export's
  `Multiverse Id`/`MTGO ID` columns (`0` only where the printing has none — Archidekt's own
  default; a foil row takes `mtgo_foil_id` first), the card page's deep links
  (`web/src/lib/buyLinks.ts`: TCGplayer + Cardmarket by product id, Gatherer by multiverse id,
  each falling back to the name search / hidden when the id is absent — never a dead button), and
  the wish list's **shopping list** `GET /api/wishlist/{game}/buy-list`
  (`handlers/shared/buy_list.rs`, the one wish-list read with **no collection twin** — you don't
  buy what you own). That read is the listing's own query through `resolve_holdings_list` +
  `wishlist_query` (so "buy what's on screen" is the filtered grid), capped at 500 card rows with
  the totals + `truncated` on the wire (a holding whose card row is gone counts for neither, so
  `truncated` only ever means the cap), in the per-user **Analytics** rate-limit class like the
  export it is shaped after, and carries the wanted sealed products (by their TCGplayer product
  id) **only on an unfiltered request** — every card filter is a card filter, and the browse grid's
  button passes `cards-only` so a plain `/cards` browse doesn't drag them along either. It answers
  rows with `tcgplayer_id`, not store URLs: the stores are `web/src/lib/bulkBuy.ts` — TCGplayer's
  mass entry (`?c=` rows `{qty}-{productId}` / `{qty} Name [SET] number`, `||`-joined, a format
  read off TCGplayer's own bundle and pinned by the spec; the row cap doesn't bound the *link*, since
  a name-form row is 40–70 encoded characters, so the builder also stops at a byte budget
  (`MASS_ENTRY_URL_BUDGET`) and the note says what it left off) and MTG Mate's decklist search,
  which has **no verifiable URL prefill**, so that option copies the `{qty} Name` list and opens
  the page. The EDHREC reference link slugs `searchName`'s answer, never the printing name — EDHREC
  files a split card under its combined name but every other multi-faced card under its **front
  face**, and a reversible printing's `Okaun // Okaun` would double. The deck shopping list has
  the same read — `GET /api/decks/{game}/needed/buy-list`, the **same `needed_rows` fold** as
  `…/needed` (never a second shortfall computation), shaped through `shared/buy_list.rs`'s
  `card_row_from_model` + `cap_rows` and rendered by the same `BuyListDialog` with a `source`
  plugged in. A third store belongs in that registry; a third *reader* of the ids belongs on
  `CardDetail`; a third shopping list belongs on that seam.
- **Every export is a file-download response through `handlers/shared/download.rs`**
  (`csv_download`/`text_download`) — don't re-roll the Content-Type + Content-Disposition
  pair. The **card-search `.txt` export** (`/api/games/{game}/cards/export` and its
  `.../sets/{code}/cards/export` sibling) is a *public catalog* read that must keep building
  its query from the listing's own builders (`catalog::cards::all_cards_query` /
  `catalog::sets::set_cards_query`) — a second query here means the file can silently
  disagree with the grid it was exported from. It is **uncapped and streamed**: rows drain
  through **one** SeaORM row stream into ~500-line chunks on a bounded channel, so peak
  memory is a chunk, not a result set — and it selects **only** the three columns a line
  needs, not all ~70 (that alone is 12x on a full-catalog drain). **Never hold a DB
  connection while awaiting the client:** SeaORM's row stream owns its `PoolConnection` for
  the stream's life and sea-orm pins SQLite (the default) to *one* connection, so streaming
  a single query to the client let one slow reader take the whole API down. Hence the
  two-phase drain — resolve ids in one query, then re-acquire per chunk — with the send
  happening outside any checked-out connection. Bounded channel *and* connection-free
  awaits; either alone is not enough. Don't give
  the response a size hint (that's what stops `conditional_request_layer` buffering the
  whole thing to compute an `ETag`), and don't turn a mid-stream failure into silence: it
  appends a `#`-comment marker **and** errors the transfer, so a short file is never
  mistaken for a whole one. Otherwise the body stays pure card lines so a paste is clean.
  That whole drain lives in **`handlers/shared/card_export.rs`**, shared with the
  **collection/wish-list card exports** (`/api/{collection,wishlist}/{game}/cards/export`,
  authed + no-store): those build from the twins' own listing builders through
  `resolve_holdings_list` + `narrow_export_statement`, and their `text` lines carry the
  **real held counts** — one line per non-empty finish, foil tagged ` *F*` (the grammar
  `collection_import::text_list` reads back) — where a catalog line is always `1 …`.
  A view served by a **different endpoint** than the one the export reuses must **hide**
  the button rather than hand back a file that isn't the rows on screen — that's why
  `SetView` gates on `!grouped`: `/drops` owns a `?drop=` filter the export can't express,
  and `/subtypes` parses the search's `unique:`/`order:` directives and then *discards*
  them, so a `q=unique:cards` grid shows every printing while the export would fold them.
  The holdings browse views gate the same way (`!grouped`), and additionally swap target
  per mode: held mode exports the holdings listing, show-ghosts mode *is* the catalog
  listing so it exports the public catalog search.

## Cards: foil folds, etched prices and the shared Card DTO

- **A foil-★ variant is one card with its base only when it *is* the same card.** Some
  printings are two Scryfall objects sharing a gameplay identity — nonfoil `sld` `1587` and
  foil `1587★` — and `scryfall::enrich_foil_variant_prices` already copies the star's foil
  price onto the base, so listing both is one card shown twice. But that pairing rule (shared
  with `collection_import::consolidate` and `m…023`) matches **1,627 pairs catalog-wide**, and
  two thirds are 7ed/8ed/9ed/dkm cards whose foil is **black-bordered** where the nonfoil
  is white: right for copying a price, wrong for hiding a row. So the display fold applies a
  strictly narrower test — `foil_variants::same_printed_card`: border, watermark, frame, frame
  effects, full-art, illustration, security stamp and **flavour text** all equal (a 10e premium
  foil prints flavour where its base prints reminder text — a printed difference `ft:` and
  `has:flavor` query directly), and `promo_types` equal except for foil-*treatment* tokens the
  star adds (`rainbowfoil`, `surgefoil`, …). ~500 pairs fold;
  every star a visitor could tell apart from its base keeps its tile, as do an orphan `…★`
  promo, an etched star, and a star whose base is itself foilable. **Never re-derive this in a
  query.** `refresh_foil_variant_folds` decides it once per sync tick into
  `cards.folded_onto_id`, and `handlers::catalog::catalog_cards` — the base query **every**
  card grid must build from — filters `IS NULL`; the correlated `EXISTS` this replaced became a
  *hashed* SubPlan on Postgres that seq-scanned the whole `cards` heap on every catalog page
  and its `COUNT(*)`. Five couplings. It is a **presentation** fold, so the star row stays and
  its Scryfall id keeps resolving by id (detail pages, holdings/deck/alert rows, provider
  imports) — which is why card-by-id lookups, sealed-product contents, the name autocomplete,
  the scan fingerprint index and the sitemap are exempt. The base's `finishes` must stay
  `nonfoil`-exactly (it's the load-bearing half of the pairing rule in all four homes), so
  `is:foil` **and every foil-treatment `is:` leaf** OR in `has_folded_foil_variant` rather than
  the base advertising anything. `DropTable::drop_for` re-tries a miss with a trailing `★` so a
  drop that lists only the star still claims the base. Every published set `card_count` —
  Scryfall's own set-object count, stored verbatim — has the folded rows subtracted through the
  one `FoldedSetCounts` seam, in all four of its readers (`sets::list_sets`, `sets::get_set`,
  the release calendar's nested set in `handlers::catalog::releases`, and the
  collection/wish-list/public tiles via `build_collection_sets`), **floored at zero**
  because a `card_sets` row lagging the cards it counts must publish a stale number, never a
  negative. And a new `cards` column that isn't provider data must be denied in **both** halves
  of `ingest::flush_cards` — the
  `update_columns` list *and* `upsert_changed_guard` — or every sync wipes it and mass-bumps
  `updated_at`, the cursor the price-alert narrowing reads.
- **Etched foil is a first-class *price*, not a holding** (issue #676). `PricesResponse.usd_etched`,
  `card_price_history.price_usd_etched` (`m..081`, snapshotted since, `NULL` before — no backfill,
  a past day's price isn't recoverable) and the `etched` alert finish all read `cards.price_usd_etched`,
  the column the Scryfall map has always written — so an etched alert is unpriced, never priced at
  the foil, when that column is `NULL`. But `collection_items` has no etched bucket: an etched copy is
  held and valued as **foil** until holding lots (#594), which is why the price tile carries that
  note and nothing in the valuation seams reads the etched price. USD only — Scryfall has no
  `eur_etched`; don't invent one. The alert finish vocabulary is `handlers::alerts::validate_finish`
  server-side and `web/src/lib/alertFinishes.ts` client-side, tests pinning both.
- **The shared `Card` DTO is not where per-printing detail goes.** `CardResponse` rides
  every listing (a catalog page is up to 200 rows, CDN/ETag-cached, and the deck/holdings
  payloads carry hundreds more), so print + collector columns — artist, flavour text,
  finishes, frame/border/stamp/promo types, Reserved List, produced mana, a Battle's
  defense, the EDHREC/Penny ranks — live on `CardDetailResponse` (ts `CardDetail`, issue
  #673), which `#[serde(flatten)]`s `Card` and is returned by the **single-card route
  alone**, so a client typed against `Card` keeps working. Each field is the column as
  stored — except that `finishes` + `promo_types` **union in a folded foil-★ star's**
  (`CardDetailResponse::with_folded_variants`, fed by a `folded_onto_id` probe in
  `get_card`): the base's stored `finishes` must stay `nonfoil`-exactly (the pairing rule),
  yet its page is the only page the folded star has and carries the star's foil price, so
  the stored column alone would say "Regular only" beside a foil price. A NULL provider
  boolean reads as `false`, a NULL comma-joined column as `[]`.

## Collection import

- A replace-mode import matching **zero** catalog cards is refused (wipe guard). Every
  collection import is **one-off** — there is no saved link and no re-sync (the
  `collection_sources` table and the incremental "smart" sync went with them, `m..072`),
  so an import always states its own provider, source, and mode.
  Moxfield **URL** import is deliberately disabled
  (`Provider::network_import_enabled()` is the switch; CSV is the supported path) —
  a 422 there is not a regression.
- **Not every `Provider` fetches.** Mythic Tools (issue #572) and ManaBox (issue #669) have
  no public API, so they're file/paste-only: `network_import_enabled()` is `false` and every
  fetch/link path gates on that before dispatching. Their collections arrive through
  `execute_file_import`, which backs **both** `/import/csv` (upload) and `/import/text`
  (paste) and **sniffs** the format from the content — Mythic Tools CSV (its `Amount` column
  is the fingerprint), ManaBox CSV (its `ManaBox ID` column), **both checked before
  Archidekt's Scryfall ID** because both exports carry one too (read as Archidekt, a ManaBox
  file was refused for spelling its finish column `Foil`, not `Finish`), then Archidekt CSV,
  Moxfield CSV, then a plain-text card list as the fallback. Mythic Tools and ManaBox are one
  parser (`csv_import::parse_id_or_pair_rows`, a `HybridShape` per app) — a fifth
  id-else-set+number export is a third `HybridShape`, not a third copy of the loop. Keep the
  text list *last* or a real CSV silently degrades into it. That line grammar lives in
  `collection_import::text_list` and is **shared with `deck_import::parser`** — extend the
  seam, don't fork a second dialect. A text line naming no printing resolves to the newest
  printing of that name
  (`reconcile::resolve_newest_printing_by_name`, also shared with deck import) — which must keep
  **excluding foil-`…★` variants**, or `4 Sol Ring` silently imports as four *foils* (the star
  shares its base's name and date, wins the id tie-break, and `consolidate` folds it on as foil).
  A line that *did* name a printing must stay unmatched when it doesn't resolve — never fall back
  to another art at another price. A Mythic Tools CSV must carry a `Finish` column (its export
  columns are user-selectable), same refusal Moxfield's `Foil` column gets — and ManaBox's
  `Foil` column too.

## Search and indexes

- **The universal search (`GET /api/games/{game}/search`, the homepage box) is a composition, not a sixth
  search.** Each leg reaches its surface through the seam that surface already exposes
  (`catalog::search_cards` over `card_name_search_query`, `catalog::search_sets` dressed through the set
  list's own `has_subtypes` + folded-count seams, `catalog::search_products`,
  `precons::search_precons` over the browse's own `filtered_query`, `catalog::keywords::search`), and all
  five answer the sealed/precon lists' name rule — every whitespace-separated word an order-independent,
  case-insensitive substring (`shared::every_word_matches[_with]`), prefix matches first
  (`starts_with_rank`) — **never the Scryfall grammar**, which would turn a colon in a card name into a
  422 for every group at once (the full grammar is one Enter away on the card listing). **The card leg
  widens that rule by one clause (issue #709):** a word the name lacks may name the printing's **set**
  instead — a whole set code (`cmr`) or part of a set name (`legends`) — or, carrying a digit, be its
  **collector number within a set another word names** (`cmr 129`, how a checklist spells a
  printing) — and, because the filter runs before the fold, the representative printing is then from
  that set (`sol ring cmr` answers the CMR Sol Ring; the SPA's `cardSublabel` names the set and number
  under the row exactly when a word matched by set, read off the same mirrored name rule). Three guards
  are load-bearing. **A term that is itself a set name gets the plain name rule**
  (`shared::term_names_a_set`: every word a substring of one set name, any length, or the term a
  code) — otherwise `bloomburrow`, or `oath of the gatewatch` through its too-short-to-resolve `of`,
  would answer a handful of arbitrary cards from the set, which is the sets group's question; past
  that gate no "at least one word in the name" clause is needed, since a row every word explains by
  set is exactly a term naming a set, and a set-and-number pair is meant. **Name-alone matches rank
  first** (`NameOrSetMatch::by_name_alone_rank`: a set word is any substring of a set name, so
  `sol ring` also admits a Lord of the *Rings* "Sol…" card — the widened rule may only ever *append*
  to what the plain name rule answers, never reorder it), then `leading_words_rank` (the prefix rank
  split by how many leading words the name starts with — a set word at the end of the term must not
  let the alphabet put "Parasol Ring" above "Sol Ring"); the other legs rank by `starts_with_rank`.
  And **the SQL is bounded and indexed**: the set half is resolved in Rust (`shared::set_codes_matching`
  over the `set_name_map` the handler already loads for the product/precon legs — a whole code at any
  length, a name substring only from three characters, or `a`/`of` would name most of the catalog;
  words deduplicated; a word naming more than `MAX_SET_CODES_PER_WORD` sets names none) into bound
  `set_code IN (…)` lists the `(game, set_code)` index answers, and the number arm — at most
  `MAX_NUMBER_WORDS` digit words get one — compares the **raw** `collector_number` (as typed, lower,
  upper) *inside* that list so `m..024`'s composite seeks it; never a `LIKE` on `cards.set_name` (no
  index), a `LOWER(collector_number)` (no expression index — a heap recheck over every named set), or
  a subquery inside the `OR`, any of which takes the per-keystroke read onto the `cards` heap. Every
  list is bind parameters and the Postgres fold repeats the filter, so a unit test pins the worst
  term under both backends' limits, and the SQL-shape canaries in `handlers/catalog/tests.rs` pin the
  rest. The sets leg also answers the **whole term as a set code**, and dresses its rows through
  `sets::dress_set` (the seam the set list and the release calendar share) over the *bounded*
  `sets_with_subtypes_in` / `folded_counts_in_sets` forms, never the list's whole-game scans. The card leg folds
  **one row per name** through `fold_unique_by` (the engine behind `unique:cards`) and spells its `LIKE` as
  `indexed_name_like`, the exact expression Postgres's `idx_cards_name_trgm` is built on — a new per-keystroke
  name read must use that spelling or it seq-scans `cards`. `has_more` is one row of over-fetch, never a
  `COUNT(*)`. The response is **the same for every visitor** (public, CDN/ETag-cached, per-IP limited like
  the autocomplete): the signed-in user's decks join **client-side** in `composables/useUniversalSearch.ts`
  from the cached deck list, filtered by `lib/universalSearch.ts`'s mirror of that name rule — a per-user
  row must never ride the public read, and the deck query stays gated on a typed term so landing on the
  homepage fetches nothing per-user.
- **A search leaf that joins another table needs an index on *both* sides.** The catalog
  listing pairs a filter with `ORDER BY name, set_code, collector_number_int, id` + `LIMIT`,
  and Postgres will happily answer that by walking `idx_cards_game_name` in sort order,
  applying the filter per row, betting the page fills early. For a *selective* filter that
  bet loses and it walks the whole game partition: the `art:` leaf's semi-join
  (`scryfall::search::compile::tags`) was indexed on `card_art_tags` (`m..063`, `m..066`)
  but not on `cards`, and one page fetch took **86 s** in production (fixed by `m..068`,
  which measures it). The planner only stops making that bet when it has a way to drive
  from the *other* side, so a new leaf that probes a second table lands with the matching
  `cards` index in the same change. Same shape one level down: `deck_cards` had no index
  leading with `section_id`, which is all `decks::facets` selects on (`m..069`).
  **A leaf on a `cards` expression has the mirror rule:** Postgres uses an expression index
  only while the query renders the identical expression, so the leaf and the index are built
  from **one seam** — `kw:` rides `m..078`'s trgm index on `array_member_expr` (drift canary in
  `search/tests.rs`), as `name:`/`t:`/`o:` ride `m..027`'s — never a second copy of the string.
  The seam keeps a *fresh* build in step; a deployed Postgres keeps the index it already built,
  so changing the expression needs a **new migration that drops and recreates the index** (the
  canary pins the literal so the change can't ship without one).
  And a `Page`'s `total` is a second full pass over the filter: a surface that shows a handful
  of rows and never the count reads `/cards/preview` (`SearchGroup`, over-fetched `has_more`,
  no `COUNT(*)`) — the keyword glossary paid a 5 s scan per page for a number in a button label.

## Images, Secret Lair drops and catalog ingest

- Card images are cached lazily on first view — self-hosts **never bulk-download**;
  image fetches are host-allow-listed with redirects disabled. **Don't add any bulk
  image path** — the one sanctioned exception is the opt-in fingerprint build
  (`FINGERPRINT_BUILD_ENABLED`, default off); read `docs/tradeoffs.md` §Visual card
  scanner before touching it.
- **Secret Lair drop titles are a runtime overlay, not just a static file.** They aren't in
  the bulk card API, so `scryfall::drops` is a swappable `RwLock<Arc<Tables>>` **seeded** by the
  committed `scryfall/sld_drops.json` (still the offline fallback; `gen-sld-drops.mjs` regenerates
  it) and **swapped** daily: the mirror origin (`MIRROR_ENABLED`) scrapes Scryfall's galleries
  (`scryfall::sld_scrape`) and serves it at `/api/mirror/scryfall/sld-drops`; every other instance
  imports it from the mirror (`scryfall::sld_sync`, `SLD_DROPS_IMPORT_ENABLED`, default on) — a
  self-host **never scrapes Scryfall itself**. **The galleries are a registry, not one page**
  (`drops::GALLERY_SETS` — set code + the **noun** its sections go by — mirrored by the script's
  `SETS`): Scryfall files The Zeta Set (`slz`, the Secret Lair release above) under three
  print-treatment sections — Photocopy / Photocopy Negatives / Color Banding — that its card
  data doesn't distinguish (every printing is black-bordered, full-art, nonfoil, no promo type),
  so the set page showed one "Full Art" group until its gallery joined the scrape; a future
  Secret Lair set filed the same way is one registry entry, one script entry, and a
  `gen-sld-drops.mjs <code>` run to seed it. **A section is called what the registry says**:
  `drop_noun` (`drop` / `treatment`) rides `Set`, `CollectionSet` and `Card`, and the SPA builds
  every group label from it (`useSetGrouping.dropNoun`, `CardMetaList`) — never the literal word
  "drop", which would present a print treatment as a product. `sld` is the **primary** set — its
  failure fails the run, and `install_snapshot` **rejects a snapshot missing the `mtg/sld`
  set** — so a broken scrape can't wipe the good table; a **secondary** set's failure keeps that
  set's last-good table (`sld_scrape::resolve_set`: the store's, else the committed seed's via
  `drops::seed_table`, so an upgraded instance whose persisted snapshot predates the set still
  carries it), never an omission, which would flap the content version — and the run is recorded
  as carried forward (`Scrape::carried_forward` → the `ingest_state` detail), not as clean. The
  sealed-contents derivation reads **only `sld`** and its version gate hashes **only that table**
  (`DropTable::content_version`, not the snapshot's `content_version`, which is the mirror
  `ETag`'s) — a Zeta change must never re-walk `AllPrintings`; the per-drop release heads-ups
  read only `sld` too (`release_alerts`): a Zeta section is a treatment, not a product or a
  separately-dated release. Each successful scrape/import is
  **persisted** to the DB (`scryfall::sld_persist`, the `sld_drop_snapshot` singleton table) and the
  store is **reseeded from that persisted snapshot at boot** (in `scryfall::sld_tasks`, before the
  `initial_delay` deferral) — so a restart serves the last-good drops, not the committed seed, and
  the deferral/`304` stay correct; the committed file is only the first-boot/offline fallback. Don't
  drop the reseed-before-defer ordering or persist on a `304`/`Unchanged`. `drops::table()` returns an
  owned `Arc<DropTable>`, and `sld::derivation_version` reads the **live** snapshot (computed, not
  memoised) so a refresh propagates to the sealed-contents gate — keep both dynamic.
- **Every field of a Scryfall bulk-catalog entry is optional, and the files are gzipped
  JSONL.** `cards`, `rulings` and `art_tags` all start from the one `/bulk-data` document, so
  a *required* field there is a single point of failure for the whole catalog: when upstream
  swapped `download_uri`/`size` for `jsonl_download_uri`/`compressed_size` (2026-07), serde
  rejected the list and every import died as "network error contacting the card-data source"
  — silently, since existing rows just went stale. Read the location through
  `BulkData::file_url`/`transfer_size`, never the fields. The files are served
  `Content-Type: application/gzip` with **no `Content-Encoding`** (so reqwest's transparent
  gzip never applies) and the mirror passes those bytes through compressed: `client::json_lines`
  is the one seam that sniffs the gzip magic byte and inflates, and every bulk consumer must
  read through it rather than wrapping the stream itself. `cargo test -- --ignored
  live_bulk_catalog` is the manual canary when an import starts failing with a decode error.
  Those three consumers share **one fetch per tick**: `catalog::refresh_all` builds a
  `client::BulkCatalog` and lends it to each, so a fourth dataset takes the borrow and reads
  its entry via `BulkCatalog::entry` rather than adding a fourth identical request (they were
  three, and each was its own chance for a transport blip to cost *that* dataset a full
  `SYNC_INTERVAL_HOURS`). A failed catalog fetch skips all three and deliberately writes **no**
  `ingest_state` error: no import was attempted, so each dataset stays legitimately `complete`
  at its current version and the next tick's gate short-circuits instead of forcing a needless
  re-download — a `mark_error` there would defeat the gate, since every version gate tests
  `status == "complete"` *first*. Nothing in this path retries, and reqwest's own retry is
  compiled out (`is_retryable_error` has bodies only under the `http2`/`http3` features, which
  `Cargo.toml` doesn't enable), so a single lost packet is a lost sync interval.
- **The card-sync tick self-heals — keep all four legs** (the 2026-08 outage: a stranded
  `CARD_SYNC` advisory lock plus a boot-anchored 24h ticker cost a week of daily price
  snapshots). (1) The schedule anchors to the last *completed* tick persisted in
  `ingest_state` (`(all, sync_tick)` via `catalog::sync_state` — the status route pins to the
  card-data dataset, so bookkeeping rows there never surface); a skipped, errored, or
  timed-out attempt retries in minutes-to-an-hour, never a full `SYNC_INTERVAL_HOURS`. (2) The
  tick body is bounded by `SYNC_TICK_DEADLINE`, so a wedged await can't hold the leader lock
  forever. (3) `capture_snapshots` runs **before** `refresh_all` and `snapshot_all` after it,
  so a killed or hung import can't cost the day's history row. (4) A lock lease turns on
  server-side TCP keepalives (a holder that dies without closing its socket is reaped in
  ~2 min) and a lost `try_acquire` logs the holder from `pg_locks` — a `state=idle`, days-old
  holder is a zombie for `pg_terminate_backend`. `record_completed` **only when every provider
  pass succeeded** (`refresh_all`'s flag): stamping an errored or timed-out tick would freeze
  the catalog for a full interval with restarts as no-ops, since the boot deferral reads that
  stamp. The TCGCSV backfill is **gap-aware** (issue #655): every run re-plans from per-day
  `GROUP BY` row counts over both history tables (`tcgcsv::gaps`) and walks exactly the missing
  days — zero-count, or under a quarter of the median nonzero day; the threshold is lenient on
  purpose, since a healthy TCGCSV-filled day counts low (USD-only, TCGplayer-joinable entities
  only) and must not re-fetch forever. It's spawned every boot, a no-gap run is two count
  queries, and its `ingest_state` row is observability only (no completion gate, no resume
  cursor — a filled day stops being a gap). It still holds its own `PRICE_BACKFILL` lock for
  the walk's whole duration, or concurrent boots would double-walk the same gap days.
- `SEED_DUMMY_DATA` is upsert-only — point it at a fresh/dedicated DB.
- Dep pins: `jsonwebtoken` keeps `default-features = false` with exactly one crypto
  provider (`aws_lc_rs`, shared with rustls); enabling no provider panics and enabling
  both providers requires manual selection. `reqwest` deliberately has **no** overall
  timeout (streaming bulk downloads) — don't "fix" that when bumping.

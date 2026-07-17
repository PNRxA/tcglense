// Regenerates `api/src/scryfall/subtype_overrides.json` — the committed snapshot of
// curated card-treatment overrides layered on top of `subtypes::classify` (issue #315,
// follow-up to #282). Most treatments (Borderless / Showcase / Extended Art / Full Art)
// are DERIVED from the bulk card data and need no entry here. This file is the escape
// hatch for the one treatment Scryfall's bulk data does NOT mark: panoramic **Borderless
// Scene** cards, whose artwork tiles edge-to-edge across a run of cards into one larger
// illustration. They carry no distinguishing promo_type/frame_effect, so they'd otherwise
// fall into the generic "Borderless" bucket.
//
// Two-part sourcing, because "is this a scene?" is a judgment the data can't answer:
//   1. CURATED  — the `SCENES` table below names the sets (and, where only part of a set
//      tiles, the exact collector-number runs) that were VISUALLY confirmed to form a
//      connected panorama. Scryfall's community `panorama` illustration tag is a good
//      candidate filter but is noisy: it also fires on standalone wide-vista full-arts
//      (e.g. all of Unstable's Contraptions, Marvel/Avatar character portraits), so each
//      set was eyeballed and non-tiling cards dropped.
//   2. FETCHED  — the exact collector numbers come from Scryfall's search API
//      (`arttag:panorama border:borderless`, scoped per set), never hand-transcribed. For
//      a 'list' set we still cross-check the curated numbers against what Scryfall returns
//      and warn on drift, so a renumber or a new printing surfaces on the next run.
//
// Re-run after validating a new scene set (add it to `SCENES` first):
//
//   node api/scripts/gen-subtype-overrides.mjs
//
// Requires Node 18+ (global fetch). No npm dependencies.

import { writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const GAME = 'mtg'
const SUBTYPE = 'borderless-scene'
const OUT = join(
  dirname(fileURLToPath(import.meta.url)),
  '..',
  'src',
  'scryfall',
  'subtype_overrides.json',
)

// Visually-validated Borderless Scene sets. `all` = every `arttag:panorama border:borderless`
// card in the set tiles into a scene; `list` = only these consecutive runs tile (the set's
// other panorama-tagged borderless cards are self-contained full-arts).
const SCENES = [
  { set: 'acr', mode: 'all' }, //  Assassin's Creed — one continuous Florence panorama
  { set: 'ltr', mode: 'all' }, //  LOTR — seven single-artist panoramas (Shire … Mount Doom)
  { set: 'ltc', mode: 'all' }, //  LOTR Commander — four panoramas + their alt-treatment reprints
  { set: 'pltr', mode: 'all' }, // LOTR promos — the Shire birthday-party panorama
  { set: 'spe', mode: 'all' }, //  Spider-Man Eternal — one graffiti-alley panorama
  // FFXV campsite (the set's other three cycles are standalone character full-arts).
  { set: 'fic', mode: 'list', collector_numbers: ['460', '461', '462', '463', '464', '465'] },
  // Three SLD panoramas: Tiurina city crowd (226–230), Briclot Eldrazi (1151–1154),
  // Kelogsloops Island seascape (2144–2147). The Transformers portraits (1079–1081) don't tile.
  {
    set: 'sld',
    mode: 'list',
    collector_numbers: [
      '226', '227', '228', '229', '230',
      '1151', '1152', '1153', '1154',
      '2144', '2145', '2146', '2147',
    ],
  },
  // TMNT Foot Clan battlefield (the vignette and stained-glass runs don't tile).
  { set: 'tmt', mode: 'list', collector_numbers: ['209', '210', '211', '212', '213', '214'] },
]

const SCRYFALL = 'https://api.scryfall.com/cards/search'
const HEADERS = {
  // Scryfall requires both a descriptive User-Agent and an Accept header.
  'User-Agent': 'TCGLense-dev-tooling/1.0 (subtype override snapshot generator)',
  Accept: 'application/json',
}

// Sort collector numbers by their leading-digit run then suffix ("12a" < "12b" < "100"),
// matching how the catalog reads them (see `map::leading_int`).
function natCompare(a, b) {
  const ma = a.match(/^(\d+)(.*)$/)
  const mb = b.match(/^(\d+)(.*)$/)
  if (ma && mb) {
    const d = Number(ma[1]) - Number(mb[1])
    return d !== 0 ? d : ma[2].localeCompare(mb[2])
  }
  return a.localeCompare(b)
}

async function sleep(ms) {
  await new Promise((r) => setTimeout(r, ms))
}

// Every distinct collector number for a set's panorama-tagged borderless cards.
async function fetchSceneNumbers(set) {
  const numbers = new Set()
  let url = `${SCRYFALL}?q=${encodeURIComponent(`set:${set} arttag:panorama border:borderless`)}&unique=prints`
  while (url) {
    const res = await fetch(url, { headers: HEADERS })
    if (res.status === 404) return numbers // Scryfall 404s an empty result set.
    if (!res.ok) throw new Error(`GET ${url} -> HTTP ${res.status}`)
    const page = await res.json()
    for (const card of page.data ?? []) numbers.add(card.collector_number)
    url = page.next_page ?? null
    if (url) await sleep(100) // Scryfall asks for ~100ms between requests.
  }
  return numbers
}

async function main() {
  const sets = []
  for (const scene of SCENES) {
    const available = await fetchSceneNumbers(scene.set)
    let numbers
    if (scene.mode === 'all') {
      numbers = [...available]
      if (numbers.length === 0) {
        throw new Error(`set '${scene.set}' returned no panorama-tagged borderless cards`)
      }
    } else {
      numbers = scene.collector_numbers
      const drifted = numbers.filter((cn) => !available.has(cn))
      if (drifted.length) {
        console.warn(
          `WARN ${scene.set}: curated numbers not in Scryfall's panorama set — ${drifted.join(', ')}`,
        )
      }
    }
    numbers.sort(natCompare)
    sets.push({ game: GAME, set: scene.set, numbers })
  }

  const setBlocks = sets
    .map(
      (s) =>
        `    {\n      "game": "${s.game}",\n      "set": "${s.set}",\n      "entries": [\n` +
        `        ${JSON.stringify({ subtype: SUBTYPE, collector_numbers: s.numbers })}\n` +
        `      ]\n    }`,
    )
    .join(',\n')

  const json = `{
  "//": "GENERATED by api/scripts/gen-subtype-overrides.mjs — do not edit by hand. Curated 'Borderless Scene' overrides (issue #315): the scene SETS are visually validated (see the script), the collector numbers are fetched from Scryfall's 'arttag:panorama border:borderless' tag.",
  "//2": "Shape: sets[].entries[] = { subtype: <slug from subtypes::CURATED>, collector_numbers: [...] }, joined to a card by (game, set, collector_number) exactly like sld_drops.json. An override wins over the derived classification.",
  "sets": [
${setBlocks}
  ]
}
`

  writeFileSync(OUT, json)
  const total = sets.reduce((n, s) => n + s.numbers.length, 0)
  console.log(`wrote ${sets.length} sets, ${total} scene cards\n -> ${OUT}`)
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})

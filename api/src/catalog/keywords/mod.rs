//! Rules-keyword glossary: what "Vigilance", "Proliferate" or "Landfall" actually do.
//!
//! Card text names its mechanics but only *sometimes* prints reminder text for them —
//! a common printing of a keyword spells it out, the rare one doesn't, and reprints
//! drop it once a mechanic is well known. So the explanation can't come from the card
//! rows: it lives here, as a curated per-game table, and is served over
//! `GET /api/games/{game}/keywords` ([`crate::handlers::catalog::list_keywords`]) so
//! the SPA's inline tooltips and its glossary pages read one authority instead of each
//! shipping their own copy of the rules.
//!
//! Adding a game = a `mod` beside [`mtg`] and one arm in [`glossary`]; everything
//! downstream (the endpoint, the sitemap, the SPA) is already generic over `game`.
//!
//! ## Why the table is static Rust, not a database table
//!
//! Keyword text changes when *Wizards* changes it (a comprehensive-rules update), not
//! when the catalog syncs — so it belongs to the deploy, exactly like
//! [`crate::catalog::GAMES`]. Keeping it out of the DB also means the glossary answers
//! identically on a fresh self-host with an empty catalog, and the sitemap can list
//! every keyword URL without a query.
//!
//! ## Slugs
//!
//! [`KeywordEntry::slug`] is **derived here and put on the wire**, so the Rust sitemap
//! and the SPA's router consume the same string and can never disagree about what
//! `/keywords/first-strike` points at. See [`slugify`].

pub mod mtg;

use std::sync::LazyLock;

use serde::Serialize;

/// What kind of rules term an entry is. The three behave differently enough that the
/// UI labels them apart: an **ability** is a rules-bearing keyword a permanent or spell
/// *has* (Flying, Ward), an **action** is a verb the rules define (Scry, Proliferate),
/// and an **ability word** is an italic flavour label with no rules meaning of its own
/// (Landfall, Metalcraft) that merely marks a recurring pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum KeywordKind {
    Ability,
    Action,
    AbilityWord,
}

/// How safely a keyword's *name* can be spotted inside a card's rules text.
///
/// Plenty of keyword names are also ordinary English words or generic rules plumbing,
/// and card text uses them in that other sense constantly — "put a +1/+1 **counter**",
/// "**Create** a Treasure token", a creature named "**Storm** Crow". Matching every
/// name everywhere would underline half of every card and would sometimes be flatly
/// wrong, so each entry carries the rule its own name needs. The SPA's matcher reads
/// this rather than guessing from [`KeywordKind`] — the two don't line up (an ability
/// word is always printed in keyword position; plenty of keyword *abilities* are
/// ordinary words).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum MatchMode {
    /// Distinctive rules jargon — "Vigilance", "Proliferate", "Annihilator". There is
    /// no realistic sentence where the word means something else, so match it as a
    /// whole word anywhere in the text.
    Anywhere,
    /// A real keyword whose name is also an everyday word — "Fear", "Storm", "Crew".
    /// Match it only in *keyword position*: heading an ability line or following a
    /// comma in a leading keyword run, and followed by something that ends the run
    /// (line end, comma, period, em dash, a `{` cost, or a digit).
    AbilityLine,
    /// Glossary-only. The word is rules plumbing that appears, in its ordinary sense,
    /// on a large fraction of all cards ("destroy", "tap", "sacrifice"): its page is
    /// still worth having, but linking it inline would be noise on nearly every card.
    Never,
}

/// One glossary entry, as the API serves it.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct KeywordEntry {
    /// The keyword spelled as card text spells it, e.g. `"First strike"`, `"Ward"`.
    /// This is what the SPA matches against a card's rules text.
    pub name: String,
    /// URL slug derived from [`Self::name`] — the `/keywords/{slug}` path segment.
    /// Derived server-side (see the module docs) so sitemap and SPA never drift.
    pub slug: String,
    /// Whether this is a keyword ability, a keyword action, or an ability word.
    pub kind: KeywordKind,
    /// Plain-English explanation — the official reminder text where one exists.
    pub text: String,
    /// Whether the keyword normally carries a value in card text (`Ward {2}`,
    /// `Annihilator 3`, `Kicker {1}{R}`). The SPA notes it on the glossary page, and it
    /// tells the text matcher that a trailing cost is part of the phrase, not a
    /// mismatch.
    pub parameterized: bool,
    /// How the SPA may look for [`Self::name`] in a card's rules text. See
    /// [`MatchMode`].
    pub match_mode: MatchMode,
}

/// A row of a game's static source table. Borrowed `&'static str`s so the table costs
/// nothing until [`glossary`] is first read; the owned [`KeywordEntry`] the API serves
/// is built from it once, at that point.
pub struct Entry {
    pub name: &'static str,
    pub kind: KeywordKind,
    pub text: &'static str,
    pub parameterized: bool,
    pub match_mode: MatchMode,
}

/// Derive a URL slug from a keyword name: lowercase, apostrophes dropped outright
/// (`"Council's dilemma"` -> `councils-dilemma`, not `council-s-dilemma`), every other
/// run of non-alphanumerics collapsed to a single `-`, and no leading/trailing `-`
/// (`"Start your engines!"` -> `start-your-engines`).
///
/// ASCII-only by construction, so the result is safe to drop straight into a URL and
/// into sitemap XML without escaping.
pub fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch == '\'' || ch == '\u{2019}' {
            // Apostrophes join the word rather than splitting it.
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}

/// Build a game's served glossary from its source table: derive each slug and sort by
/// name, so the endpoint's order is stable and alphabetical without the table author
/// having to keep the source rows sorted by hand.
fn build(entries: &'static [Entry]) -> Vec<KeywordEntry> {
    let mut built: Vec<KeywordEntry> = entries
        .iter()
        .map(|entry| KeywordEntry {
            name: entry.name.to_string(),
            slug: slugify(entry.name),
            kind: entry.kind,
            text: entry.text.to_string(),
            parameterized: entry.parameterized,
            match_mode: entry.match_mode,
        })
        .collect();
    built.sort_by(|a, b| a.name.cmp(&b.name));
    built
}

static MTG: LazyLock<Vec<KeywordEntry>> = LazyLock::new(|| build(mtg::ENTRIES));

/// The universal search's keyword leg: the glossary entries whose **name** contains every
/// whitespace-separated word of `term` (case-insensitively, in any order), the ones whose
/// name *starts* with the whole term first and the rest in glossary (name) order, cut at
/// `limit` — plus whether the cut left any behind.
///
/// The same rule the SQL legs apply to card, sealed-product and precon names
/// (`handlers::shared::every_word_matches` + `starts_with_rank`), spelled over the
/// in-memory table so `first strike`, `strike first` and `FIRST` all find "First strike"
/// exactly as they would find a card. Name only, never the reminder `text`: a universal
/// box answers "what is this word", and matching explanations would bury the one keyword
/// called `Draw`… under every keyword whose text says "draw a card". The index page's
/// own filter box still searches both.
///
/// `to_ascii_lowercase`, not Unicode folding, so the fold matches the SQL legs' (SQLite's
/// `LOWER()` is ASCII-only, and the table is ASCII anyway). A blank `term` answers nothing.
pub fn search(game: &str, term: &str, limit: usize) -> (Vec<KeywordEntry>, bool) {
    let words: Vec<String> = term
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    if words.is_empty() {
        return (Vec::new(), false);
    }
    let prefix = term.trim().to_ascii_lowercase();
    let mut hits: Vec<&KeywordEntry> = glossary(game)
        .iter()
        .filter(|entry| {
            let name = entry.name.to_ascii_lowercase();
            words.iter().all(|word| name.contains(word.as_str()))
        })
        .collect();
    // Stable, so within each rank the glossary's own name order is kept.
    hits.sort_by_key(|entry| !entry.name.to_ascii_lowercase().starts_with(&prefix));
    let has_more = hits.len() > limit;
    hits.truncate(limit);
    (hits.into_iter().cloned().collect(), has_more)
}

/// The glossary for a game, name-ordered. A game with no curated table yet returns an
/// empty slice rather than an error — the endpoint then answers `200 []`, which the SPA
/// reads as "this game has no glossary" and simply renders no tooltips.
pub fn glossary(game: &str) -> &'static [KeywordEntry] {
    match game {
        crate::scryfall::GAME => &MTG,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn slugify_matches_the_spa_fixtures() {
        // These exact pairs are asserted again in `web/src/lib/__tests__/keywords.spec.ts`
        // against the TypeScript normaliser, which must stay in step with this one.
        assert_eq!(slugify("Vigilance"), "vigilance");
        assert_eq!(slugify("First strike"), "first-strike");
        assert_eq!(slugify("Council's dilemma"), "councils-dilemma");
        assert_eq!(slugify("Doctor\u{2019}s companion"), "doctors-companion");
        assert_eq!(slugify("Start your engines!"), "start-your-engines");
        assert_eq!(slugify("For Mirrodin!"), "for-mirrodin");
        assert_eq!(slugify("Jump-start"), "jump-start");
        assert_eq!(slugify("The Ring tempts you"), "the-ring-tempts-you");
        assert_eq!(slugify("Hexproof from black"), "hexproof-from-black");
    }

    #[test]
    fn mtg_glossary_is_sorted_and_well_formed() {
        let glossary = glossary(crate::scryfall::GAME);
        assert!(
            glossary.len() > 200,
            "expected the full MTG keyword table, got {}",
            glossary.len()
        );
        let mut previous: Option<&str> = None;
        for entry in glossary {
            assert!(!entry.name.trim().is_empty(), "empty name");
            assert!(
                !entry.slug.is_empty(),
                "'{}' slugified to nothing",
                entry.name
            );
            assert!(
                entry.text.trim().len() > 20,
                "'{}' has no real explanation: {:?}",
                entry.name,
                entry.text
            );
            // A sentence, allowing a closing quote or bracket after the stop — several
            // entries end by quoting the text a card gains ("… change \"target\" to
            // \"each.\"") and must not grow a second full stop outside the quote.
            assert!(
                entry
                    .text
                    .trim_end()
                    .trim_end_matches(['"', '\'', ')', ']'])
                    .ends_with(['.', '!', '?']),
                "'{}' explanation should read as a sentence: {:?}",
                entry.name,
                entry.text
            );
            // Reminder text is stored bare; the SPA supplies its own punctuation.
            assert!(
                !entry.text.starts_with('(') && !entry.text.ends_with(')'),
                "'{}' should not keep the reminder text's parentheses",
                entry.name
            );
            if let Some(previous) = previous {
                assert!(
                    previous <= entry.name.as_str(),
                    "glossary is not name-ordered: {previous:?} before {:?}",
                    entry.name
                );
            }
            previous = Some(&entry.name);
        }
    }

    #[test]
    fn mtg_names_and_slugs_are_unique() {
        // A duplicate slug would make one of the two `/keywords/{slug}` pages
        // unreachable, and a duplicate name would render two tooltips for one word.
        let mut names: HashMap<String, &str> = HashMap::new();
        let mut slugs: HashMap<&str, &str> = HashMap::new();
        for entry in glossary(crate::scryfall::GAME) {
            let lowered = entry.name.to_lowercase();
            if let Some(other) = names.insert(lowered, &entry.name) {
                panic!("duplicate keyword name: {:?} and {other:?}", entry.name);
            }
            if let Some(other) = slugs.insert(&entry.slug, &entry.name) {
                panic!(
                    "keywords {:?} and {other:?} share the slug {:?}",
                    entry.name, entry.slug
                );
            }
        }
    }

    #[test]
    fn ability_words_are_always_anchored() {
        // An ability word is printed in exactly one shape — "Landfall — Whenever …" — and
        // its name is very often an ordinary noun (Domain, Rally, Threshold, Void). Left
        // matchable anywhere, those would light up half the catalog, so the anchored rule
        // isn't a judgement call for this kind: it's the only correct one.
        for entry in glossary(crate::scryfall::GAME) {
            if entry.kind == KeywordKind::AbilityWord {
                assert_eq!(
                    entry.match_mode,
                    MatchMode::AbilityLine,
                    "ability word {:?} must be anchored",
                    entry.name
                );
            }
        }
    }

    #[test]
    fn the_generic_rules_verbs_are_never_matched_inline() {
        // These appear, in their ordinary sense, on a huge share of all cards ("put a
        // +1/+1 counter", "Create a Treasure token"). They keep their glossary pages;
        // marking them in card text would underline something on nearly every line.
        const PLUMBING: &[&str] = &[
            "Activate",
            "Cast",
            "Counter",
            "Create",
            "Destroy",
            "Discard",
            "Exile",
            "Play",
            "Sacrifice",
            "Search",
            "Shuffle",
            "Tap",
            "Untap",
        ];
        for name in PLUMBING {
            let entry = glossary(crate::scryfall::GAME)
                .iter()
                .find(|entry| entry.name == *name)
                .unwrap_or_else(|| panic!("{name} should be in the glossary"));
            assert_eq!(
                entry.match_mode,
                MatchMode::Never,
                "{name} must stay glossary-only"
            );
        }
    }

    #[test]
    fn search_matches_every_word_of_the_name_in_any_order_and_case() {
        let (hits, _) = search("mtg", "strike first", 10);
        assert!(
            hits.iter().any(|e| e.name == "First strike"),
            "order-independent words should find First strike: {:?}",
            hits.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
        let (hits, _) = search("mtg", "FIRST STRIKE", 10);
        assert!(hits.iter().any(|e| e.name == "First strike"));
        // Only the name is searched — a word found solely in reminder text is no hit.
        let (hits, _) = search("mtg", "discard this card", 10);
        assert!(hits.is_empty(), "{:?}", hits);
    }

    #[test]
    fn search_ranks_prefix_matches_first_then_keeps_glossary_order() {
        // "cycling" is a prefix of Cycling and a mere substring of Basic landcycling,
        // Forestcycling, Islandcycling, Landcycling, …: the prefix match leads even though
        // "Basic landcycling" sorts before it alphabetically, and the rest keep name order.
        let (hits, _) = search("mtg", "cycling", 50);
        assert_eq!(hits.first().map(|e| e.name.as_str()), Some("Cycling"));
        let rest: Vec<&str> = hits[1..].iter().map(|e| e.name.as_str()).collect();
        let mut sorted = rest.clone();
        sorted.sort_unstable();
        assert_eq!(
            rest, sorted,
            "non-prefix hits keep the glossary's name order"
        );
        assert!(rest.contains(&"Landcycling"));
    }

    #[test]
    fn search_cuts_at_the_limit_and_reports_the_remainder() {
        let (all, more) = search("mtg", "cycling", 50);
        assert!(all.len() > 2, "the fixture needs several cycling keywords");
        assert!(!more);
        let (cut, more) = search("mtg", "cycling", 2);
        assert_eq!(cut.len(), 2);
        assert!(more, "two of many should report has_more");
        assert_eq!(
            cut[0].name, all[0].name,
            "the cut keeps the same leading order"
        );
    }

    #[test]
    fn search_answers_nothing_for_a_blank_term_or_an_unknown_game() {
        assert_eq!(search("mtg", "   ", 10).0.len(), 0);
        assert_eq!(search("nope", "flying", 10).0.len(), 0);
    }

    #[test]
    fn unknown_game_has_an_empty_glossary() {
        assert!(glossary("pokemon").is_empty());
    }
}

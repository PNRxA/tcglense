//! The **clause grammar** the per-card signals are read with: how a card's rules text is
//! cut into clauses, the land vocabulary, the table-wide / self-scoped / targeted tests, the
//! bounded quantifier scan and the library-search descriptor.
//!
//! Two consumers, one grammar. [`bracket`] holds the four categories Wizards' Commander
//! bracket ladder is written in terms of (issue #596); [`roles`] holds the deckbuilding roles
//! a Commander list is counted in — ramp, draw, removal, wipes, counters, tutors, recursion,
//! protection (issue #671). Both are grammars over the **printed text, not curated lists of
//! ids** (a card printed after this code was written is classified the day the catalog
//! ingests it), both read that text through [`super::rules`]'s `ability_lines`/`has_word`
//! rather than a second copy of them, and both take the same stance: **a predicate declines
//! when it isn't sure**, and every counted card rides the response so the number can be
//! audited. This module is what stops a third copy of "does this clause say X" growing
//! beside the first two — the rule of three, applied before the third arrived.
//!
//! Everything here is `pub(super)`-or-private: the predicates are the surface, the grammar
//! is their shared implementation.

use super::CardFacts;
use super::rules::{ability_lines, has_word};

pub(super) mod bracket;
pub(super) mod roles;

/// Land words a mass-denial effect can name — the type itself plus the five basic land
/// types, so "Destroy all Islands" reads as land denial the same way "Destroy all lands"
/// does.
pub(super) const LAND_WORDS: &[&str] = &[
    "land",
    "lands",
    "plains",
    "island",
    "islands",
    "swamp",
    "swamps",
    "mountain",
    "mountains",
    "forest",
    "forests",
];

/// The plural half of [`LAND_WORDS`]. The untap-denial branch needs it: a card that stops
/// *one* land untapping is describing itself, while one that stops **lands** untapping is
/// Winter Orb. (`plains` is in both lists — it is its own plural.)
const PLURAL_LAND_WORDS: &[&str] = &[
    "lands",
    "plains",
    "islands",
    "swamps",
    "mountains",
    "forests",
];

/// Verbs that remove a permanent from the battlefield en masse. Deliberately *not*
/// "search", "put", or "play": those are how a deck ramps, and every land-fetch effect in
/// the game would otherwise read as land destruction.
pub(super) const MASS_VERBS: &[&str] = &[
    "destroy",
    "destroys",
    "exile",
    "exiles",
    "sacrifice",
    "sacrifices",
    "return",
    "returns",
];

/// Words that may sit between a mass quantifier and the noun it reaches, so
/// "all artifacts, creatures, and lands" (Jokulhaups) still finds its land while
/// "all creatures" stops at the first noun that isn't one. Permanent types are in the list
/// because a wrath that also hits lands spells them out; "nonland" deliberately is **not**,
/// so "destroy all nonland permanents" stops dead.
const TYPE_LIST_WORDS: &[&str] = &[
    "and",
    "or",
    "other",
    "the",
    "basic",
    "nonbasic",
    "non-basic",
    "snow",
    "legendary",
    "tapped",
    "untapped",
    "artifact",
    "artifacts",
    "creature",
    "creatures",
    "enchantment",
    "enchantments",
    "planeswalker",
    "planeswalkers",
    "permanent",
    "permanents",
    "battle",
    "battles",
];

/// How far past a quantifier the scan looks for the noun it governs. Long enough for the
/// longest printed type list, short enough that "all creatures" can't reach a "land" three
/// clauses later in the same sentence.
const QUANTIFIER_SCAN_WORDS: usize = 8;

/// How far into a "search your library …" clause the scan reads when the sentence never
/// says "card" — bounded so a long sentence can't drag an unrelated land word into the
/// descriptor.
const SEARCH_SCAN_CHARS: usize = 80;

/// Rules text as lowercased **clauses**, reminder text stripped: split on sentence stops
/// **and on the colon that separates an activation cost from its effect**.
///
/// Both splits are load-bearing, and the colon is the subtler one. `{T}, Sacrifice a Forest:
/// Untap all lands you control.` is a single sentence whose *cost* supplies a mass verb and
/// whose *effect* supplies "all … lands"; read whole, it is Armageddon, and read as two
/// clauses it is the mana creature it actually is. Every predicate scans a clause, so a
/// cost can never lend its verb to an effect that didn't have one.
pub(super) fn sentences(card: &CardFacts) -> Vec<String> {
    ability_lines(card)
        .iter()
        .flat_map(|line| {
            line.split(['.', ':'])
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(super) fn names_a_land(text: &str) -> bool {
    LAND_WORDS.iter().any(|word| has_word(text, word))
}

pub(super) fn names_lands(text: &str) -> bool {
    PLURAL_LAND_WORDS.iter().any(|word| has_word(text, word))
}

/// Whether the clause puts a table-wide subject in front of its verb — "each player
/// sacrifices four lands", "…during their **controllers'** untap steps". The symmetric
/// spellings only; a clause about *one* player is targeted removal, not mass denial.
pub(super) fn addresses_everyone(sentence: &str) -> bool {
    has_word(sentence, "each player")
        || has_word(sentence, "each opponent")
        || has_word(sentence, "players")
        || has_word(sentence, "controllers")
}

/// Whether the clause is about the caster's **own** permanents. The Amonkhet "Last …" cycle
/// is the reason this exists: "Lands you control don't untap during your next untap step" is
/// a *drawback on a wrath*, and reads word-for-word like Winter Orb to anything that only
/// asks whether "lands" and "don't untap" are both present.
pub(super) fn is_self_scoped(sentence: &str) -> bool {
    sentence.contains("you control") || sentence.contains("you own")
}

/// Whether the clause names a target. A targeted effect is by definition not table-wide, so
/// "Up to three target lands don't untap…" and "Target player can't play lands this turn"
/// are tempo cards rather than the lockdown they otherwise pattern-match.
pub(super) fn is_targeted(sentence: &str) -> bool {
    has_word(sentence, "target")
}

/// Whether the clause denies the **whole table** something: it addresses everyone, and it
/// neither targets nor confines itself to the caster's own side. Every "nobody gets to use
/// their lands" branch gates on this, because those branches read a *restriction* rather
/// than a removal, and a restriction on yourself is a cost you paid.
pub(super) fn denies_the_table(sentence: &str) -> bool {
    addresses_everyone(sentence) && !is_targeted(sentence) && !is_self_scoped(sentence)
}

/// One word of a scanned clause, punctuation trimmed the way the quantifier scan reads it
/// ("creatures," is "creatures"; "non-basic" keeps its hyphen).
fn bare_word(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-')
}

/// Whether one of `quantifiers` ("all ", "each ", …) in this sentence governs one of `nouns`:
/// the quantifier, then within a bounded run of `list_words`, the noun — with the word that
/// follows the noun handed to `accept`, so a caller can tell "all creatures" from "all
/// creature **cards**".
///
/// This is the whole difference between Armageddon and Wrath of God, both of which are
/// "destroy all …" — so the scan stops at the first word that isn't part of a type list
/// rather than looking anywhere in the sentence. Every occurrence of every quantifier is
/// tried, so a noun the caller rejects doesn't hide a later one it would take.
pub(super) fn quantified_noun(
    sentence: &str,
    quantifiers: &[&str],
    list_words: &[&str],
    nouns: &[&str],
    accept: impl Fn(&str, Option<&str>) -> bool,
) -> bool {
    for quantifier in quantifiers {
        let mut from = 0usize;
        while let Some(offset) = sentence[from..].find(quantifier) {
            let start = from + offset;
            let starts_a_word =
                start == 0 || !sentence.as_bytes()[start - 1].is_ascii_alphanumeric();
            if starts_a_word {
                let mut words = sentence[start + quantifier.len()..]
                    .split_whitespace()
                    .take(QUANTIFIER_SCAN_WORDS + 1)
                    .map(bare_word)
                    .peekable();
                let mut scanned = 0usize;
                while let Some(word) = words.next() {
                    scanned += 1;
                    if scanned > QUANTIFIER_SCAN_WORDS {
                        break;
                    }
                    if nouns.contains(&word) {
                        if accept(word, words.peek().copied()) {
                            return true;
                        }
                        break;
                    }
                    if !list_words.contains(&word) {
                        break;
                    }
                }
            }
            from = start + 1;
        }
    }
    false
}

/// Whether a mass quantifier in this sentence governs a **land**: `all` (or `every`)
/// followed, within a bounded run of type-list words, by a land word.
pub(super) fn mass_quantified_land(sentence: &str) -> bool {
    quantified_noun(
        sentence,
        &["all ", "every "],
        TYPE_LIST_WORDS,
        LAND_WORDS,
        |_, _| true,
    )
}

/// What a "search your library …" clause searches **for**: the descriptor from " for " up to
/// and including the first " card" ("for a basic land card", "for a creature card"), or the
/// rest of the bounded clause when it never says "card". `None` when the sentence holds no
/// search, or a search that names nothing.
///
/// "If you search your library this way, shuffle" is a back-reference to a search that
/// already happened, not a second one — and it names nothing, so a descriptor scan would
/// find no land in it and call every land-fetcher a tutor. A real search says what it is
/// *for*, which is why the `for` is required rather than defaulted.
pub(super) fn search_descriptor(sentence: &str) -> Option<String> {
    let index = sentence.find("search your library")?;
    // Bounded by chars, not bytes — oracle text carries em dashes and accents, and a byte
    // slice through one would panic.
    let scanned: String = sentence[index..].chars().take(SEARCH_SCAN_CHARS).collect();
    let target = scanned.find(" for ")?;
    let clause = &scanned[target..];
    Some(match clause.find(" card") {
        Some(end) => clause[..end + " card".len()].to_string(),
        None => clause.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The quantifier scan reads the word after the noun, so a caller can refuse a
    /// "creature **card**" — and a refused noun doesn't hide a later quantifier's.
    #[test]
    fn a_quantified_noun_hands_its_following_word_to_the_caller() {
        let nouns = &["creature", "creatures"];
        let list = &["other", "and"];
        let is_permanent = |_: &str, next: Option<&str>| !matches!(next, Some("card" | "cards"));
        assert!(quantified_noun(
            "destroy all creatures",
            &["all "],
            list,
            nouns,
            is_permanent
        ));
        assert!(quantified_noun(
            "destroy all other creatures",
            &["all "],
            list,
            nouns,
            is_permanent
        ));
        assert!(!quantified_noun(
            "exile all creature cards from all graveyards",
            &["all "],
            list,
            nouns,
            is_permanent
        ));
        assert!(
            quantified_noun(
                "exile all creature cards from all graveyards, then destroy all creatures",
                &["all "],
                list,
                nouns,
                is_permanent
            ),
            "a refused noun must not hide the one after it"
        );
        // A word that is neither a noun nor a list word ends the scan.
        assert!(!quantified_noun(
            "destroy all lands and creatures",
            &["all "],
            list,
            nouns,
            is_permanent
        ));
        // A quantifier inside another word is not a quantifier.
        assert!(!quantified_noun(
            "recall creatures",
            &["all "],
            list,
            nouns,
            is_permanent
        ));
    }

    #[test]
    fn a_search_descriptor_is_what_the_search_is_for() {
        assert_eq!(
            search_descriptor(
                "search your library for a basic land card, put it onto the battlefield"
            )
            .as_deref(),
            Some(" for a basic land card")
        );
        assert_eq!(
            search_descriptor("search your library for a card, then shuffle").as_deref(),
            Some(" for a card")
        );
        assert_eq!(
            search_descriptor("if you search your library this way, shuffle"),
            None,
            "a back-reference names nothing"
        );
        assert_eq!(search_descriptor("draw a card"), None);
    }
}

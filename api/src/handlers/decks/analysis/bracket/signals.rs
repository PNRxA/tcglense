//! The four card categories the bracket ladder is written in terms of, read off a card's
//! own catalog row.
//!
//! Like [`super::super::rules`], this is a **grammar over the printed text, not a curated
//! list of ids** — a card printed after this code was written is classified the day the
//! catalog ingests it, and a self-host never has to sync a side-car file. The one exception
//! is the Game Changers list, which *is* curated — by Wizards, and shipped on the card row
//! as Scryfall's `game_changer` boolean, so it is read rather than reproduced.
//!
//! The stance is the parent module's: **a false positive is worse than a miss.** A category
//! here can push a deck's estimate up a bracket, so every predicate below is written to
//! decline when it isn't sure, and the estimate hands the matched cards back so a player can
//! see exactly what was counted rather than being told a number to trust.

use super::super::CardFacts;
use super::super::rules::{ability_lines, has_word};

/// Land words a mass-denial effect can name — the type itself plus the five basic land
/// types, so "Destroy all Islands" reads as land denial the same way "Destroy all lands"
/// does.
const LAND_WORDS: &[&str] = &[
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
const MASS_VERBS: &[&str] = &[
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

/// Rules text as lowercased **sentences**, reminder text stripped.
///
/// Sentence-at-a-time is what keeps a cost from being read as an effect: `{2}, Sacrifice a
/// land: Destroy all creatures.` is one line but two clauses, and a sentence scan is what
/// stops the "all" of the wrath from binding to the land of the cost.
fn sentences(card: &CardFacts) -> Vec<String> {
    ability_lines(card)
        .iter()
        .flat_map(|line| {
            line.split('.')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn names_a_land(text: &str) -> bool {
    LAND_WORDS.iter().any(|word| has_word(text, word))
}

fn names_lands(text: &str) -> bool {
    PLURAL_LAND_WORDS.iter().any(|word| has_word(text, word))
}

/// Whether the sentence puts a table-wide subject in front of its verb — "each player
/// sacrifices four lands". The symmetric spellings only; a sentence about *one* player is
/// targeted removal, not mass denial.
fn addresses_everyone(sentence: &str) -> bool {
    has_word(sentence, "each player")
        || has_word(sentence, "each opponent")
        || has_word(sentence, "players")
}

/// Whether a mass quantifier in this sentence governs a **land**: `all` (or `every`)
/// followed, within a bounded run of type-list words, by a land word.
///
/// This is the whole difference between Armageddon and Wrath of God, both of which are
/// "destroy all …" — so the scan stops at the first word that isn't part of a type list
/// rather than looking anywhere in the sentence.
fn mass_quantified_land(sentence: &str) -> bool {
    for quantifier in ["all ", "every "] {
        let mut from = 0usize;
        while let Some(offset) = sentence[from..].find(quantifier) {
            let start = from + offset;
            let starts_a_word =
                start == 0 || !sentence.as_bytes()[start - 1].is_ascii_alphanumeric();
            if starts_a_word {
                for word in sentence[start + quantifier.len()..]
                    .split_whitespace()
                    .take(QUANTIFIER_SCAN_WORDS)
                {
                    let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
                    if LAND_WORDS.contains(&word) {
                        return true;
                    }
                    if !TYPE_LIST_WORDS.contains(&word) {
                        break;
                    }
                }
            }
            from = start + 1;
        }
    }
    false
}

/// Whether the card denies everyone their lands: destroying, bouncing, or sacrificing them
/// as a group, or stopping them untapping at all.
///
/// Brackets 1–3 allow none of it, so this is the one category that can move a deck straight
/// to bracket 4 — which is why it declines on every shape that merely *mentions* lands.
/// Cards that put lands **onto the battlefield** or return them **from a graveyard** are
/// ramp, and are excluded by name rather than by hoping the verb list misses them
/// (Splendid Reclamation is "Return all land cards from your graveyard …", which is the
/// exact shape of Sunder's "Return all lands to their owners' hands").
pub(super) fn is_mass_land_denial(card: &CardFacts) -> bool {
    sentences(card).iter().any(|sentence| {
        if !names_a_land(sentence) {
            return false;
        }
        let everyone = addresses_everyone(sentence);

        // Locking lands down is denial without removing anything (Winter Orb, Back to
        // Basics). A *singular* land only counts when the sentence is about the table:
        // "this land doesn't untap during your next untap step" describes one permanent.
        if (names_lands(sentence) || everyone)
            && (sentence.contains("don't untap")
                || sentence.contains("doesn't untap")
                || sentence.contains("can't untap"))
        {
            return true;
        }
        if sentence.contains("can't play lands") {
            return true;
        }

        // Ramp, not denial.
        if sentence.contains("graveyard") || sentence.contains("the battlefield") {
            return false;
        }

        let removes = MASS_VERBS.iter().any(|verb| has_word(sentence, verb));
        removes && (mass_quantified_land(sentence) || (everyone && names_lands(sentence)))
    })
}

/// Whether the card hands somebody an extra turn.
///
/// Both the taking verb and the phrase are required, so a card that merely *cares* about
/// turns ("during each opponent's turn …") is left alone. Matched as a substring rather
/// than a word so "take two extra turns after this one" counts too.
pub(super) fn is_extra_turn(card: &CardFacts) -> bool {
    sentences(card).iter().any(|sentence| {
        sentence.contains("extra turn")
            && (has_word(sentence, "take") || has_word(sentence, "takes"))
    })
}

/// Whether the card is a tutor — it searches your library for something that isn't a land.
///
/// Land fetching is excluded because every deck in every bracket ramps: what the bracket
/// guidance is about is how reliably a deck finds its *best* card. The exclusion reads the
/// search's own descriptor ("search your library for a **basic land** card") rather than
/// the whole sentence, so "search your library for a creature card, then put a land …"
/// still counts.
pub(super) fn is_tutor(card: &CardFacts) -> bool {
    sentences(card).iter().any(|sentence| {
        let Some(index) = sentence.find("search your library") else {
            return false;
        };
        // Bounded by chars, not bytes — oracle text carries em dashes and accents, and a
        // byte slice through one would panic.
        let scanned: String = sentence[index..].chars().take(SEARCH_SCAN_CHARS).collect();
        let descriptor = match scanned.find(" card") {
            Some(end) => &scanned[..end + " card".len()],
            None => scanned.as_str(),
        };
        !names_a_land(descriptor)
    })
}

/// Whether the card is on Wizards' **Game Changers** list, as the catalog stores it.
///
/// A curated list is exactly what the rest of this module avoids, but this one is curated
/// *by the format's rules committee* and published on the card, so reproducing it here
/// would be a second copy that goes stale. A row with no value is `false` — "we have no
/// data" is not "this is a Game Changer".
pub(super) fn is_game_changer(card: &CardFacts) -> bool {
    card.game_changer.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::decks::analysis::test_fixtures::card;

    fn oracle(text: &str) -> CardFacts {
        card("x", "X").oracle(text)
    }

    // ---------- Mass land denial ----------

    #[test]
    fn mass_land_denial_reads_the_armageddon_shapes() {
        for text in [
            "Destroy all lands.",
            "Destroy all nonbasic lands.",
            "Return all lands to their owners' hands.",
            "Destroy all artifacts, creatures, and lands. They can't be regenerated.",
            "Destroy all Islands.",
            "Each player sacrifices four lands.",
            "Each player loses X life, discards X cards, sacrifices X creatures, then sacrifices X lands.",
            "Lands don't untap during their controllers' untap steps.",
            "Nonbasic lands don't untap during their controllers' untap steps.",
        ] {
            assert!(
                is_mass_land_denial(&oracle(text)),
                "should read as mass land denial: {text}"
            );
        }
    }

    /// The near-misses, one per way the grammar could have been sloppy. Every one of these
    /// is a card that belongs in a bracket 2 deck, and flagging it would push that deck two
    /// brackets up.
    #[test]
    fn mass_land_denial_declines_everything_that_merely_mentions_lands() {
        for text in [
            // A wrath is not land destruction, even in the same sentence as a land cost.
            "Destroy all creatures. They can't be regenerated.",
            "{2}, Sacrifice a land: Destroy all creatures.",
            "Destroy all nonland permanents.",
            "Exile all permanents.",
            // Ramp and fetching.
            "Search your library for a basic land card, put it onto the battlefield tapped, then shuffle.",
            "Return all land cards from your graveyard to the battlefield tapped.",
            "Each player may search their library for a land card and put it onto the battlefield.",
            "{T}, Pay 1 life, Sacrifice this land: Search your library for a Forest or Plains card.",
            // Targeted land removal is not mass land denial.
            "{T}: Destroy target land.",
            "Sacrifice a land: You gain 2 life.",
            // A permanent that describes its own untapping.
            "This artifact doesn't untap during your untap step.",
        ] {
            assert!(
                !is_mass_land_denial(&oracle(text)),
                "should NOT read as mass land denial: {text}"
            );
        }
    }

    /// Reminder text is stripped before anything is read, so a parenthetical can neither
    /// create nor hide a match.
    #[test]
    fn mass_land_denial_ignores_reminder_text() {
        let reminder = oracle("Landfall (Whenever a land enters, destroy all lands you control.)");
        assert!(!is_mass_land_denial(&reminder));
    }

    // ---------- Extra turns ----------

    #[test]
    fn extra_turns_need_a_turn_actually_being_taken() {
        assert!(is_extra_turn(&oracle("Take an extra turn after this one.")));
        assert!(is_extra_turn(&oracle(
            "Target player takes an extra turn after this one."
        )));
        assert!(is_extra_turn(&oracle(
            "Take two extra turns after this one. Exile this card."
        )));
        assert!(is_extra_turn(&oracle(
            "Sacrifice five artifacts: Take an extra turn after this one."
        )));
        assert!(!is_extra_turn(&oracle(
            "During each opponent's turn, you may cast this spell."
        )));
        assert!(!is_extra_turn(&oracle("Skip your next combat phase.")));
    }

    // ---------- Tutors ----------

    #[test]
    fn tutors_are_library_searches_that_arent_land_ramp() {
        for text in [
            "Search your library for a card, then shuffle and put that card into your hand.",
            "Search your library for a creature card, reveal it, put it into your hand, then shuffle.",
            "Search your library, graveyard, and hand for a card named Dark Ritual.",
            "Search your library for an artifact card with mana value 3 or less.",
        ] {
            assert!(is_tutor(&oracle(text)), "should read as a tutor: {text}");
        }
        for text in [
            "Search your library for a basic land card, put it onto the battlefield tapped, then shuffle.",
            "Search your library for a land card, reveal it, put it into your hand, then shuffle.",
            "Search your library for up to two basic land cards.",
            "Search your library for a Forest or Plains card and put it onto the battlefield.",
            "Draw a card.",
        ] {
            assert!(
                !is_tutor(&oracle(text)),
                "should NOT read as a tutor: {text}"
            );
        }
    }

    /// A search clause with no "card" in it still gets read, and the bounded scan is what
    /// stops a land three clauses later from disqualifying it.
    #[test]
    fn a_tutor_descriptor_is_read_only_as_far_as_what_is_searched_for() {
        assert!(is_tutor(&oracle(
            "Search your library for a creature card, put it onto the battlefield, then \
             search your library for a land and put it into your hand."
        )));
    }

    // ---------- Game Changers ----------

    #[test]
    fn game_changer_reads_the_catalog_flag_and_absent_data_is_not_one() {
        assert!(is_game_changer(
            &card("g", "Rhystic Study").game_changer(true)
        ));
        assert!(!is_game_changer(
            &card("n", "Llanowar Elves").game_changer(false)
        ));
        assert!(!is_game_changer(&card("u", "Unknown")));
    }
}

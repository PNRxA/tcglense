//! The four card categories the bracket ladder is written in terms of, read off a card's
//! own catalog row.
//!
//! Like [`super::super::rules`], this is a **grammar over the printed text, not a curated
//! list of ids** — a card printed after this code was written is classified the day the
//! catalog ingests it, and a self-host never has to sync a side-car file. The one exception
//! is the Game Changers list, which *is* curated — by Wizards, and shipped on the card row
//! as Scryfall's `game_changer` boolean, so it is read rather than reproduced.
//!
//! The stance is the bracket module's: **a false positive is worse than a miss.** A category
//! here can push a deck's estimate up a bracket, so every predicate below is written to
//! decline when it isn't sure, and the estimate hands the matched cards back so a player can
//! see exactly what was counted rather than being told a number to trust.
//!
//! The clause grammar these predicates scan — [`sentences`], the land vocabulary, the
//! table-wide / self-scoped / targeted tests and the quantifier scan — lives in the parent
//! [`super`] module, shared with the deck-role predicates in [`super::roles`]: both read the
//! same oracle text for the same reason, and "does this clause say X" must mean one thing.

use super::super::CardFacts;
use super::super::rules::has_word;
use super::{
    MASS_VERBS, addresses_everyone, denies_the_table, is_self_scoped, mass_quantified_land,
    names_a_land, names_lands, search_descriptor, sentences,
};

/// Whether the card denies everyone their lands: destroying, bouncing, or sacrificing them
/// as a group, or stopping them untapping at all.
///
/// Brackets 1–3 allow none of it, so this is the one category that can move a deck straight
/// to bracket 4 — which is why it declines on every shape that merely *mentions* lands.
/// Cards that put lands **onto the battlefield** or return them **from a graveyard** are
/// ramp, and are excluded by name rather than by hoping the verb list misses them
/// (Splendid Reclamation is "Return all land cards from your graveyard …", which is the
/// exact shape of Sunder's "Return all lands to their owners' hands").
pub(crate) fn is_mass_land_denial(card: &CardFacts) -> bool {
    sentences(card).iter().any(|sentence| {
        if !names_a_land(sentence) {
            return false;
        }
        let everyone = addresses_everyone(sentence);

        // Locking lands down is denial without removing anything (Winter Orb, Back to
        // Basics). Both lock branches demand the WHOLE TABLE, not merely a plural: a
        // restriction you place on yourself is a cost, and one you place on a single target
        // is tempo — neither is the thing brackets 1 to 3 forbid.
        if denies_the_table(sentence)
            && (sentence.contains("don't untap")
                || sentence.contains("doesn't untap")
                || sentence.contains("can't untap"))
        {
            return true;
        }
        if denies_the_table(sentence) && sentence.contains("can't play lands") {
            return true;
        }

        // Ramp, not denial.
        if sentence.contains("graveyard") || sentence.contains("the battlefield") {
            return false;
        }

        // Blowing up your OWN lands is a price, not a denial: "Sacrifice all Swamps you
        // control" is what a reanimation spell costs. Removal reads the quantifier rather
        // than the subject, so the self-scope test is what keeps the two apart.
        if is_self_scoped(sentence) {
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
pub(crate) fn is_extra_turn(card: &CardFacts) -> bool {
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
/// still counts. The descriptor scan itself is [`search_descriptor`], shared with the ramp
/// role, which reads the same clause for the opposite answer.
pub(crate) fn is_tutor(card: &CardFacts) -> bool {
    sentences(card).iter().any(|sentence| {
        search_descriptor(sentence).is_some_and(|descriptor| !names_a_land(&descriptor))
    })
}

/// Whether the card is on Wizards' **Game Changers** list, as the catalog stores it.
///
/// A curated list is exactly what the rest of this module avoids, but this one is curated
/// *by the format's rules committee* and published on the card, so reproducing it here
/// would be a second copy that goes stale. A row with no value is `false` — "we have no
/// data" is not "this is a Game Changer".
pub(crate) fn is_game_changer(card: &CardFacts) -> bool {
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

    /// The near-misses an adversarial review of this module found, each a real card that a
    /// plural-vs-singular reading of the lock branches called mass land denial. Every one of
    /// them belongs in a bracket 2 deck, and flagging any of them jumps that deck to 4 — the
    /// single most expensive mistake this file can make.
    #[test]
    fn mass_land_denial_declines_a_drawback_a_player_takes_on_themselves() {
        for text in [
            // The Amonkhet "Last …" cycle: a wrath whose price is your own untap step.
            "Destroy all creatures. Lands you control don't untap during your next untap step.",
            "Return target creature card from your graveyard to the battlefield. Lands you control \
             don't untap during your next untap step.",
            // Playing off the top of your library, at the cost of your own land drops.
            "You may look at the top card of your library any time. You can't play lands or cast \
             spells from your hand.",
            "You can't play lands.",
            // Its own controller's Swamps, as the price of the reanimation.
            "Return all Nightstalker creature cards from your graveyard to the battlefield. \
             Sacrifice all Swamps you control.",
            // A cost that supplies the verb and an effect that supplies the quantifier — one
            // sentence, two clauses, and no destruction anywhere.
            "{T}, Sacrifice this creature: Untap all Forests you control.",
        ] {
            assert!(
                !is_mass_land_denial(&oracle(text)),
                "a cost you pay yourself is not denying anyone their lands: {text}"
            );
        }
    }

    /// Targeting one player is tempo, not a table-wide lock — the other half of the same
    /// review's finding.
    #[test]
    fn mass_land_denial_declines_a_single_target() {
        for text in [
            "Lands target player controls don't untap during that player's next untap step.",
            "Creatures and lands target opponent controls don't untap during their next untap step.",
            "Up to three target lands don't untap during their controllers' next untap steps.",
            "Target player can't play lands this turn. Draw a card.",
        ] {
            assert!(
                !is_mass_land_denial(&oracle(text)),
                "one player's lands for one turn is not mass land denial: {text}"
            );
        }
    }

    /// …while the genuine table-wide locks still read, including the "their controllers'"
    /// phrasing that is the only thing making Winter Orb symmetric on its face.
    #[test]
    fn mass_land_denial_still_reads_a_table_wide_lock() {
        for text in [
            "Lands don't untap during their controllers' untap steps.",
            "Nonbasic lands don't untap during their controllers' untap steps.",
            "Each player can't untap more than one land during their untap step.",
            "Players can't play lands.",
        ] {
            assert!(
                is_mass_land_denial(&oracle(text)),
                "a lock on everyone's lands is exactly what this category is: {text}"
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

    /// "If you search your library this way, shuffle" is a back-reference to the search in
    /// the previous sentence, not a search of its own — and it names nothing, so a
    /// descriptor scan finds no land in it and would call every land-fetcher carrying that
    /// boilerplate a tutor.
    #[test]
    fn a_shuffle_back_reference_is_not_a_second_search() {
        assert!(!is_tutor(&oracle(
            "When this creature enters, if an opponent controls more lands than you, search \
             your library for a Plains card and put it onto the battlefield tapped. If you \
             search your library this way, shuffle."
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

//! **Rulebreaker** — the Mystery Booster Commander Edition keyword whose whole text is a
//! change to the *construction* rules its parent module enforces. A deck led by
//! [Whtz, the Bibliophile] has no maximum size; one led by [Seluma, Light of Aysen] may run
//! Angels of any colour identity. Both are answers [`super::evaluate_deck_rules`] would
//! otherwise get wrong, so this module reads the ability and hands back what it permits.
//!
//! It is a **parser, not a list**, for the reason the parent module states: everything is
//! derived from the catalog row, so a Rulebreaker printed after this code was written obeys
//! its own text the day the catalog ingests it. The grammar covers the shapes MBC prints:
//!
//! ```text
//! Rulebreaker — A deck with this commander has no maximum deck size.
//! Rulebreaker — A deck with this commander can have any land cards.
//! Rulebreaker — A deck with this commander can have <types> cards of any color identity
//!               and any basic land cards.
//! Rulebreaker — A deck with this commander can have creature cards with mana value 7 or
//!               greater of any color identity and any basic land cards.
//! Rulebreaker — If <name> is your commander, the color identity of instant and sorcery
//!               cards in your deck can include one color of your choice not in your
//!               commander's color identity, and your deck can have any basic land cards.
//! ```
//!
//! Two deliberate choices:
//!
//! * **Only a `Rulebreaker` ability line is read.** Each phrase the grammar keys on
//!   ("no maximum deck size", "of any color identity", "one color of your choice not in")
//!   appears on Rulebreaker cards and nowhere else in the catalog, so the keyword gate costs
//!   no coverage — and it means no ordinary card can ever have its rules quietly loosened by
//!   a sentence that merely reads like one of these.
//! * **A Rulebreaker we cannot read suppresses the rules it might have lifted** — see
//!   [`Rulebreakers::unreadable`]. The parent module's rule is that a false "in breach" is
//!   worse than a miss, and a commander whose ability we failed to parse is exactly the case
//!   where guessing produces one.

use super::{CardFacts, NUMBER_WORDS, ability_lines, has_word};

/// The keyword itself, as [`ability_lines`] lowercases it.
const KEYWORD: &str = "rulebreaker";

/// The word the grammar's card descriptors end on ("Angel **cards**").
const CARDS: &str = " cards";

/// Which cards a Rulebreaker clause names — "Angel", "artifact creature and Equipment",
/// "creature … with mana value 7 or greater".
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CardMatcher {
    /// Alternatives separated by "and"; a card matches when it satisfies **any** of them,
    /// and satisfies one when **every** word in it is on its front type line. So "artifact
    /// creature and Equipment" is `[["artifact", "creature"], ["equipment"]]`.
    alternatives: Vec<Vec<String>>,
    /// A "with mana value N or greater" qualifier, which applies to the whole descriptor.
    min_mana_value: Option<f64>,
}

impl CardMatcher {
    /// Build from the descriptor's type words, or `None` when they aren't plain type/subtype
    /// words — which is also what stops a descriptor scan that ran past the end of its
    /// sentence from matching anything (the stray words carry the punctuation).
    fn parse(types: &str, min_mana_value: Option<f64>) -> Option<Self> {
        let alternatives: Vec<Vec<String>> = types
            .split(" and ")
            .map(|alternative| {
                alternative
                    .split_whitespace()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|words| !words.is_empty())
            .collect();
        if alternatives.is_empty() {
            return None;
        }
        let plain = |word: &String| {
            word.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '\'')
        };
        if !alternatives.iter().flatten().all(plain) {
            return None;
        }
        Some(Self {
            alternatives,
            min_mana_value,
        })
    }

    /// Whether this card is one the clause names. A card whose mana value the catalog
    /// doesn't carry fails a mana-value qualifier rather than passing it — the qualifier
    /// exists to *narrow* the descriptor, so an unknown value can't satisfy it.
    pub(super) fn matches(&self, card: &CardFacts) -> bool {
        if let Some(min) = self.min_mana_value
            && !card.cmc.is_some_and(|value| value >= min)
        {
            return false;
        }
        self.alternatives.iter().any(|words| {
            words
                .iter()
                .all(|word| has_word(&card.front_type_line, word))
        })
    }
}

/// What one Rulebreaker clause permits.
#[derive(Clone, Debug, PartialEq)]
enum RulebreakerEffect {
    /// "…has no maximum deck size." The *floor* still applies — a Commander deck is still
    /// short of legal at 99 cards, it simply may keep going past 100.
    NoMaximumDeckSize,
    /// "…can have `<these>` cards of any color identity", and its "can have any `<these>`
    /// cards" spelling: the cards named are outside the colour-identity rule entirely.
    AnyColourIdentity(CardMatcher),
    /// "…the color identity of `<these>` cards in your deck can include `<n>` color(s) of
    /// your choice not in your commander's color identity": one choice, shared by every
    /// card the clause names.
    ExtraColours { cards: CardMatcher, count: usize },
}

/// Every Rulebreaker effect a deck's command zone grants.
#[derive(Clone, Debug, Default)]
pub(super) struct Rulebreakers {
    effects: Vec<RulebreakerEffect>,
    /// A Rulebreaker ability was there and the grammar above didn't recognise it. Since
    /// every one of them *widens* a construction rule, reporting the deck against the
    /// unwidened rules would be a false "in breach" — so an unreadable Rulebreaker lifts
    /// the maximum deck size and exempts every card from colour identity, exactly as if it
    /// had said so. The deck's own floor, copy limit and command zone are untouched: no
    /// printed Rulebreaker speaks to them.
    unreadable: bool,
}

impl Rulebreakers {
    /// Read the Rulebreaker abilities off a deck's command zone. Callers pass the command
    /// zone's cards only — every clause is worded "a deck with this commander", so the same
    /// card in the 99 grants nothing.
    ///
    /// Effects are **deduplicated**, and callers pass one entry per *distinct* card rather
    /// than one per deck row. Both matter: every effect held here is later tested against
    /// every card name in the deck, and a deck's command-zone row count is caller-controlled
    /// (nothing stops one deck filing the same commander in two hundred sections), so
    /// keeping a duplicate per row would make the colour-identity check quadratic in data an
    /// attacker chooses. Held this way, the set is bounded by the printed Rulebreakers.
    pub(super) fn collect<'a>(cards: impl IntoIterator<Item = &'a CardFacts>) -> Self {
        let mut found = Self::default();
        for card in cards {
            for line in ability_lines(card) {
                let Some(clause) = rulebreaker_clause(&line) else {
                    continue;
                };
                match parse_clause(clause) {
                    Some(effects) if !effects.is_empty() => {
                        for effect in effects {
                            if !found.effects.contains(&effect) {
                                found.effects.push(effect);
                            }
                        }
                    }
                    // Either the grammar failed outright, or it read the line and found it
                    // granted nothing — a Rulebreaker that grants nothing is one we have
                    // not understood.
                    _ => found.unreadable = true,
                }
            }
        }
        found
    }

    /// Whether the format's exact deck size becomes a floor.
    pub(super) fn lifts_maximum_deck_size(&self) -> bool {
        self.unreadable || self.effects.contains(&RulebreakerEffect::NoMaximumDeckSize)
    }

    /// Whether this card is outside the colour-identity rule altogether.
    pub(super) fn exempts_from_colour_identity(&self, card: &CardFacts) -> bool {
        self.unreadable
            || self.effects.iter().any(|effect| match effect {
                RulebreakerEffect::AnyColourIdentity(cards) => cards.matches(card),
                _ => false,
            })
    }

    /// The "N colours of your choice" clauses, each with the cards it names.
    pub(super) fn extra_colour_clauses(&self) -> Vec<(&CardMatcher, usize)> {
        self.effects
            .iter()
            .filter_map(|effect| match effect {
                RulebreakerEffect::ExtraColours { cards, count } => Some((cards, *count)),
                _ => None,
            })
            .collect()
    }
}

/// The text of a `Rulebreaker` ability line, with the keyword and its em dash removed, or
/// `None` for any other line. A bare "Rulebreaker" with no clause after it grants nothing.
fn rulebreaker_clause(line: &str) -> Option<&str> {
    let rest = line.strip_prefix(KEYWORD)?;
    // The same separators [`super::has_ability`] accepts after a keyword, so "Rulebreaker —"
    // and "Rulebreaker-" read alike and "Rulebreakers of the Guild" doesn't read at all.
    if !rest.starts_with([' ', '\u{2014}', '-']) {
        return None;
    }
    let text = rest.trim_start_matches([' ', '\u{2014}', '-']).trim_end();
    (!text.is_empty()).then_some(text)
}

/// Every effect one Rulebreaker clause grants, or `None` when the grammar met something it
/// couldn't read — which the caller turns into [`Rulebreakers::unreadable`]. The three
/// shapes are independent scans rather than one sentence parse: Tolabow's clause carries
/// both a colour choice and a "can have" exemption, and reading each on its own marker keeps
/// them from interfering. A failure in *any* of them fails the whole clause, so a card whose
/// second half we misread can't be honoured on its first half alone.
fn parse_clause(text: &str) -> Option<Vec<RulebreakerEffect>> {
    let mut effects = Vec::new();
    if text.contains("no maximum deck size") {
        effects.push(RulebreakerEffect::NoMaximumDeckSize);
    }
    if let Some(effect) = parse_extra_colours(text) {
        effects.push(effect);
    }
    let mut rest = text;
    while let Some(index) = rest.find("can have ") {
        rest = &rest[index + "can have ".len()..];
        effects.extend(
            parse_descriptors(rest)?
                .into_iter()
                .map(RulebreakerEffect::AnyColourIdentity),
        );
    }
    Some(effects)
}

/// The card descriptors following a "can have": one per `[any ]<types> cards[ with mana
/// value N or greater][ of any color identity]`, joined by "and". `None` means the text
/// after the "can have" is not a descriptor list this grammar knows.
///
/// The list must be accounted for **whole**: every descriptor ends either on an "and" that
/// introduces another or at the end of its sentence, and anything else — a qualifier in a
/// word order we don't know, a clause never printed before — fails the parse rather than
/// being quietly dropped. Silently stopping mid-list is the dangerous failure: it keeps the
/// descriptors already read (too generous) *and* discards the ones still to come (too
/// strict, and a false "in breach" is the thing this module must never produce).
///
/// Terminates because every pass consumes at least the ` cards` it found.
fn parse_descriptors(text: &str) -> Option<Vec<CardMatcher>> {
    let mut matchers = Vec::new();
    let mut rest = text;
    let mut first = true;
    loop {
        let mut head = rest.trim_start();
        head = head.strip_prefix("and ").unwrap_or(head).trim_start();
        head = head.strip_prefix("any ").unwrap_or(head).trim_start();
        let Some(end) = head.find(CARDS) else {
            // On the first pass this simply wasn't a "can have <some> cards" clause, and the
            // clause may still grant something through another marker. After an "and" it is
            // a continuation we promised to read and couldn't.
            return first.then_some(matchers);
        };
        let types = &head[..end];
        let (min_mana_value, tail) = strip_mana_value(&head[end + CARDS.len()..])?;
        rest = tail.strip_prefix(" of any color identity").unwrap_or(tail);
        matchers.push(CardMatcher::parse(types, min_mana_value)?);
        first = false;
        // A further descriptor only ever follows on "and"; otherwise the sentence must end
        // here, or we have not understood it.
        match rest.strip_prefix(" and ") {
            Some(next) => rest = next,
            None if rest.is_empty() || rest.starts_with(['.', ',', ';']) => return Some(matchers),
            None => return None,
        }
    }
}

/// Split a leading " with mana value N or greater" qualifier off `rest`, returning N and
/// what follows it. Text with no qualifier at all yields `(None, rest)`; a *qualifier we
/// can't read* — "6 or less", "3 or 4" — yields `None`, because dropping it would silently
/// widen the descriptor it was there to narrow.
#[allow(clippy::type_complexity)]
fn strip_mana_value(rest: &str) -> Option<(Option<f64>, &str)> {
    const PREFIX: &str = " with mana value ";
    const SUFFIX: &str = " or greater";
    let Some(after) = rest.strip_prefix(PREFIX) else {
        return Some((None, rest));
    };
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    let value: f64 = digits.parse().ok()?;
    let tail = after[digits.len()..].strip_prefix(SUFFIX)?;
    Some((Some(value), tail))
}

/// "the color identity of `<types>` cards in your deck can include `<n>` color(s) of your
/// choice not in your commander's color identity".
fn parse_extra_colours(text: &str) -> Option<RulebreakerEffect> {
    const SUBJECT: &str = "the color identity of ";
    const CHOICE: &str = "can include ";
    let index = text.find(SUBJECT)?;
    let after = &text[index + SUBJECT.len()..];
    let end = after.find(CARDS)?;
    let rest = &after[end + CARDS.len()..];
    let choice = rest.find(CHOICE)?;
    let tail = &rest[choice + CHOICE.len()..];
    // "…of your choice" is what makes this a permission rather than a statement about the
    // deck, and it has to be this clause's, not a later sentence's.
    if !tail.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let word: String = tail.chars().take_while(char::is_ascii_alphabetic).collect();
    let count = NUMBER_WORDS
        .iter()
        .find(|(spelling, _)| *spelling == word)
        .map(|(_, count)| *count)?;
    if !tail[word.len()..].starts_with(" color of your choice")
        && !tail[word.len()..].starts_with(" colors of your choice")
    {
        return None;
    }
    let cards = CardMatcher::parse(&after[..end], None)?;
    Some(RulebreakerEffect::ExtraColours {
        cards,
        count: usize::try_from(count).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::decks::analysis::test_fixtures::card;

    /// Every Rulebreaker printed in Mystery Booster Commander Edition, oracle text verbatim.
    /// The parser is a grammar rather than a list precisely so a ninth needs no code — these
    /// are here to prove the grammar covers the eight that exist.
    fn whtz() -> CardFacts {
        card("whtz", "Whtz, the Bibliophile")
            .type_line("Legendary Creature — Homunculus")
            .colors("W,U")
            .oracle(
                "Rulebreaker — A deck with this commander has no maximum deck size.\n\
                 {3}, {T}: You draw a card and gain 1 life. This ability costs {3} less to \
                 activate if you had 200 or more cards in your starting deck.",
            )
    }

    fn grizzlegom() -> CardFacts {
        card("grizzlegom", "Grizzlegom, Hurloon Hero")
            .type_line("Legendary Creature — Minotaur Warrior")
            .colors("R,G")
            .oracle("Rulebreaker — A deck with this commander can have any land cards.")
    }

    fn maular() -> CardFacts {
        card("maular", "Maular, the Next Evolution")
            .type_line("Legendary Creature — Dinosaur Mutant")
            .colors("G")
            .oracle(
                "Rulebreaker — A deck with this commander can have creature cards with mana \
                 value 7 or greater of any color identity and any basic land cards.",
            )
    }

    fn seluma() -> CardFacts {
        card("seluma", "Seluma, Light of Aysen")
            .type_line("Legendary Creature — Angel Warrior")
            .colors("W")
            .oracle(
                "Rulebreaker — A deck with this commander can have Angel cards of any color \
                 identity and any basic land cards.\nFlying",
            )
    }

    fn everforger() -> CardFacts {
        card("everforger", "The Everforger")
            .type_line("Legendary Artifact Creature — Construct")
            .oracle(
                "Rulebreaker — A deck with this commander can have artifact creature and \
                 Equipment cards of any color identity and any basic land cards.",
            )
    }

    fn unluckiest() -> CardFacts {
        card("unluckiest", "The Unluckiest Planeswalker")
            .type_line("Legendary Planeswalker")
            .colors("R")
            .oracle(
                "Rulebreaker — A deck with this commander can have Aura cards of any color \
                 identity and any basic land cards.\n\
                 The Unluckiest Planeswalker can be your commander.",
            )
    }

    fn tolabow() -> CardFacts {
        card("tolabow", "Tolabow, Loch Rascal")
            .type_line("Legendary Creature — Otter")
            .colors("U")
            .oracle(
                "Rulebreaker — If Tolabow, Loch Rascal is your commander, the color identity \
                 of instant and sorcery cards in your deck can include one color of your \
                 choice not in your commander's color identity, and your deck can have any \
                 basic land cards.",
            )
    }

    fn valko() -> CardFacts {
        card("valko", "Valko Indorian")
            .type_line("Legendary Creature — Human Wizard")
            .colors("B")
            .oracle(
                "Rulebreaker — A deck with this commander can have Phyrexian cards of any \
                 color identity and any basic land cards.",
            )
    }

    /// Nothing in the set defeats the grammar — every one of the eight yields at least one
    /// effect, so none of them trips the unreadable fallback.
    #[test]
    fn every_printed_rulebreaker_parses() {
        for commander in [
            whtz(),
            grizzlegom(),
            maular(),
            seluma(),
            everforger(),
            unluckiest(),
            tolabow(),
            valko(),
        ] {
            let read = Rulebreakers::collect([&commander]);
            assert!(
                !read.unreadable && !read.effects.is_empty(),
                "{} parsed as {read:?}",
                commander.name
            );
        }
    }

    #[test]
    fn only_whtz_lifts_the_maximum_deck_size() {
        assert!(Rulebreakers::collect([&whtz()]).lifts_maximum_deck_size());
        assert!(!Rulebreakers::collect([&seluma()]).lifts_maximum_deck_size());
        let no_commander: [&CardFacts; 0] = [];
        assert!(!Rulebreakers::collect(no_commander).lifts_maximum_deck_size());
    }

    /// The exemption each card grants, and one near miss per card that it must *not* cover.
    #[test]
    fn each_exemption_names_its_own_cards() {
        let angel = card("a", "Lyra").type_line("Legendary Creature — Angel");
        let bear = card("b", "Bear").type_line("Creature — Bear");
        let forest = card("f", "Forest").type_line("Basic Land — Forest");
        let dual = card("d", "Tundra").type_line("Land — Plains Island");
        let equipment = card("e", "Skullclamp").type_line("Artifact — Equipment");
        let robot = card("r", "Myr").type_line("Artifact Creature — Myr");
        let aura = card("u", "Rancor").type_line("Enchantment — Aura");
        let phyrexian = card("p", "Sheoldred").type_line("Legendary Creature — Phyrexian Praetor");
        let titan = card("t", "Titan").type_line("Creature — Giant").cmc(7.0);
        let small = card("s", "Ogre").type_line("Creature — Ogre").cmc(6.0);

        let seluma = Rulebreakers::collect([&seluma()]);
        assert!(seluma.exempts_from_colour_identity(&angel));
        assert!(seluma.exempts_from_colour_identity(&forest), "and basics");
        assert!(!seluma.exempts_from_colour_identity(&bear));
        assert!(!seluma.exempts_from_colour_identity(&dual), "*basic* land");

        let grizzlegom = Rulebreakers::collect([&grizzlegom()]);
        assert!(grizzlegom.exempts_from_colour_identity(&dual), "any land");
        assert!(grizzlegom.exempts_from_colour_identity(&forest));
        assert!(!grizzlegom.exempts_from_colour_identity(&bear));

        let everforger = Rulebreakers::collect([&everforger()]);
        assert!(everforger.exempts_from_colour_identity(&equipment));
        assert!(everforger.exempts_from_colour_identity(&robot));
        assert!(
            !everforger.exempts_from_colour_identity(&bear),
            "not artifact"
        );

        let unluckiest = Rulebreakers::collect([&unluckiest()]);
        assert!(unluckiest.exempts_from_colour_identity(&aura));
        assert!(!unluckiest.exempts_from_colour_identity(&bear));

        let valko = Rulebreakers::collect([&valko()]);
        assert!(valko.exempts_from_colour_identity(&phyrexian));
        assert!(!valko.exempts_from_colour_identity(&bear));

        let maular = Rulebreakers::collect([&maular()]);
        assert!(maular.exempts_from_colour_identity(&titan));
        assert!(
            !maular.exempts_from_colour_identity(&small),
            "mana value 7+"
        );
        assert!(
            !maular.exempts_from_colour_identity(&card("x", "?").type_line("Creature — Bear")),
            "an unknown mana value can't satisfy a mana-value qualifier"
        );
        assert!(maular.exempts_from_colour_identity(&forest));
    }

    #[test]
    fn tolabow_grants_one_colour_to_instants_and_sorceries() {
        let read = Rulebreakers::collect([&tolabow()]);
        let clauses = read.extra_colour_clauses();
        assert_eq!(clauses.len(), 1);
        let (cards, count) = clauses[0];
        assert_eq!(count, 1);
        assert!(cards.matches(&card("i", "Bolt").type_line("Instant")));
        assert!(cards.matches(&card("s", "Wrath").type_line("Sorcery")));
        assert!(!cards.matches(&card("c", "Bear").type_line("Creature — Bear")));
        // The second half of the same sentence is still an ordinary exemption.
        assert!(
            read.exempts_from_colour_identity(
                &card("f", "Forest").type_line("Basic Land — Forest")
            )
        );
        assert!(!read.exempts_from_colour_identity(&card("i", "Bolt").type_line("Instant")));
    }

    /// The keyword gate: an ordinary card is never read for these phrases, however its own
    /// text reads.
    #[test]
    fn only_a_rulebreaker_line_is_read() {
        let impostor = card("i", "Impostor").oracle(
            "A deck with this commander has no maximum deck size.\n\
             Rulebreakers of the Guild get +1/+1.",
        );
        let read = Rulebreakers::collect([&impostor]);
        assert!(!read.lifts_maximum_deck_size());
        assert!(!read.unreadable, "neither line is a Rulebreaker ability");
    }

    /// Reminder text never grants anything — [`ability_lines`] strips it before we look.
    #[test]
    fn reminder_text_grants_nothing() {
        let explained = card("r", "Explained")
            .oracle("Flying\n(Rulebreaker — A deck with this commander can have any land cards.)");
        let read = Rulebreakers::collect([&explained]);
        assert!(!read.unreadable);
        assert!(read.effects.is_empty());
    }

    /// A Rulebreaker the grammar can't read widens everything it might have widened, rather
    /// than reporting a legal deck as illegal.
    #[test]
    fn an_unreadable_rulebreaker_suppresses_what_it_might_have_lifted() {
        let future = card("f", "Future Legend")
            .type_line("Legendary Creature — Wizard")
            .oracle("Rulebreaker — A deck with this commander plays by tomorrow's rules.");
        let read = Rulebreakers::collect([&future]);
        assert!(read.unreadable);
        assert!(read.lifts_maximum_deck_size());
        assert!(read.exempts_from_colour_identity(&card("b", "Bear").type_line("Creature — Bear")));

        // A bare keyword with no clause is not an ability we failed to read.
        let bare = card("b", "Bare").oracle("Rulebreaker");
        assert!(!Rulebreakers::collect([&bare]).unreadable);
    }

    /// A descriptor scan must stop at its sentence: the words after a full stop aren't type
    /// words, so they can never form an exemption.
    #[test]
    fn a_descriptor_never_runs_past_its_sentence() {
        let chatty = card("c", "Chatty")
            .oracle("Rulebreaker — A deck with this commander can have any land. Draw two cards.");
        let read = Rulebreakers::collect([&chatty]);
        assert!(read.unreadable, "nothing readable, so nothing is claimed");
        let mixed = card("m", "Mixed").oracle(
            "Rulebreaker — A deck with this commander can have any land cards. Draw two cards.",
        );
        let read = Rulebreakers::collect([&mixed]);
        assert!(!read.unreadable);
        assert!(
            read.exempts_from_colour_identity(
                &card("i", "Island").type_line("Basic Land — Island")
            )
        );
        assert!(
            !read.exempts_from_colour_identity(&card("b", "Bear").type_line("Creature — Bear"))
        );
    }

    /// A descriptor list must be read **whole**. Stopping mid-list would keep the
    /// descriptors already read and silently drop the rest — too generous about one half of
    /// the sentence and too strict about the other, which is how a legal deck gets reported
    /// as illegal. Each of these is a shape the grammar doesn't know, and each must fail the
    /// clause rather than half-succeed.
    #[test]
    fn half_a_descriptor_list_is_no_descriptor_list() {
        // A qualifier that isn't "or greater": dropping it would widen the descriptor it was
        // printed to narrow, *and* sever the "and any basic land cards" that follows it.
        let inverted = card("i", "Inverted").oracle(
            "Rulebreaker — A deck with this commander can have creature cards with mana \
             value 6 or less of any color identity and any basic land cards.",
        );
        let read = Rulebreakers::collect([&inverted]);
        assert!(read.unreadable, "got {read:?}");
        assert!(read.effects.is_empty());

        // The same qualifier in a word order the grammar doesn't know.
        let flipped = card("f", "Flipped").oracle(
            "Rulebreaker — A deck with this commander can have creature cards of any color \
             identity with mana value 7 or greater and any basic land cards.",
        );
        assert!(Rulebreakers::collect([&flipped]).unreadable);

        // A continuation we promised to read ("and …") and couldn't.
        let trailing = card("t", "Trailing").oracle(
            "Rulebreaker — A deck with this commander can have Angel cards of any color \
             identity and whatever else it fancies.",
        );
        assert!(Rulebreakers::collect([&trailing]).unreadable);

        // And the whole clause fails together: Tolabow's shape, with its second half
        // unreadable, must not be honoured on its first half alone.
        let partial = card("p", "Partial").oracle(
            "Rulebreaker — If Partial is your commander, the color identity of instant and \
             sorcery cards in your deck can include one color of your choice not in your \
             commander's color identity, and your deck can have any basic land cards of the \
             third kind.",
        );
        let read = Rulebreakers::collect([&partial]);
        assert!(read.unreadable, "got {read:?}");
        assert!(read.extra_colour_clauses().is_empty());
    }

    /// Effects are held once however many command-zone rows carry them. Every effect here is
    /// tested against every card name in the deck, and the row count is caller-controlled,
    /// so a duplicate per row would make the colour-identity check quadratic in chosen data.
    #[test]
    fn effects_are_held_once_however_many_rows_grant_them() {
        let one = Rulebreakers::collect([&seluma()]);
        let many = Rulebreakers::collect([&seluma(), &seluma(), &seluma(), &seluma()]);
        assert_eq!(many.effects.len(), one.effects.len());
        assert_eq!(many.effects, one.effects);
        // Distinct commanders still contribute their own.
        let pair = Rulebreakers::collect([&seluma(), &valko()]);
        assert!(pair.effects.len() > one.effects.len());
    }

    /// The grammar reads its numbers and mana values off the card, so a Rulebreaker printed
    /// with different ones needs no code change.
    #[test]
    fn numbers_come_off_the_card() {
        let generous = card("g", "Generous").oracle(
            "Rulebreaker — If Generous is your commander, the color identity of \
                 enchantment cards in your deck can include two colors of your choice not in \
                 your commander's color identity.",
        );
        let read = Rulebreakers::collect([&generous]);
        let clauses = read.extra_colour_clauses();
        assert_eq!(clauses.len(), 1);
        assert_eq!(clauses[0].1, 2);

        let heavy = card("h", "Heavy").oracle(
            "Rulebreaker — A deck with this commander can have artifact cards with mana \
             value 4 or greater of any color identity.",
        );
        let read = Rulebreakers::collect([&heavy]);
        let big = card("a", "Colossus")
            .type_line("Artifact — Construct")
            .cmc(4.0);
        let small = card("b", "Bauble").type_line("Artifact").cmc(3.0);
        assert!(read.exempts_from_colour_identity(&big));
        assert!(!read.exempts_from_colour_identity(&small));
    }
}

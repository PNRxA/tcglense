//! The deckbuilding **roles** a card can fill — ramp, card draw, targeted removal, board
//! wipes, counterspells, tutors, recursion, protection — read off its own rules text
//! (issue #671).
//!
//! "Ten ramp, ten draw, eight removal, three wipes" is how a Commander list is counted, and
//! the type presets the importer files cards into (`deck_import::categorize`) can't answer
//! it: a Cultivate is a sorcery, and a builder wants to know it is *ramp*. Each predicate
//! below is one grammar over the card's oracle text, built on the clause grammar in
//! [`super`] (the same one the bracket's signals read, so "does this clause say X" means one
//! thing) and on [`super::super::rules`]'s `has_word` / `own_names` rather than a second
//! copy of either.
//!
//! The stance is the bracket's, kept as a habit even though a false positive here costs a
//! builder one wrong bar rather than a wrong bracket: **every predicate declines when it
//! isn't sure**, and the counted cards ride the response so a number can be audited. The
//! near-misses each predicate is written around are named in its tests — the search for a
//! land that goes to *hand* is not ramp, the "Whenever you draw a card" trigger is not draw,
//! the keyword line `Hexproof` protects nothing but the card it is printed on. A card may
//! fill more than one role (a cantripping removal spell is both), and no predicate here is
//! an "else": a card matching none of them is simply unclassified.
//!
//! The one predicate not written here is the tutor, which is [`super::bracket::is_tutor`]
//! verbatim — the bracket already had the right answer, and two "is this a tutor" readers
//! would drift.

use super::super::CardFacts;
use super::super::rules::{has_word, own_names};
use super::{LAND_WORDS, is_self_scoped, quantified_noun, search_descriptor, sentences};

/// The five basic land *types* — what makes a land search "for a **Forest** card" typed.
/// [`LAND_WORDS`] minus the bare type itself.
const LAND_TYPE_WORDS: &[&str] = &[
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

/// Nouns a removal effect or a wipe reaches: things that sit on the battlefield.
const PERMANENT_NOUNS: &[&str] = &[
    "creature",
    "creatures",
    "artifact",
    "artifacts",
    "enchantment",
    "enchantments",
    "planeswalker",
    "planeswalkers",
    "permanent",
    "permanents",
    "land",
    "lands",
    "battle",
    "battles",
    "aura",
    "auras",
    "equipment",
    "token",
    "tokens",
];

/// Words that may sit between a wipe's quantifier and the noun it reaches — the type-list
/// words the land scan takes, **plus** the restrictions a wrath spells out ("all nonland
/// permanents", "each attacking creature", "all nonblack creatures"). "Nonland" is here and
/// deliberately *not* in the land scan's list: there it would let "destroy all nonland
/// permanents" reach a land, here it is how that very card reaches its permanents.
const WIPE_LIST_WORDS: &[&str] = &[
    "and",
    "or",
    "other",
    "the",
    "basic",
    "nonbasic",
    "non-basic",
    "snow",
    "legendary",
    "nonlegendary",
    "tapped",
    "untapped",
    "attacking",
    "blocking",
    "nontoken",
    "nonland",
    "non-land",
    "nonartifact",
    "noncreature",
    "nonenchantment",
    "white",
    "blue",
    "black",
    "red",
    "green",
    "nonwhite",
    "nonblue",
    "nonblack",
    "nonred",
    "nongreen",
    "colorless",
    "multicolored",
    "monocolored",
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

/// Words that end a "target …" scan without finding a permanent: the target is a player, a
/// spell, a card in a zone, or an ability — none of which removal takes off the battlefield.
const TARGET_STOP_WORDS: &[&str] = &[
    "player",
    "players",
    "opponent",
    "opponents",
    "spell",
    "spells",
    "card",
    "cards",
    "ability",
    "abilities",
    "activated",
    "triggered",
    "you",
    "each",
];

/// How far past "target" the scan reads for the noun it governs — "target nonblack
/// nonartifact creature" is three words of restriction before the noun.
const TARGET_SCAN_WORDS: usize = 5;

/// The ways a card refers to itself in its own text — modern oracle wording, which the
/// catalog stores. A card's *name* is the other way (older text, and every legendary), read
/// through [`own_names`] so a reversible printing's `Name // Name` still answers.
const SELF_REFERENCES: &[&str] = &[
    "this card",
    "this creature",
    "this permanent",
    "this artifact",
    "this enchantment",
    "this planeswalker",
    "this land",
    "this spell",
    "this aura",
    "this equipment",
    "this vehicle",
    "this token",
];

/// Keyword abilities a protection effect grants.
const PROTECTION_WORDS: &[&str] = &["hexproof", "indestructible", "shroud", "protection from"];

// ---------- Shared readers ----------

fn has_any_word(sentence: &str, words: &[&str]) -> bool {
    words.iter().any(|word| has_word(sentence, word))
}

/// Whether **any face** of the card is a land. A land taps for mana because it is a land, and
/// a fetch land finds a land because that is what fetch lands do — neither is *ramp*. The
/// raw type line is read, not just the front face: a modal double-faced spell's oracle text
/// carries its land back face's `{T}: Add {U}.` too, and Jwari Disruption is a counterspell
/// with a land on the back, not a mana rock.
fn is_land(card: &CardFacts) -> bool {
    has_word(&card.front_type_line, "land")
        || card
            .type_line
            .as_deref()
            .is_some_and(|line| has_word(&line.to_lowercase(), "land"))
}

/// Whether `text` ends with a reference to the card itself — "return **this card** from your
/// graveyard", "regenerate **this creature**", "**Squee, Goblin Nabob** from your graveyard".
fn ends_with_self_reference(card: &CardFacts, text: &str) -> bool {
    let text = text.trim_end();
    SELF_REFERENCES.iter().any(|phrase| text.ends_with(phrase))
        || own_names(card)
            .iter()
            .any(|name| text.ends_with(name.as_str()))
}

/// Whether `text` starts with a reference to the card itself — "**this creature** gains
/// indestructible", "**this creature** deals 1 damage to any target".
fn starts_with_self_reference(card: &CardFacts, text: &str) -> bool {
    let text = text.trim_start();
    SELF_REFERENCES
        .iter()
        .any(|phrase| text.starts_with(phrase))
        || own_names(card)
            .iter()
            .any(|name| text.starts_with(name.as_str()))
}

/// Whether the sentence is a **prevention** effect. "Prevent all damage that would be dealt
/// to target creature" names a target, a permanent and damage, and is the opposite of
/// removal — so both the removal and wipe readers stand down on it, and the protection
/// reader is the one that counts it.
fn prevents(sentence: &str) -> bool {
    has_word(sentence, "prevent")
}

/// Whether the sentence shrinks something — `gets -N/-N` (or `-X/-X`): the second sign is
/// what separates Grasp of Darkness from a `-2/-0` that only blunts an attacker.
fn shrinks(sentence: &str) -> bool {
    for verb in ["gets -", "get -"] {
        let mut from = 0usize;
        while let Some(offset) = sentence[from..].find(verb) {
            let start = from + offset + verb.len();
            let rest = &sentence[start..];
            let digits = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .count();
            if digits > 0
                && let Some(toughness) = rest[digits..].strip_prefix("/-")
            {
                let drop: String = toughness
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                // "-2/-0" blunts an attacker; only a real toughness drop shrinks.
                if !drop.is_empty() && drop != "0" {
                    return true;
                }
            }
            from = start;
        }
    }
    false
}

/// One "target …" phrase in a clause, resolved to what it points at.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    /// A permanent on the battlefield that isn't the caster's own.
    OthersPermanent,
    /// A permanent the caster controls ("target creature you control").
    OwnPermanent,
    /// A player, a spell, a card in a zone, an ability — or nothing the scan could name.
    Other,
}

/// Every target the clause names, in order. "target creature you control fights target
/// creature you don't control" is two targets, one own and one not — which is what lets a
/// fight spell count as removal while a flicker of your own creature doesn't.
fn targets(sentence: &str) -> Vec<Target> {
    let mut found = Vec::new();
    let mut from = 0usize;
    while let Some(offset) = sentence[from..].find("target") {
        let start = from + offset;
        let end = start + "target".len();
        let bytes = sentence.as_bytes();
        let starts_a_word = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let ends_a_word = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        from = start + 1;
        if !starts_a_word {
            continue;
        }
        if !ends_a_word {
            // The plural noun — "damage divided evenly among any number of **targets**"
            // (Fireball, Arc Lightning, Comet Storm) — is removal-shaped exactly when the
            // damage is divided; any other suffix ("targeted", "targeting", "untargetable")
            // is not the noun at all.
            let plural = bytes[end] == b's'
                && (end + 1 == bytes.len() || !bytes[end + 1].is_ascii_alphanumeric());
            if plural && has_word(sentence, "divided") {
                found.push(Target::OthersPermanent);
            }
            continue;
        }
        // "any target" is a creature, a player, or a planeswalker — removal-shaped.
        if sentence[..start].trim_end().ends_with("any") {
            found.push(Target::OthersPermanent);
            continue;
        }
        let after = &sentence[end..];
        let mut resolved = Target::Other;
        // Walked by byte position so the text *after* the noun can be read — a `find` of
        // the noun could land on an earlier word that happens to contain it.
        let mut position = 0usize;
        for (index, raw) in after
            .split_whitespace()
            .take(TARGET_SCAN_WORDS + 1)
            .enumerate()
        {
            let at = after[position..]
                .find(raw)
                .map_or(position, |offset| position + offset);
            position = at + raw.len();
            let word = super::bare_word(raw);
            if index == TARGET_SCAN_WORDS || TARGET_STOP_WORDS.contains(&word) {
                break;
            }
            if PERMANENT_NOUNS.contains(&word) {
                let following = after[position..].trim_start();
                let next = following.split_whitespace().next().map(super::bare_word);
                resolved = match next {
                    // "target creature card" is a card in a zone; "target creature spell"
                    // is on the stack. Neither is on the battlefield.
                    Some("card" | "cards" | "spell" | "spells") => Target::Other,
                    _ if following.starts_with("you control")
                        || following.starts_with("you own") =>
                    {
                        Target::OwnPermanent
                    }
                    _ => Target::OthersPermanent,
                };
                break;
            }
        }
        found.push(resolved);
    }
    found
}

// ---------- Ramp ----------

/// Whether a search descriptor names a **typed** land — "a basic land card", "a Forest
/// card", "a Plains, Island, Swamp, or Mountain card". A bare "a land card" is left alone:
/// Crop Rotation and Knight of the Reliquary search for one, and neither adds a land.
fn typed_land_descriptor(descriptor: &str) -> bool {
    has_word(descriptor, "basic") || has_any_word(descriptor, LAND_TYPE_WORDS)
}

/// Whether the ability line holds a **tap-for-mana** ability — `{T}: Add …`, whether the
/// card's own or one it grants ("Enchanted land has '{T}: Add {G}.'"). The `{t}` is what
/// keeps a ritual out: Dark Ritual adds mana too, once.
fn taps_for_mana(line: &str) -> bool {
    line.contains("{t}") && line.contains(": add ")
}

/// Whether the card is **ramp** — it makes more mana than a land drop does, or puts lands
/// onto the battlefield past the one a turn allows.
///
/// Three shapes. A nonland permanent that taps for mana (a rock, a dork, an aura that makes
/// a land add more). A land search that puts a **typed** land onto the battlefield
/// (Cultivate, Rampant Growth, Wood Elves) — to the battlefield, because Land Tax's search
/// to hand is card advantage, and typed, because Crop Rotation's "a land card" swaps a land
/// for a land. And an extra land drop (Exploration, Azusa, Burgeoning's land from hand).
/// A land is never ramp, however it fetches or taps: that is what a land is.
pub(crate) fn is_ramp(card: &CardFacts) -> bool {
    if is_land(card) {
        return false;
    }
    let mana_ability = super::super::rules::ability_lines(card)
        .iter()
        .any(|line| taps_for_mana(line));
    if mana_ability {
        return true;
    }
    sentences(card).iter().any(|sentence| {
        if sentence.contains("graveyard") {
            return false;
        }
        // "Whenever enchanted land is tapped for mana, its controller adds an additional {G}."
        if sentence.contains("adds an additional") || sentence.contains("add an additional") {
            return true;
        }
        if sentence.contains("additional land") && has_word(sentence, "play") {
            return true;
        }
        if !sentence.contains("onto the battlefield") {
            return false;
        }
        if let Some(descriptor) = search_descriptor(sentence) {
            return typed_land_descriptor(&descriptor);
        }
        sentence.contains("land card from your hand")
            || sentence.contains("land cards from your hand")
    })
}

// ---------- Card draw ----------

/// Whether the card **draws you cards**.
///
/// The verb has to be yours — "draw a card", "you draw two cards", "each player draws",
/// "target player draws" (Blue Sun's Zenith is pointed at yourself) — so an opponent's
/// draw, a drawback, or a card handed to "that player" at their draw step is left alone.
/// And it has to be an *effect*, not a trigger on drawing or a replacement of a draw:
/// "Whenever you draw a card" is Niv-Mizzet, "If you would draw a card … instead" is a
/// doubler, and neither puts a card in your hand by itself.
pub(crate) fn is_card_draw(card: &CardFacts) -> bool {
    sentences(card).iter().any(|sentence| {
        let draws = has_word(sentence, "draw")
            || sentence.contains("each player draws")
            || sentence.contains("each player may draw")
            || sentence.contains("target player draws");
        if !draws || !(has_word(sentence, "card") || has_word(sentence, "cards")) {
            return false;
        }
        for shape in [
            "whenever you draw",
            "whenever a player draws",
            "would draw",
            "can't draw",
            "don't draw",
            "draw step",
            "instead of drawing",
            "the first time you draw",
            "the second time you draw",
        ] {
            if sentence.contains(shape) {
                return false;
            }
        }
        true
    })
}

// ---------- Targeted removal ----------

/// Whether the clause takes a targeted permanent off the battlefield, or as good as:
/// destroys, exiles, bounces, buries, damages, shrinks, or fights it.
fn removes_a_target(sentence: &str) -> bool {
    if prevents(sentence) || sentence.contains("graveyard") {
        return false;
    }
    let hits_another = targets(sentence).contains(&Target::OthersPermanent);
    if !hits_another {
        return false;
    }
    has_any_word(
        sentence,
        &["destroy", "destroys", "exile", "exiles", "fight", "fights"],
    ) || (has_word(sentence, "damage") && !sentence.contains("prevent"))
        || shrinks(sentence)
        || sentence.contains("owner's hand")
        || sentence.contains("owners' hands")
        || sentence.contains("owner's library")
        || sentence.contains("into their library")
}

/// Whether the clause is an **edict** — it makes a player sacrifice something, without
/// targeting the thing itself. A land isn't "something" here: "each player sacrifices a
/// land" is Smallpox's tax, and the bracket's land-denial category is where the mass
/// version of it is counted.
fn edicts(sentence: &str) -> bool {
    [
        "target player sacrifices",
        "target opponent sacrifices",
        "each opponent sacrifices",
        "each player sacrifices",
    ]
    .iter()
    .any(|shape| {
        sentence.find(shape).is_some_and(|at| {
            let after = &sentence[at + shape.len()..];
            // "a creature", "two creatures", "a nontoken creature" — not "all creatures",
            // which is the wipe's shape rather than this one's.
            let mut words = after.split_whitespace().map(super::bare_word);
            match words.next() {
                Some("all" | "each" | "every") => false,
                Some(_) => after
                    .split_whitespace()
                    .take(4)
                    .map(super::bare_word)
                    .any(|word| PERMANENT_NOUNS.contains(&word) && !LAND_WORDS.contains(&word)),
                None => false,
            }
        })
    })
}

/// Whether the card is **targeted removal** — it answers one thing an opponent has.
///
/// Reads the clause's targets ([`targets`]) rather than the word "target" alone: a target
/// that is the caster's own permanent is a flicker or a sacrifice outlet, a target that is
/// a card in a graveyard is recursion or grave hate, a target spell is a counter. The one
/// shape without a target is the edict ("target opponent sacrifices a creature"). "Prevent
/// all damage that would be dealt to target creature" is protection, and stands down here.
pub(crate) fn is_removal(card: &CardFacts) -> bool {
    sentences(card)
        .iter()
        .any(|sentence| removes_a_target(sentence) || edicts(sentence))
}

// ---------- Board wipes ----------

/// Whether a wipe's quantifier governs a **permanent**, and not a "creature card" in a
/// graveyard or a "creature spell" on the stack.
fn quantified_permanent(sentence: &str) -> bool {
    quantified_noun(
        sentence,
        &["all ", "each ", "every "],
        WIPE_LIST_WORDS,
        PERMANENT_NOUNS,
        |noun, next| {
            !LAND_WORDS.contains(&noun)
                && !matches!(next, Some("card" | "cards" | "spell" | "spells"))
        },
    )
}

/// Whether the card is a **board wipe** — it removes permanents as a group.
///
/// A mass verb governing a quantified permanent ("destroy all creatures", "exile each
/// nonland permanent", "each player sacrifices all creatures they control"), damage to
/// each creature, or every creature shrinking. Lands are excluded from the noun list —
/// Armageddon is mass land denial, the bracket's category, not a wipe — though Jokulhaups
/// still reads through its artifacts and creatures. A wipe of your **own** side is a cost
/// ("sacrifice all creatures you control"), and a return **from** a graveyard is a
/// reanimation, not a bounce.
pub(crate) fn is_board_wipe(card: &CardFacts) -> bool {
    sentences(card).iter().any(|sentence| {
        if prevents(sentence) || sentence.contains("graveyard") || is_self_scoped(sentence) {
            return false;
        }
        if !quantified_permanent(sentence) {
            return false;
        }
        has_any_word(
            sentence,
            &[
                "destroy",
                "destroys",
                "exile",
                "exiles",
                "sacrifice",
                "sacrifices",
                "damage",
            ],
        ) || shrinks(sentence)
            || sentence.contains("owners' hands")
            || sentence.contains("owner's hand")
    })
}

// ---------- Counterspells ----------

/// Whether the card is a **counterspell** — it counters a spell (or an ability) on the
/// stack. "Counter target spell", "counter it unless its controller pays {3}", "counter
/// target activated or triggered ability". The word alone is nowhere near enough: a `+1/+1
/// counter` is the most common word on a creature, and "can't be countered" is a different
/// word again.
pub(crate) fn is_counterspell(card: &CardFacts) -> bool {
    sentences(card).iter().any(|sentence| {
        sentence.contains("counter target")
            || sentence.contains("counter that spell")
            || sentence.contains("counter that ability")
            || (sentence.contains("counter it") && sentence.contains("unless"))
            || sentence.contains("counter each spell")
            || sentence.contains("counter all spells")
    })
}

// ---------- Recursion ----------

/// Whether the card is **recursion** — it returns cards from a graveyard to hand or to the
/// battlefield.
///
/// The returned thing must be *something else*: Bloodghast's "return this card from your
/// graveyard to the battlefield" and Squee's "cast this card from your graveyard" are what
/// the card does for itself, and counting them tells a builder they run recursion when they
/// run a recursive creature. The self test reads the text right before "from … graveyard"
/// through [`own_names`], so a card named by its old-style oracle text is caught the same
/// way a reversible printing's `Name // Name` is.
pub(crate) fn is_recursion(card: &CardFacts) -> bool {
    sentences(card).iter().any(|sentence| {
        let Some(at) = sentence.find("graveyard") else {
            return false;
        };
        let (before, after) = sentence.split_at(at);
        let returns = has_any_word(before, &["return", "returns", "put", "puts"]);
        let destination = after.contains("to your hand")
            || after.contains("to its owner's hand")
            || after.contains("to their owner's hand")
            || after.contains("to their owners' hands")
            || after.contains("the battlefield");
        if !returns || !destination {
            return false;
        }
        // "… from your graveyard", "… from a graveyard": what stands before the "from".
        let Some(from) = before.rfind(" from ") else {
            return false;
        };
        !ends_with_self_reference(card, &before[..from])
    })
}

// ---------- Protection ----------

/// Whether the card is **protection** — it keeps something of yours from being answered.
///
/// A protective keyword *granted* ("gains hexproof", "creatures you control have
/// indestructible", "equipped creature has hexproof"), regeneration, damage prevention, a
/// phase-out, or a flicker of your own permanent ("exile target creature you control, then
/// return it"). What it is not is the keyword printed on the card itself: `Hexproof` on
/// its own line protects that one card, and "this creature gains indestructible" is the
/// same thing with a cost — both start with the card, and both are left alone.
pub(crate) fn is_protection(card: &CardFacts) -> bool {
    sentences(card).iter().any(|sentence| {
        if starts_with_self_reference(card, sentence) {
            return false;
        }
        let grants = has_any_word(sentence, &["gain", "gains", "has", "have"])
            && PROTECTION_WORDS.iter().any(|word| sentence.contains(word));
        if grants {
            return true;
        }
        if has_word(sentence, "regenerate") && !ends_with_self_reference(card, sentence) {
            return true;
        }
        if prevents(sentence) && has_word(sentence, "damage") {
            // "prevent all damage that would be dealt to this creature" is the card's own
            // armour, not a Fog.
            return !SELF_REFERENCES
                .iter()
                .any(|phrase| sentence.contains(&format!("dealt to {phrase}")));
        }
        if sentence.contains("phase out") || sentence.contains("phases out") {
            return true;
        }
        // A flicker: exile something of your own and get it back.
        let own_target = targets(sentence).contains(&Target::OwnPermanent);
        own_target && has_word(sentence, "exile") && has_word(sentence, "return")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::decks::analysis::test_fixtures::card;

    fn oracle(text: &str) -> CardFacts {
        card("x", "Nameless").oracle(text)
    }

    fn assert_all(predicate: fn(&CardFacts) -> bool, expected: bool, cases: &[(&str, &str)]) {
        for (name, text) in cases {
            let facts = card("x", name).oracle(text);
            assert_eq!(
                predicate(&facts),
                expected,
                "{name} should{} match: {text}",
                if expected { "" } else { " NOT" }
            );
        }
    }

    // ---------- Ramp ----------

    #[test]
    fn ramp_reads_rocks_dorks_fetches_and_extra_land_drops() {
        assert_all(
            is_ramp,
            true,
            &[
                ("Sol Ring", "{T}: Add {C}{C}."),
                ("Llanowar Elves", "{T}: Add {G}."),
                (
                    "Arcane Signet",
                    "{T}: Add one mana of any color in your commander's color identity.",
                ),
                (
                    "Mana Crypt",
                    "At the beginning of your upkeep, flip a coin. {T}: Add {C}{C}.",
                ),
                (
                    "Springleaf Drum",
                    "{T}, Tap an untapped creature you control: Add one mana of any color.",
                ),
                (
                    "Wild Growth",
                    "Enchant land\nWhenever enchanted land is tapped for mana, its controller adds an additional {G}.",
                ),
                (
                    "Chromatic Lantern",
                    "Lands you control have \"{T}: Add one mana of any color.\"\n{T}: Add one mana of any color.",
                ),
                (
                    "Cultivate",
                    "Search your library for up to two basic land cards, reveal those cards, put one onto the battlefield tapped and the other into your hand, then shuffle.",
                ),
                (
                    "Rampant Growth",
                    "Search your library for a basic land card, put that card onto the battlefield tapped, then shuffle.",
                ),
                (
                    "Farseek",
                    "Search your library for a Plains, Island, Swamp, or Mountain card, put it onto the battlefield tapped, then shuffle.",
                ),
                (
                    "Wood Elves",
                    "When this creature enters, you may search your library for a Forest card, put that card onto the battlefield, then shuffle.",
                ),
                (
                    "Sakura-Tribe Elder",
                    "Sacrifice this creature: Search your library for a basic land card, put that card onto the battlefield tapped, then shuffle.",
                ),
                (
                    "Exploration",
                    "You may play an additional land on each of your turns.",
                ),
                (
                    "Burgeoning",
                    "Whenever an opponent plays a land, you may put a land card from your hand onto the battlefield.",
                ),
                (
                    "Explore",
                    "You may play an additional land this turn.\nDraw a card.",
                ),
            ],
        );
    }

    /// The issue's own example first: a land search that goes to *hand* is card advantage.
    /// Then the rest — a land is a land, a ritual is once, a bare "land card" search swaps.
    #[test]
    fn ramp_declines_land_searches_to_hand_rituals_and_lands_themselves() {
        assert_all(
            is_ramp,
            false,
            &[
                (
                    "Land Tax",
                    "At the beginning of your upkeep, if an opponent controls more lands than you, you may search your library for up to three basic land cards, reveal them, put them into your hand, then shuffle.",
                ),
                (
                    "Sylvan Scrying",
                    "Search your library for a land card, reveal it, put it into your hand, then shuffle.",
                ),
                ("Dark Ritual", "Add {B}{B}{B}."),
                (
                    "Simian Spirit Guide",
                    "Exile this card from your hand: Add {R}.",
                ),
                (
                    "Crop Rotation",
                    "As an additional cost to cast this spell, sacrifice a land.\nSearch your library for a land card, put that card onto the battlefield, then shuffle.",
                ),
                (
                    "Splendid Reclamation",
                    "Return all land cards from your graveyard to the battlefield tapped.",
                ),
                ("Draw a card", "Draw a card."),
            ],
        );
        // Lands, however they fetch or tap.
        let forest = card("f", "Forest")
            .type_line("Basic Land — Forest")
            .oracle("({T}: Add {G}.)");
        assert!(!is_ramp(&forest));
        let wilds = card("w", "Evolving Wilds")
            .type_line("Land")
            .oracle("{T}, Sacrifice this land: Search your library for a basic land card, put it onto the battlefield tapped, then shuffle.");
        assert!(!is_ramp(&wilds));
        let arbor = card("a", "Dryad Arbor")
            .type_line("Land Creature — Forest Dryad")
            .oracle("({T}: Add {G}.)");
        assert!(!is_ramp(&arbor));
        // A modal double-faced spell whose BACK face is a land: the stored oracle text joins
        // both faces, so the back face's mana ability is in the text — and a counterspell
        // with a land behind it is still not a mana rock.
        let mdfc = card("j", "Jwari Disruption // Jwari Ruins")
            .type_line("Instant // Land")
            .oracle(
                "Counter target spell unless its controller pays {2}.\n//\nThis land enters tapped.\n{T}: Add {U}.",
            );
        assert!(!is_ramp(&mdfc), "a land on the back face is still a land");
        assert!(is_counterspell(&mdfc), "…and the front face is still read");
    }

    // ---------- Card draw ----------

    #[test]
    fn card_draw_reads_you_drawing() {
        assert_all(
            is_card_draw,
            true,
            &[
                ("Harmonize", "Draw three cards."),
                (
                    "Phyrexian Arena",
                    "At the beginning of your upkeep, you draw a card and you lose 1 life.",
                ),
                (
                    "Rhystic Study",
                    "Whenever an opponent casts a spell, you may draw a card unless that player pays {1}.",
                ),
                (
                    "Faithless Looting",
                    "Draw two cards, then discard two cards.",
                ),
                (
                    "Guardian Project",
                    "Whenever a nontoken creature you control enters, if it has a different name than each other creature you control and each creature card in your graveyard, draw a card.",
                ),
                (
                    "Brainstorm",
                    "Draw three cards, then put two cards from your hand on top of your library in any order.",
                ),
                (
                    "Blue Sun's Zenith",
                    "Target player draws X cards. Shuffle this card into its owner's library.",
                ),
                ("Prosperity", "Each player draws X cards."),
            ],
        );
    }

    #[test]
    fn card_draw_declines_triggers_replacements_and_other_peoples_draws() {
        assert_all(
            is_card_draw,
            false,
            &[
                (
                    "Niv-Mizzet, Parun",
                    "Whenever you draw a card, this creature deals 1 damage to any target.",
                ),
                (
                    "Alhammarret's Archive",
                    "If you would draw a card except the first one you draw in each of your draw steps, draw two cards instead.",
                ),
                (
                    "Narset",
                    "Each opponent can't draw more than one card each turn.",
                ),
                ("Necropotence", "Skip your draw step."),
                (
                    "Kederekt Parasite",
                    "Whenever an opponent draws a card, if you control a red permanent, this creature deals 1 damage to that player.",
                ),
                (
                    "Cycling reminder",
                    "Cycling {2} ({2}, Discard this card: Draw a card.)",
                ),
                (
                    "Impulse",
                    "Look at the top four cards of your library. Put one of them into your hand and the rest on the bottom of your library in any order.",
                ),
            ],
        );
        // A draw handed to one opponent is a drawback, not draw — and Howling Mine's card
        // for "that player" at their draw step is group draw the grammar declines to call
        // yours (Prosperity's "each player draws", above, it does).
        assert!(!is_card_draw(&oracle("Target opponent draws a card.")));
        assert!(!is_card_draw(&oracle(
            "At the beginning of each player's draw step, if this artifact is untapped, that player draws an additional card."
        )));
    }

    // ---------- Targeted removal ----------

    #[test]
    fn removal_reads_the_spot_removal_shapes() {
        assert_all(
            is_removal,
            true,
            &[
                (
                    "Swords to Plowshares",
                    "Exile target creature. Its controller gains life equal to its power.",
                ),
                (
                    "Beast Within",
                    "Destroy target permanent. Its controller creates a 3/3 green Beast creature token.",
                ),
                (
                    "Lightning Bolt",
                    "Lightning Bolt deals 3 damage to any target.",
                ),
                (
                    "Krosan Grip",
                    "Split second\nDestroy target artifact or enchantment.",
                ),
                (
                    "Cyclonic Rift",
                    "Return target nonland permanent you don't control to its owner's hand.\nOverload {6}{U}",
                ),
                (
                    "Chaos Warp",
                    "The owner of target permanent shuffles it into their library, then reveals the top card of their library. If it's a permanent card, they put it onto the battlefield.",
                ),
                (
                    "Grasp of Darkness",
                    "Target creature gets -4/-4 until end of turn.",
                ),
                (
                    "Prey Upon",
                    "Target creature you control fights target creature you don't control.",
                ),
                (
                    "Ram Through",
                    "Target creature you control deals damage equal to its power to target creature you don't control.",
                ),
                ("Liliana", "−2: Destroy target creature."),
                (
                    "Fleshbag Marauder",
                    "When this creature enters, each player sacrifices a creature.",
                ),
                (
                    "Plaguecrafter",
                    "When this creature enters, each player sacrifices a creature or planeswalker of their choice.",
                ),
                ("Vindicate", "Destroy target permanent."),
                (
                    "Assassin's Trophy",
                    "Destroy target permanent an opponent controls. Its controller may search their library for a basic land card, put it onto the battlefield, then shuffle.",
                ),
                (
                    "Reality Shift",
                    "Exile target creature. Its controller manifests the top card of their library.",
                ),
                (
                    "Return to Nature",
                    "Choose one —\n• Destroy target artifact.\n• Destroy target enchantment.\n• Exile target card from a graveyard.",
                ),
                ("Pinger", "{T}: This creature deals 1 damage to any target."),
                // Divided damage is worded with the plural noun and no "any target".
                (
                    "Fireball",
                    "This spell costs {1} more to cast for each target beyond the first.\nFireball deals X damage divided evenly, rounded down, among any number of targets.",
                ),
                (
                    "Arc Lightning",
                    "Arc Lightning deals 3 damage divided as you choose among one, two, or three targets.",
                ),
            ],
        );
    }

    /// Each a real card a sloppier reading of "target" would count: your own creature
    /// flickered, a card in a graveyard, a spell on the stack, a player, a prevention, a
    /// creature merely tapped or pumped.
    #[test]
    fn removal_declines_what_only_mentions_a_target() {
        assert_all(
            is_removal,
            false,
            &[
                (
                    "Ephemerate",
                    "Exile target creature you control, then return it to the battlefield under its owner's control.",
                ),
                (
                    "Regrowth",
                    "Return target card from your graveyard to your hand.",
                ),
                (
                    "Reanimate",
                    "Put target creature card from a graveyard onto the battlefield under your control. You lose life equal to its mana value.",
                ),
                ("Counterspell", "Counter target spell."),
                (
                    "Lava Spike",
                    "Lava Spike deals 3 damage to target player or planeswalker.",
                ),
                (
                    "Healing Salve",
                    "Prevent the next 3 damage that would be dealt to any target this turn.",
                ),
                (
                    "Blossoming Defense",
                    "Target creature you control gets +2/+2 and gains hexproof until end of turn.",
                ),
                (
                    "Giant Growth",
                    "Target creature gets +3/+3 until end of turn.",
                ),
                (
                    "Pacifism",
                    "Enchant creature\nEnchanted creature can't attack or block.",
                ),
                (
                    "Frost Breath",
                    "Tap up to two target creatures. Those creatures don't untap during their controller's next untap step.",
                ),
                (
                    "Sacrifice outlet",
                    "Sacrifice a creature: Destroy target creature you control.",
                ),
                (
                    "Bojuka Bog",
                    "This land enters tapped.\nWhen this land enters, exile target player's graveyard.",
                ),
                ("Hobble", "Target creature gets -2/-0 until end of turn."),
                ("Smallpox-ish", "Each player sacrifices a land."),
                ("Draw a card", "Draw a card."),
            ],
        );
    }

    // ---------- Board wipes ----------

    #[test]
    fn board_wipes_read_the_wrath_shapes() {
        assert_all(
            is_board_wipe,
            true,
            &[
                (
                    "Wrath of God",
                    "Destroy all creatures. They can't be regenerated.",
                ),
                (
                    "Farewell",
                    "Choose one or more —\n• Exile all artifacts.\n• Exile all creatures.\n• Exile all enchantments.\n• Exile all graveyards.",
                ),
                (
                    "Blasphemous Act",
                    "This spell costs {1} less to cast for each creature on the battlefield.\nBlasphemous Act deals 13 damage to each creature.",
                ),
                (
                    "Toxic Deluge",
                    "As an additional cost to cast this spell, pay X life.\nAll creatures get -X/-X until end of turn.",
                ),
                ("Evacuation", "Return all creatures to their owners' hands."),
                (
                    "Ruinous Ultimatum",
                    "Destroy all nonland permanents your opponents control.",
                ),
                (
                    "Austere Command",
                    "Choose two —\n• Destroy all artifacts.\n• Destroy all enchantments.\n• Destroy all creatures with mana value 3 or less.\n• Destroy all creatures with mana value 4 or greater.",
                ),
                (
                    "Jokulhaups",
                    "Destroy all artifacts, creatures, and lands. They can't be regenerated.",
                ),
                (
                    "Living Death-ish",
                    "Each player sacrifices all creatures they control.",
                ),
                (
                    "Massacre Girl-ish",
                    "When this creature enters, all other creatures get -1/-1 until end of turn.",
                ),
                ("Pyroclasm", "Pyroclasm deals 2 damage to each creature."),
                (
                    "Settle the Wreckage",
                    "Exile all attacking creatures target player controls. That player may search their library for that many basic land cards, put those cards onto the battlefield tapped, then shuffle.",
                ),
            ],
        );
    }

    #[test]
    fn board_wipes_decline_land_denial_your_own_side_graveyards_and_spot_removal() {
        assert_all(
            is_board_wipe,
            false,
            &[
                ("Armageddon", "Destroy all lands."),
                (
                    "Swords to Plowshares",
                    "Exile target creature. Its controller gains life equal to its power.",
                ),
                (
                    "Rise of the Dark Realms",
                    "Put all creature cards from all graveyards onto the battlefield under your control.",
                ),
                (
                    "Bojuka Bog",
                    "When this land enters, exile target player's graveyard.",
                ),
                (
                    "Rest in Peace",
                    "When this enchantment enters, exile all graveyards.",
                ),
                ("Self-sacrifice", "Sacrifice all creatures you control."),
                (
                    "Fog",
                    "Prevent all combat damage that would be dealt this turn.",
                ),
                (
                    "Fleshbag Marauder",
                    "When this creature enters, each player sacrifices a creature.",
                ),
                ("Anthem", "Each creature you control gets +1/+1."),
                ("Innocent Blood-ish tax", "Each player sacrifices a land."),
                ("Tap-down", "Tap all creatures your opponents control."),
                (
                    "Prevent all",
                    "Prevent all damage that would be dealt to each creature this turn.",
                ),
                ("Draw a card", "Draw a card."),
            ],
        );
    }

    // ---------- Counterspells ----------

    #[test]
    fn counterspells_counter_something_on_the_stack() {
        assert_all(
            is_counterspell,
            true,
            &[
                ("Counterspell", "Counter target spell."),
                (
                    "Mana Leak",
                    "Counter target spell unless its controller pays {3}.",
                ),
                ("Essence Scatter", "Counter target creature spell."),
                (
                    "Force of Negation",
                    "If it's not your turn, you may exile a blue card from your hand rather than pay this spell's mana cost.\nCounter target noncreature spell. If that spell is countered this way, exile it instead of putting it into its owner's graveyard.",
                ),
                ("Stifle", "Counter target activated or triggered ability."),
                (
                    "Dovin's Veto",
                    "This spell can't be countered.\nCounter target noncreature spell.",
                ),
                (
                    "Mystic Confluence",
                    "Choose three. You may choose the same mode more than once.\n• Counter target spell unless its controller pays {3}.\n• Return target creature to its owner's hand.\n• Draw a card.",
                ),
                (
                    "Chalice",
                    "Whenever a player casts a spell with mana value equal to the number of charge counters on this artifact, counter that spell.",
                ),
                (
                    "Rhystic Counter",
                    "Whenever an opponent casts a spell, counter it unless that player pays {1}.",
                ),
            ],
        );
    }

    #[test]
    fn counterspells_decline_counters_of_the_other_kind() {
        assert_all(
            is_counterspell,
            false,
            &[
                (
                    "Hardened Scales",
                    "If one or more +1/+1 counters would be put on a creature you control, that many plus one +1/+1 counters are put on it instead.",
                ),
                ("Pump", "Put a +1/+1 counter on target creature."),
                (
                    "Vexing Shusher",
                    "This spell can't be countered.\n{R/G}: Target spell can't be countered.",
                ),
                (
                    "Remove a counter",
                    "Remove a loyalty counter from target planeswalker.",
                ),
                ("Draw a card", "Draw a card."),
            ],
        );
    }

    // ---------- Tutors (the bracket's reader, re-exported) ----------

    #[test]
    fn the_tutor_role_is_the_brackets_tutor() {
        assert!(super::super::bracket::is_tutor(&oracle(
            "Search your library for a card, then shuffle and put that card into your hand."
        )));
        assert!(!super::super::bracket::is_tutor(&oracle(
            "Search your library for a basic land card, put it onto the battlefield tapped, then shuffle."
        )));
    }

    // ---------- Recursion ----------

    #[test]
    fn recursion_returns_something_else_from_a_graveyard() {
        assert_all(
            is_recursion,
            true,
            &[
                (
                    "Regrowth",
                    "Return target card from your graveyard to your hand.",
                ),
                (
                    "Eternal Witness",
                    "When this creature enters, you may return target card from your graveyard to your hand.",
                ),
                (
                    "Reanimate",
                    "Put target creature card from a graveyard onto the battlefield under your control. You lose life equal to its mana value.",
                ),
                (
                    "Sun Titan",
                    "Vigilance\nWhenever this creature enters or attacks, you may return target permanent card with mana value 3 or less from your graveyard to the battlefield.",
                ),
                (
                    "Splendid Reclamation",
                    "Return all land cards from your graveyard to the battlefield tapped.",
                ),
                (
                    "Rise of the Dark Realms",
                    "Put all creature cards from all graveyards onto the battlefield under your control.",
                ),
                (
                    "Animate Dead-ish",
                    "Return target creature card from an opponent's graveyard to the battlefield under your control.",
                ),
            ],
        );
    }

    #[test]
    fn recursion_declines_a_card_returning_itself_and_grave_hate() {
        assert_all(
            is_recursion,
            false,
            &[
                (
                    "Bloodghast",
                    "This creature can't block.\nLandfall — Whenever a land you control enters, you may return this card from your graveyard to the battlefield.",
                ),
                (
                    "Squee, Goblin Nabob",
                    "You may cast this card from your graveyard.",
                ),
                (
                    "Gravecrawler",
                    "This creature can't block.\nYou may cast this card from your graveyard as long as you control a Zombie.",
                ),
                (
                    "Bojuka Bog",
                    "When this land enters, exile target player's graveyard.",
                ),
                (
                    "Mill",
                    "Target player mills three cards. Put the top three cards of your library into your graveyard.",
                ),
                (
                    "Delve reminder",
                    "Delve (Each card you exile from your graveyard while casting this spell pays for {1}.)",
                ),
                (
                    "Grave trigger",
                    "Whenever a creature card is put into your graveyard from anywhere, you gain 1 life.",
                ),
                ("Draw a card", "Draw a card."),
            ],
        );
        // Old-style oracle text names the card; a reversible printing repeats the name.
        let named = card("s", "Squee, Goblin Nabob")
            .oracle("At the beginning of your upkeep, you may return Squee, Goblin Nabob from your graveyard to your hand.");
        assert!(
            !is_recursion(&named),
            "returning yourself by name is still yourself"
        );
        let reversible = card("r", "Squee, Goblin Nabob // Squee, Goblin Nabob")
            .oracle("At the beginning of your upkeep, you may return Squee, Goblin Nabob from your graveyard to your hand.");
        assert!(
            !is_recursion(&reversible),
            "a reversible printing answers to each half"
        );
    }

    // ---------- Protection ----------

    #[test]
    fn protection_reads_what_is_granted_prevented_or_flickered() {
        assert_all(
            is_protection,
            true,
            &[
                (
                    "Swiftfoot Boots",
                    "Equipped creature has hexproof and haste.\nEquip {1}",
                ),
                (
                    "Lightning Greaves",
                    "Equipped creature has haste and shroud.\nEquip {0}",
                ),
                (
                    "Heroic Intervention",
                    "Permanents you control gain hexproof and indestructible until end of turn.",
                ),
                (
                    "Teferi's Protection",
                    "Until your next turn, your life total can't change and you gain protection from everything. All permanents you control phase out.",
                ),
                (
                    "Blossoming Defense",
                    "Target creature you control gets +2/+2 and gains hexproof until end of turn.",
                ),
                (
                    "Boros Charm-ish",
                    "Permanents you control gain indestructible until end of turn.",
                ),
                (
                    "Fog",
                    "Prevent all combat damage that would be dealt this turn.",
                ),
                (
                    "Ephemerate",
                    "Exile target creature you control, then return it to the battlefield under its owner's control.",
                ),
                ("Regeneration", "{G}: Regenerate target creature."),
                (
                    "Asceticism",
                    "Creatures you control have hexproof.\n{1}{G}: Regenerate target creature.",
                ),
                ("Leyline of Sanctity", "You have hexproof."),
                (
                    "Slip Out the Back",
                    "Put a +1/+1 counter on target creature. It phases out.",
                ),
            ],
        );
    }

    /// The keyword printed on the card is the card's own armour: `Hexproof` on a line of its
    /// own, "this creature gains indestructible", "regenerate this creature".
    #[test]
    fn protection_declines_a_card_protecting_itself() {
        assert_all(
            is_protection,
            false,
            &[
                ("Slippery Bogle", "Hexproof"),
                (
                    "Darksteel Colossus",
                    "Trample, indestructible\nIf this card would be put into a graveyard from anywhere, reveal it and shuffle it into its owner's library instead.",
                ),
                (
                    "Boros Reckoner-ish",
                    "{R/W}: This creature gains first strike until end of turn.",
                ),
                (
                    "Self-indestructible",
                    "{1}: This creature gains indestructible until end of turn.",
                ),
                ("Regenerator", "{B}: Regenerate this creature."),
                (
                    "Armoured",
                    "Prevent all damage that would be dealt to this creature.",
                ),
                ("Protection keyword", "Protection from red"),
                ("Hexproof from", "Hexproof from blue"),
                (
                    "Loses",
                    "Creatures your opponents control lose hexproof and indestructible.",
                ),
                ("Draw a card", "Draw a card."),
            ],
        );
        // By name, old-style text: still itself.
        let named = card("m", "Mirri the Cursed").oracle("{B}: Regenerate Mirri the Cursed.");
        assert!(!is_protection(&named));
    }

    // ---------- Shared readers ----------

    #[test]
    fn targets_resolve_each_target_phrase() {
        assert_eq!(
            targets("target creature you control fights target creature you don't control"),
            vec![Target::OwnPermanent, Target::OthersPermanent]
        );
        assert_eq!(
            targets("deals 3 damage to any target"),
            vec![Target::OthersPermanent]
        );
        assert_eq!(targets("counter target spell"), vec![Target::Other]);
        assert_eq!(
            targets("exile target creature card from a graveyard"),
            vec![Target::Other]
        );
        assert_eq!(
            targets("target player sacrifices a creature"),
            vec![Target::Other]
        );
        assert_eq!(
            targets("destroy target nonblack nonartifact creature"),
            vec![Target::OthersPermanent]
        );
        assert_eq!(
            targets("exile target player's graveyard"),
            vec![Target::Other]
        );
        assert_eq!(targets("untargetable"), Vec::<Target>::new());
        // The plural noun counts only for divided damage; the verb "targets" never does.
        assert_eq!(
            targets("deals 3 damage divided as you choose among one, two, or three targets"),
            vec![Target::OthersPermanent]
        );
        assert_eq!(
            targets("whenever a spell an opponent controls targets a creature you control"),
            Vec::<Target>::new()
        );
    }

    #[test]
    fn shrinking_needs_both_signs() {
        assert!(shrinks("target creature gets -4/-4 until end of turn"));
        assert!(shrinks("all creatures get -x/-x until end of turn"));
        assert!(!shrinks("target creature gets -2/-0 until end of turn"));
        assert!(!shrinks("target creature gets +2/-2 until end of turn"));
        assert!(!shrinks("target creature gets +3/+3 until end of turn"));
    }

    /// Reminder text is stripped before anything is read, so a parenthetical can neither
    /// create nor hide a role.
    #[test]
    fn reminder_text_creates_no_role() {
        let cycling = oracle("Cycling {2} ({2}, Discard this card: Draw a card.)");
        assert!(!is_card_draw(&cycling));
        let flashback = oracle(
            "Flashback {3}{G} (You may cast this card from your graveyard for its flashback cost. Then exile it.)",
        );
        assert!(!is_recursion(&flashback));
    }
}

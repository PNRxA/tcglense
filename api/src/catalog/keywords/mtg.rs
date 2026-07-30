//! The Magic: The Gathering keyword glossary.
//!
//! Every keyword ability, keyword action and ability word a card can name, with the
//! official reminder text where one exists (parentheses stripped, and phrased about
//! "this creature" rather than any particular card) and a written-to-match explanation
//! where one doesn't — ability words in particular have no reminder text at all,
//! because they carry no rules meaning of their own.
//!
//! Ordering here doesn't matter: [`super::glossary`] sorts by name. Two fields need
//! care when adding a row:
//!
//! * `name` must be spelled exactly as card text spells it (`"First strike"`,
//!   `"For Mirrodin!"`, `"The Ring tempts you"`) — the SPA matches this string against
//!   a card's rules text, and the keyword's URL is derived from it.
//! * `match_mode` is how aggressively that name may be matched. Default to
//!   [`MatchMode::Anywhere`] only if you cannot write a realistic rules sentence in
//!   which the word means something else; see [`MatchMode`] for the other two.

use super::Entry;
use super::KeywordKind::{Ability, AbilityWord, Action};
use super::MatchMode::{AbilityLine, Anywhere, Never};

pub(super) const ENTRIES: &[Entry] = &[
    Entry {
        name: "Abandon",
        kind: Action,
        text: "To abandon an ongoing scheme is to turn it face down and put it on the bottom of its \
              owner's scheme deck.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Absorb",
        kind: Ability,
        text: "If a source would deal damage to this creature, prevent that much of that damage. \
              Absorb only prevents damage dealt to the creature that has it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Activate",
        kind: Action,
        text: "To activate an activated ability is to put it on the stack and pay its costs. It \
              then waits to resolve and can be responded to.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Adamant",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether at least three mana of a single \
              color was spent to cast the spell. If so, the spell gets an extra effect.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Adapt",
        kind: Action,
        text: "If this creature has no +1/+1 counters on it, put that many +1/+1 counters on it. If \
              it already has any, adapt does nothing.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Addendum",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether you cast the spell during your \
              main phase, adding a bonus effect if you did.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Affinity",
        kind: Ability,
        text: "This spell costs {1} less to cast for each permanent you control of the stated kind, \
              most often for each artifact you control.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Afflict",
        kind: Ability,
        text: "Whenever this creature becomes blocked, defending player loses that much life.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Afterlife",
        kind: Ability,
        text: "When this creature dies, create that many 1/1 white and black Spirit creature tokens \
              with flying.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Aftermath",
        kind: Ability,
        text: "Cast this spell only from your graveyard. Then exile it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Airbend",
        kind: Action,
        text: "Exile it. While it's exiled, its owner may cast it for {2} rather than its mana \
              cost.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Alliance",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever another creature enters the \
              battlefield under your control.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Amass",
        kind: Action,
        text: "Put that many +1/+1 counters on an Army you control. If you don't control one, \
              create a 0/0 black Army creature token first (of the named creature type, if one is \
              given).",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Amplify",
        kind: Ability,
        text: "As this creature enters, put that many +1/+1 counters on it for each card of the \
              stated creature type you reveal in your hand.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Annihilator",
        kind: Ability,
        text: "Whenever this creature attacks, defending player sacrifices that many permanents.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Ascend",
        kind: Ability,
        text: "If you control ten or more permanents, you get the city's blessing for the rest of \
              the game.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Assemble",
        kind: Action,
        text: "To assemble a Contraption, put the top card of your Contraption deck face up onto \
              one of your three sprockets.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Assist",
        kind: Ability,
        text: "Another player can pay up to the generic portion of this spell's cost. You choose \
              how much help to accept as you cast it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Attach",
        kind: Action,
        text: "To attach an Aura, Equipment, or Fortification is to take it from where it is and \
              put it onto the object or player it becomes attached to.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Augment",
        kind: Ability,
        text: "Pay the augment cost and reveal this card from your hand: Combine it with target \
              host. Augment only as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Aura swap",
        kind: Ability,
        text: "Pay the aura swap cost: Exchange this Aura with an Aura card in your hand.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Awaken",
        kind: Ability,
        text: "If you cast this spell for its awaken cost, also put that many +1/+1 counters on \
              target land you control and it becomes a 0/0 Elemental creature with haste. It's \
              still a land.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Backup",
        kind: Ability,
        text: "When this creature enters, put that many +1/+1 counters on target creature. If \
              that's another creature, it also gains the abilities printed below the backup \
              keyword until end of turn.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Banding",
        kind: Ability,
        text: "Any creatures with banding, and up to one without, can attack in a band. Bands are \
              blocked as a group. If any creatures with banding you control are blocking or being \
              blocked by a creature, you divide that creature's combat damage, not its controller, \
              among any of the creatures it's being blocked by or is blocking.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Bands with other",
        kind: Ability,
        text: "Creatures with \"bands with other [quality]\" can attack in a band with creatures of \
              that quality; the attacking player chooses how a blocker assigns its combat damage \
              among the band.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Bargain",
        kind: Ability,
        text: "You may sacrifice an artifact, enchantment, or token as you cast this spell.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Basic landcycling",
        kind: Ability,
        text: "Pay the basic landcycling cost and discard this card: Search your library for a \
              basic land card, reveal it, put it into your hand, then shuffle.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Battalion",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger when this creature and at least two \
              other creatures attack.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Battle cry",
        kind: Ability,
        text: "Whenever this creature attacks, each other attacking creature gets +1/+0 until end \
              of turn.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Behold",
        kind: Action,
        text: "To behold a quality — for example, behold a Dragon — you may reveal a card with that \
              quality from your hand or choose an untapped permanent with that quality you \
              control.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Bestow",
        kind: Ability,
        text: "If you cast this card for its bestow cost, it's an Aura spell with enchant creature. \
              It becomes a creature again if it's not attached to a creature.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Blight",
        kind: Action,
        text: "To blight a number is to put that many -1/-1 counters on a creature you control.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Blitz",
        kind: Ability,
        text: "If you cast this spell for its blitz cost, it gains haste and \"When this creature \
              dies, draw a card.\" Sacrifice it at the beginning of the next end step.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Bloodrush",
        kind: AbilityWord,
        text: "An ability word on cards with \"{cost}, Discard this card: Target attacking creature \
              gets +X/+X until end of turn\" — it has no rules meaning of its own.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Bloodthirst",
        kind: Ability,
        text: "If an opponent was dealt damage this turn, this creature enters with that many +1/+1 \
              counters on it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Boast",
        kind: Ability,
        text: "Activate a boast ability only if this creature attacked this turn and only once each \
              turn.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Bolster",
        kind: Action,
        text: "Choose a creature you control with the least toughness, or tied for least toughness, \
              then put that many +1/+1 counters on it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Bushido",
        kind: Ability,
        text: "Whenever this creature blocks or becomes blocked, it gets that much +N/+N until end \
              of turn.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Buyback",
        kind: Ability,
        text: "You may pay an additional buyback cost as you cast this spell. If you do, put this \
              card into your hand as it resolves.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Cascade",
        kind: Ability,
        text: "When you cast this spell, exile cards from the top of your library until you exile a \
              nonland card whose mana value is less than this spell's mana value. You may cast it \
              without paying its mana cost. Put the exiled cards on the bottom of your library in \
              a random order.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Cast",
        kind: Action,
        text: "To cast a spell is to move it to the stack, make the choices it calls for, and pay \
              its costs. It can then be responded to before it resolves.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Casualty",
        kind: Ability,
        text: "As you cast this spell, you may sacrifice a creature with power equal to or greater \
              than the casualty number. When you do, copy this spell and you may choose new \
              targets for the copy.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Celebration",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether two or more nonland permanents \
              entered the battlefield under your control this turn.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Champion",
        kind: Ability,
        text: "When this permanent enters, sacrifice it unless you exile another permanent of the \
              stated kind you control. When this permanent leaves the battlefield, that card \
              returns to the battlefield.",
        parameterized: true,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Changeling",
        kind: Ability,
        text: "This card is every creature type.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Channel",
        kind: AbilityWord,
        text: "An ability word marking activated abilities you pay for by discarding the card from \
              your hand, letting the card do something without ever being cast.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Choose a Background",
        kind: Ability,
        text: "You can have a Background as a second commander.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Chroma",
        kind: AbilityWord,
        text: "An ability word marking abilities whose effect scales with the number of a \
              particular mana symbol found among the relevant cards or permanents.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Cipher",
        kind: Ability,
        text: "Then you may exile this spell card encoded on a creature you control. Whenever that \
              creature deals combat damage to a player, its controller may cast a copy of the \
              encoded card without paying its mana cost.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Clash",
        kind: Action,
        text: "Each clashing player reveals the top card of their library, then puts that card on \
              the top or bottom of their library. A player wins the clash if their card had a \
              higher mana value than every other clashing player's card.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Cleave",
        kind: Ability,
        text: "You may cast this spell for its cleave cost. If you do, remove the words in square \
              brackets.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Cloak",
        kind: Action,
        text: "To cloak a card, put it onto the battlefield face down as a 2/2 creature with ward \
              {2}. Turn it face up any time for its mana cost if it's a creature card.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Cohort",
        kind: AbilityWord,
        text: "An ability word marking activated abilities whose cost is tapping this creature \
              together with another untapped Ally you control.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Collect evidence",
        kind: Action,
        text: "Exile cards from your graveyard with total mana value that number or greater. If you \
              can't, no evidence is collected.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Commander ninjutsu",
        kind: Ability,
        text: "Pay the commander ninjutsu cost and return an unblocked attacker you control to \
              hand: Put this card onto the battlefield from your hand or the command zone tapped \
              and attacking.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Companion",
        kind: Ability,
        text: "If this card is your chosen companion, you may put it into your hand from outside \
              the game for {3} any time you could cast a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Compleated",
        kind: Ability,
        text: "A Phyrexian mana symbol in this card's cost can be paid with either of its colors or \
              with 2 life. If life was paid that way, this planeswalker enters with that many \
              fewer loyalty counters.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Conjure",
        kind: Action,
        text: "A digital-only action: create the named card in the specified zone. A conjured card \
              isn't from your deck but is a real card in every other way.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Connive",
        kind: Action,
        text: "Draw a card, then discard a card. If a nonland card is discarded this way, put a \
              +1/+1 counter on the creature that connived.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Conspire",
        kind: Ability,
        text: "As you cast this spell, you may tap two untapped creatures you control that share a \
              color with it. When you do, copy it. You may choose new targets for the copy.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Constellation",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever an enchantment enters the \
              battlefield under your control.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Converge",
        kind: AbilityWord,
        text: "An ability word marking abilities whose effect scales with the number of colors of \
              mana spent to cast the spell.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Convert",
        kind: Action,
        text: "To convert a transforming double-faced card is to turn it to its other face; casting \
              a card \"converted\" means casting its back face.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Convoke",
        kind: Ability,
        text: "Your creatures can help cast this spell. Each creature you tap while casting this \
              spell pays for {1} or one mana of that creature's color.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Corrupted",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether an opponent has three or more \
              poison counters.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Council's dilemma",
        kind: AbilityWord,
        text: "An ability word marking abilities where each player votes and every individual vote \
              adds to the effect, rather than only the winning option happening.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Counter",
        kind: Action,
        text: "To counter a spell or ability is to cancel it — it's removed from the stack and none \
              of its effects happen. A countered spell is put into its owner's graveyard.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Coven",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether you control three or more \
              creatures with different powers.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Covercast",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever you cast another instant or \
              sorcery spell if five or more mana was spent to cast it.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Craft",
        kind: Ability,
        text: "Pay the craft cost, exile this permanent, and exile the listed materials from among \
              other permanents you control and/or cards in your graveyard: Return this card \
              transformed under its owner's control. Craft only as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Create",
        kind: Action,
        text: "To create a token is to put a token with the listed characteristics onto the \
              battlefield under your control. If an effect would create a token but the token \
              can't exist, nothing happens.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Crew",
        kind: Ability,
        text: "Tap any number of creatures you control with total power equal to or greater than \
              the crew number: This Vehicle becomes an artifact creature until end of turn.",
        parameterized: true,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Cumulative upkeep",
        kind: Ability,
        text: "At the beginning of your upkeep, put an age counter on this permanent, then \
              sacrifice it unless you pay its upkeep cost for each age counter on it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Cycling",
        kind: Ability,
        text: "Pay the cycling cost, Discard this card: Draw a card.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Dash",
        kind: Ability,
        text: "You may cast this spell for its dash cost. If you do, it gains haste, and it's \
              returned from the battlefield to its owner's hand at the beginning of the next end \
              step.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Daybound",
        kind: Ability,
        text: "If a player casts no spells during their own turn, it becomes night next turn.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Deathtouch",
        kind: Ability,
        text: "Any amount of damage this creature deals to a creature is enough to destroy it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Decayed",
        kind: Ability,
        text: "This creature can't block. When it attacks, sacrifice it at end of combat.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Defender",
        kind: Ability,
        text: "This creature can't attack.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Delirium",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether there are four or more card \
              types among the cards in your graveyard.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Delve",
        kind: Ability,
        text: "Each card you exile from your graveyard while casting this spell pays for {1}.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Demonstrate",
        kind: Ability,
        text: "When you cast this spell, you may copy it. If you do, choose an opponent to also \
              copy it. Players may choose new targets for their copies.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Descend",
        kind: AbilityWord,
        text: "An ability word marking abilities that check how many permanent cards are in your \
              graveyard — Descend 4 wants four or more, Descend 8 wants eight or more.",
        parameterized: true,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Desertwalk",
        kind: Ability,
        text: "This creature can't be blocked as long as defending player controls a Desert.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Destroy",
        kind: Action,
        text: "To destroy a permanent is to move it from the battlefield to its owner's graveyard. \
              A permanent with indestructible can't be destroyed, and regeneration replaces \
              destruction.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Detain",
        kind: Action,
        text: "Until your next turn, that permanent can't attack or block and its activated \
              abilities can't be activated.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Dethrone",
        kind: Ability,
        text: "Whenever this creature attacks the player with the most life or tied for the most \
              life, put a +1/+1 counter on it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Devoid",
        kind: Ability,
        text: "This card has no color.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Devour",
        kind: Ability,
        text: "As this creature enters, you may sacrifice any number of creatures. It enters with a \
              number of +1/+1 counters on it equal to the devour number times the number of \
              creatures sacrificed this way.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Disappear",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether a permanent left the \
              battlefield under your control this turn.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Discard",
        kind: Action,
        text: "To discard a card is to move it from its owner's hand to that player's graveyard. \
              Unless a spell or ability says otherwise, the card's owner chooses which card to \
              discard.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Discover",
        kind: Action,
        text: "Exile cards from the top of your library until you exile a nonland card with mana \
              value that number or less. Cast it without paying its mana cost or put it into your \
              hand, then put the rest of the exiled cards on the bottom of your library in a \
              random order.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Disguise",
        kind: Ability,
        text: "You may cast this card face down for {3} as a 2/2 creature with ward {2}. Turn it \
              face up any time for its disguise cost.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Disturb",
        kind: Ability,
        text: "You may cast this card from your graveyard transformed for its disturb cost.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Doctor's companion",
        kind: Ability,
        text: "You can have two commanders if the other is the Doctor.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Domain",
        kind: AbilityWord,
        text: "An ability word marking abilities whose effect scales with the number of basic land \
              types among lands you control.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Double",
        kind: Action,
        text: "To double a number is to add an amount equal to it; doubling counters on a permanent \
              puts that many more of each kind of counter on it.",
        parameterized: true,
        match_mode: Never,
    },
    Entry {
        name: "Double agenda",
        kind: Ability,
        text: "Start the game with this conspiracy face down in the command zone and secretly \
              choose two different card names. You may turn this conspiracy face up any time and \
              reveal those names.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Double strike",
        kind: Ability,
        text: "This creature deals both first-strike and regular combat damage.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Double team",
        kind: Ability,
        text: "When this creature attacks, conjure a duplicate of it into your hand, then both of \
              them perpetually lose double team.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Dredge",
        kind: Ability,
        text: "If you would draw a card, you may mill that many cards instead. If you do, return \
              this card from your graveyard to your hand.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Earthbend",
        kind: Action,
        text: "Target land you control becomes a 0/0 creature with haste that's still a land. Put \
              that many +1/+1 counters on it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Echo",
        kind: Ability,
        text: "At the beginning of your upkeep, if this permanent came under your control since the \
              beginning of your last upkeep, sacrifice it unless you pay its echo cost.",
        parameterized: true,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Eerie",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever an enchantment you control \
              enters and whenever you fully unlock a Room.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Embalm",
        kind: Ability,
        text: "Pay the embalm cost and exile this card from your graveyard: Create a token that's a \
              copy of it, except it's a white Zombie with no mana cost. Embalm only as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Emerge",
        kind: Ability,
        text: "You may cast this spell by sacrificing a creature and paying the emerge cost reduced \
              by that creature's mana value.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Eminence",
        kind: AbilityWord,
        text: "An ability word marking abilities on a legendary creature that function while it's \
              in the command zone as well as while it's on the battlefield.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Enchant",
        kind: Ability,
        text: "This Aura can be attached only to the stated kind of object or player, and it's put \
              into its owner's graveyard if it ever becomes attached illegally.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Encore",
        kind: Ability,
        text: "Pay the encore cost and exile this card from your graveyard: For each opponent, \
              create a token copy that attacks that opponent this turn if able. They gain haste. \
              Sacrifice them at the beginning of the next end step. Activate only as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Endure",
        kind: Action,
        text: "Put that many +1/+1 counters on this creature or create a white Spirit creature \
              token with power and toughness equal to that number. You choose which as the ability \
              resolves.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Enlist",
        kind: Ability,
        text: "As this creature attacks, you may tap a nonattacking creature you control without \
              summoning sickness. When you do, add its power to this creature's until end of turn.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Enrage",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever this creature is dealt \
              damage.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Entwine",
        kind: Ability,
        text: "Choose both modes if you pay the entwine cost.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Epic",
        kind: Ability,
        text: "For the rest of the game, you can't cast spells. At the beginning of each of your \
              upkeeps, copy this spell except for its epic ability. You may choose new targets for \
              the copy.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Equip",
        kind: Ability,
        text: "Pay the equip cost: Attach this Equipment to target creature you control. Equip only \
              as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Escalate",
        kind: Ability,
        text: "Pay this cost for each mode chosen beyond the first.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Escape",
        kind: Ability,
        text: "You may cast this card from your graveyard for its escape cost.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Eternalize",
        kind: Ability,
        text: "Pay the eternalize cost and exile this card from your graveyard: Create a token \
              that's a copy of it, except it's a 4/4 black Zombie with no mana cost. Eternalize \
              only as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Evoke",
        kind: Ability,
        text: "You may cast this spell for its evoke cost. If you do, it's sacrificed when it \
              enters.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Evolve",
        kind: Ability,
        text: "Whenever a creature you control enters, if that creature has greater power or \
              toughness than this creature, put a +1/+1 counter on this creature.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Exalted",
        kind: Ability,
        text: "Whenever a creature you control attacks alone, that creature gets +1/+1 until end of \
              turn.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Exchange",
        kind: Action,
        text: "To exchange objects, life totals, or control of permanents. If the entire exchange \
              can't be completed, no part of it happens.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Exert",
        kind: Action,
        text: "You can exert a creature as it attacks by choosing to do so. An exerted creature \
              won't untap during your next untap step.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Exhaust",
        kind: Ability,
        text: "Activate each exhaust ability only once.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Exile",
        kind: Action,
        text: "To exile a card or permanent is to move it to the exile zone, a zone outside the \
              game. Exiled cards are face up unless an effect says they're exiled face down.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Exploit",
        kind: Ability,
        text: "When this creature enters, you may sacrifice a creature.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Explore",
        kind: Action,
        text: "Reveal the top card of your library. If it's a land, put it into your hand. \
              Otherwise, put a +1/+1 counter on the exploring creature, then put the card back on \
              top of your library or into your graveyard.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Extort",
        kind: Ability,
        text: "Whenever you cast a spell, you may pay {W/B}. If you do, each opponent loses 1 life \
              and you gain that much life.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Fabricate",
        kind: Ability,
        text: "When this creature enters, put that many +1/+1 counters on it or create that many \
              1/1 colorless Servo artifact creature tokens.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Face a villainous choice",
        kind: Action,
        text: "Each affected player chooses one of the two listed options, and that option happens \
              for that player. The choice is made as the spell or ability resolves.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Fading",
        kind: Ability,
        text: "This permanent enters with that many fade counters on it. At the beginning of your \
              upkeep, remove a fade counter from it. If you can't, sacrifice it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Fateful hour",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether you have 5 or less life.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Fateseal",
        kind: Action,
        text: "Look at that many cards from the top of target opponent's library, then put any \
              number of them on the bottom of that library and the rest on top in any order.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Fathomless descent",
        kind: AbilityWord,
        text: "An ability word marking abilities whose effect scales with the number of permanent \
              cards in your graveyard.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Fear",
        kind: Ability,
        text: "This creature can't be blocked except by artifact creatures and/or black creatures.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Ferocious",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether you control a creature with \
              power 4 or greater.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Fight",
        kind: Action,
        text: "Each of those creatures deals damage equal to its power to the other. This damage \
              isn't combat damage, and no damage is dealt if either creature is no longer on the \
              battlefield or is no longer a creature.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Firebending",
        kind: Ability,
        text: "Whenever this creature attacks, add that much {R}. This mana lasts until end of \
              combat.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "First strike",
        kind: Ability,
        text: "This creature deals combat damage before creatures without first strike.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Flanking",
        kind: Ability,
        text: "Whenever a creature without flanking blocks this creature, the blocking creature \
              gets -1/-1 until end of turn.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Flash",
        kind: Ability,
        text: "You may cast this spell any time you could cast an instant.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Flashback",
        kind: Ability,
        text: "You may cast this card from your graveyard for its flashback cost. Then exile it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Flurry",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever you cast your second spell \
              each turn.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Flying",
        kind: Ability,
        text: "This creature can't be blocked except by creatures with flying or reach.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "For Mirrodin!",
        kind: Ability,
        text: "When this Equipment enters, create a 2/2 red Rebel creature token, then attach this \
              to it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Forage",
        kind: Action,
        text: "Exile three cards from your graveyard or sacrifice a Food.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Forecast",
        kind: Ability,
        text: "Activate a forecast ability only during your upkeep and only once each turn, and \
              only by revealing this card from your hand.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Forestcycling",
        kind: Ability,
        text: "Pay the forestcycling cost and discard this card: Search your library for a Forest \
              card, reveal it, put it into your hand, then shuffle.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Forestwalk",
        kind: Ability,
        text: "This creature can't be blocked as long as defending player controls a Forest.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Foretell",
        kind: Ability,
        text: "During your turn, you may pay {2} and exile this card from your hand face down. Cast \
              it on a later turn for its foretell cost.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Formidable",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether creatures you control have \
              total power 8 or greater.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Fortify",
        kind: Ability,
        text: "Pay the fortify cost: Attach this Fortification to target land you control. Fortify \
              only as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Freerunning",
        kind: Ability,
        text: "You may cast this spell for its freerunning cost if you dealt combat damage to a \
              player this turn with an Assassin or commander.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Frenzy",
        kind: Ability,
        text: "Whenever this creature attacks and isn't blocked, it gets +X/+0 until end of turn, \
              where X is its frenzy number.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Friends forever",
        kind: Ability,
        text: "You can have two commanders if both have friends forever.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Fuse",
        kind: Ability,
        text: "You may cast one or both halves of this split card from your hand.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Gift",
        kind: Ability,
        text: "You may promise an opponent a gift as you cast this spell. If you do, they receive \
              the named gift, either before the spell's other effects or when the permanent \
              enters, and the spell gains an additional effect.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Goad",
        kind: Action,
        text: "Until your next turn, that creature attacks each combat if able and attacks a player \
              other than you if able.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Gotcha",
        kind: AbilityWord,
        text: "An ability word from Unhinged marking abilities that let you return the card from \
              your graveyard to your hand by saying \"Gotcha!\" when an opponent says or does the \
              thing the card names.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Graft",
        kind: Ability,
        text: "This creature enters with that many +1/+1 counters on it. Whenever another creature \
              enters, you may move a +1/+1 counter from this creature onto it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Grandeur",
        kind: AbilityWord,
        text: "An ability word marking activated abilities whose cost is discarding another card \
              with the same name as this one.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Gravestorm",
        kind: Ability,
        text: "When you cast this spell, copy it for each permanent put into a graveyard this turn. \
              You may choose new targets for the copies.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Harmonize",
        kind: Ability,
        text: "You may cast this card from your graveyard for its harmonize cost. You may tap a \
              creature you control to reduce that cost by {X}, where X is its power. Then exile \
              this spell.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Harness",
        kind: Action,
        text: "To harness a permanent is to give it the harnessed designation. It stays harnessed \
              until it leaves the battlefield.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Haste",
        kind: Ability,
        text: "This creature can attack and {T} as soon as it comes under your control.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Haunt",
        kind: Ability,
        text: "When this creature dies, or when this spell card is put into a graveyard after \
              resolving, exile it haunting target creature. Its haunt ability triggers again when \
              the haunted creature dies.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Heal",
        kind: Action,
        text: "To heal damage dealt to a permanent is to remove that much damage marked on it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Hellbent",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether you have no cards in hand.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Hero's Reward",
        kind: AbilityWord,
        text: "An ability word from the Theros challenge decks marking the bonus every player \
              receives when that card leaves the battlefield.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Heroic",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever you cast a spell that \
              targets this creature.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Hexproof",
        kind: Ability,
        text: "This permanent can't be the target of spells or abilities your opponents control.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Hexproof from",
        kind: Ability,
        text: "This permanent can't be the target of spells or abilities of the stated quality that \
              your opponents control.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Hidden agenda",
        kind: Ability,
        text: "Start the game with this conspiracy face down in the command zone and secretly \
              choose a card name. You may turn this conspiracy face up any time and reveal that \
              name.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Hideaway",
        kind: Ability,
        text: "When this permanent enters, look at that many cards from the top of your library, \
              exile one face down, then put the rest on the bottom in a random order.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Horsemanship",
        kind: Ability,
        text: "This creature can't be blocked except by creatures with horsemanship.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Impending",
        kind: Ability,
        text: "If you cast this spell for its impending cost, it enters with that many time \
              counters and isn't a creature until the last is removed. At the beginning of your \
              end step, remove a time counter from it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Imprint",
        kind: AbilityWord,
        text: "An ability word marking abilities that exile one or more cards, with the exiled \
              cards then shaping what the permanent does.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Improvise",
        kind: Ability,
        text: "Your artifacts can help cast this spell. Each artifact you tap after you're done \
              activating mana abilities pays for {1}.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Increment",
        kind: Ability,
        text: "Whenever you cast a spell, if the amount of mana you spent is greater than this \
              creature's power or toughness, put a +1/+1 counter on this creature.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Incubate",
        kind: Action,
        text: "Create an Incubator token with that many +1/+1 counters on it. It's a colorless \
              artifact with \"{2}: Transform this artifact,\" and its other face is a 0/0 \
              colorless Phyrexian artifact creature.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Indestructible",
        kind: Ability,
        text: "Damage and effects that say \"destroy\" don't destroy this permanent.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Infect",
        kind: Ability,
        text: "This creature deals damage to creatures in the form of -1/-1 counters and to players \
              in the form of poison counters.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Infusion",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether you gained life this turn.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Ingest",
        kind: Ability,
        text: "Whenever this creature deals combat damage to a player, that player exiles the top \
              card of their library.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Inspired",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever this creature becomes \
              untapped.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Intimidate",
        kind: Ability,
        text: "This creature can't be blocked except by artifact creatures and/or creatures that \
              share a color with it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Investigate",
        kind: Action,
        text: "Create a Clue token. It's a colorless artifact with \"{2}, Sacrifice this artifact: \
              Draw a card.\".",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Islandcycling",
        kind: Ability,
        text: "Pay the islandcycling cost and discard this card: Search your library for an Island \
              card, reveal it, put it into your hand, then shuffle.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Islandwalk",
        kind: Ability,
        text: "This creature can't be blocked as long as defending player controls an Island.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Job select",
        kind: Ability,
        text: "When this Equipment enters, create a 1/1 colorless Hero creature token, then attach \
              this to it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Join forces",
        kind: AbilityWord,
        text: "An ability word marking abilities where each player, starting with you, may pay \
              mana, and the total amount paid determines how large the effect is.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Jump-start",
        kind: Ability,
        text: "You may cast this card from your graveyard by discarding a card in addition to \
              paying its other costs. Then exile this card.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Kicker",
        kind: Ability,
        text: "You may pay an additional kicker cost as you cast this spell.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Kinfall",
        kind: AbilityWord,
        text: "An ability word from a Mystery Booster playtest card marking abilities that trigger \
              when a creature sharing a creature type with it enters the battlefield under your \
              control.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Kinship",
        kind: AbilityWord,
        text: "An ability word marking upkeep abilities that let you look at the top card of your \
              library and reveal it for a bonus if it shares a creature type with this creature.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Landcycling",
        kind: Ability,
        text: "Pay the landcycling cost and discard this card: Search your library for a land card \
              of the named type, reveal it, put it into your hand, then shuffle.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Landfall",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever a land enters the \
              battlefield under your control.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Landship",
        kind: AbilityWord,
        text: "An ability word from a Mystery Booster playtest card marking upkeep abilities that \
              reward you for revealing a land from the top of your library.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Learn",
        kind: Action,
        text: "You may reveal a Lesson card you own from outside the game and put it into your \
              hand, discard a card to draw a card, or do nothing.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Legacy",
        kind: AbilityWord,
        text: "An ability word from Mystery Booster playtest cards marking abilities that \
              permanently mark or write on the physical card, so the change carries from one game \
              to the next.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Legendary landwalk",
        kind: Ability,
        text: "This creature can't be blocked as long as defending player controls a legendary \
              land.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Level up",
        kind: Ability,
        text: "Pay the level up cost: Put a level counter on this creature. Level up only as a \
              sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Lieutenant",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether you control your commander.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Lifelink",
        kind: Ability,
        text: "Damage dealt by this creature also causes you to gain that much life.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Living metal",
        kind: Ability,
        text: "During your turn, this Vehicle is also a creature.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Living weapon",
        kind: Ability,
        text: "When this Equipment enters, create a 0/0 black Phyrexian Germ creature token, then \
              attach this to it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Madness",
        kind: Ability,
        text: "If you discard this card, discard it into exile. When you do, cast it for its \
              madness cost or put it into your graveyard.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Magecraft",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever you cast or copy an instant \
              or sorcery spell.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Manifest",
        kind: Action,
        text: "To manifest a card, put it onto the battlefield face down as a 2/2 creature with no \
              name, types, or abilities. Turn it face up any time for its mana cost if it's a \
              creature card.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Manifest dread",
        kind: Action,
        text: "Look at the top two cards of your library. Put one onto the battlefield face down as \
              a 2/2 creature and the other into your graveyard. Turn the face-down card face up \
              any time for its mana cost if it's a creature card.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Max speed",
        kind: Ability,
        text: "An ability word from Aetherdrift marking abilities that are active only while your \
              speed is 4, the maximum.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Mayhem",
        kind: Ability,
        text: "You may cast this card from your graveyard for its mayhem cost if you discarded it \
              this turn. Timing rules still apply.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Megamorph",
        kind: Ability,
        text: "You may cast this card face down as a 2/2 creature for {3}. Turn it face up any time \
              for its megamorph cost and put a +1/+1 counter on it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Meld",
        kind: Action,
        text: "To meld two specified cards you own, exile them, then put them onto the battlefield \
              combined into one oversized permanent with their melded backs face up. Both cards \
              must be owned by the same player and on the battlefield.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Melee",
        kind: Ability,
        text: "Whenever this creature attacks, it gets +1/+1 until end of turn for each opponent \
              you attacked this combat.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Menace",
        kind: Ability,
        text: "This creature can't be blocked except by two or more creatures.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Mentor",
        kind: Ability,
        text: "Whenever this creature attacks, put a +1/+1 counter on target attacking creature \
              with lesser power.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Metalcraft",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether you control three or more \
              artifacts.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Mill",
        kind: Action,
        text: "To mill a number of cards, put that many cards from the top of your library into \
              your graveyard. Milling isn't drawing, and a player who is told to mill more cards \
              than their library holds simply mills as many as they can.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Miracle",
        kind: Ability,
        text: "You may cast this card for its miracle cost when you draw it if it's the first card \
              you drew this turn.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Mobilize",
        kind: Ability,
        text: "Whenever this creature attacks, create that many tapped and attacking 1/1 red \
              Warrior creature tokens. Sacrifice them at the beginning of the next end step.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Modular",
        kind: Ability,
        text: "This creature enters with that many +1/+1 counters on it. When it dies, you may put \
              its +1/+1 counters on target artifact creature.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Monstrosity",
        kind: Action,
        text: "If this creature isn't monstrous, put that many +1/+1 counters on it and it becomes \
              monstrous. A creature can become monstrous only once.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Morbid",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether a creature died this turn.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "More Than Meets the Eye",
        kind: Ability,
        text: "You may cast this card converted — that is, with its other face up — for its More \
              Than Meets the Eye cost.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Morph",
        kind: Ability,
        text: "You may cast this card face down as a 2/2 creature for {3}. Turn it face up any time \
              for its morph cost.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Mountaincycling",
        kind: Ability,
        text: "Pay the mountaincycling cost and discard this card: Search your library for a \
              Mountain card, reveal it, put it into your hand, then shuffle.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Mountainwalk",
        kind: Ability,
        text: "This creature can't be blocked as long as defending player controls a Mountain.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Multikicker",
        kind: Ability,
        text: "You may pay this spell's multikicker cost any number of times as you cast it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Mutate",
        kind: Ability,
        text: "If you cast this spell for its mutate cost, put it over or under target non-Human \
              creature you own. They mutate into the creature on top plus all abilities from under \
              it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Myriad",
        kind: Ability,
        text: "Whenever this creature attacks, for each opponent other than defending player, you \
              may create a token copy that's tapped and attacking that player or a planeswalker \
              they control. Exile the tokens at end of combat.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Nightbound",
        kind: Ability,
        text: "As long as a player casts at least two spells during their own turn, it becomes day \
              next turn.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Ninjutsu",
        kind: Ability,
        text: "Pay the ninjutsu cost and return an unblocked attacker you control to hand: Put this \
              card onto the battlefield from your hand tapped and attacking.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Nonbasic landwalk",
        kind: Ability,
        text: "This creature can't be blocked as long as defending player controls a nonbasic land.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Offering",
        kind: Ability,
        text: "You may cast this spell as though it had flash by sacrificing a permanent of the \
              stated type and paying the difference in mana costs between this and the sacrificed \
              permanent. Mana cost includes color.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Offspring",
        kind: Ability,
        text: "You may pay an additional offspring cost as you cast this spell. If you do, when \
              this creature enters, create a 1/1 token copy of it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Open an Attraction",
        kind: Action,
        text: "Put the top card of your Attraction deck onto the battlefield. If your Attraction \
              deck is empty, nothing happens.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Opus",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever you cast an instant or \
              sorcery spell, with a bigger effect if five or more mana was spent to cast it.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Outlast",
        kind: Ability,
        text: "Pay the outlast cost and tap this creature: Put a +1/+1 counter on it. Outlast only \
              as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Overload",
        kind: Ability,
        text: "You may cast this spell for its overload cost. If you do, change \"target\" in its \
              text to \"each.\".",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Pack tactics",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether you attacked with creatures \
              with total power 6 or greater.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Paradigm",
        kind: Ability,
        text: "Then exile this spell. After you first resolve a spell with this name, you may cast \
              a copy of it from exile without paying its mana cost at the beginning of each of \
              your first main phases.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Paradox",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever you cast a spell from \
              anywhere other than your hand.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Parley",
        kind: AbilityWord,
        text: "An ability word marking abilities where each player reveals the top card of their \
              library and the effect scales with what was revealed.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Partner",
        kind: Ability,
        text: "You can have two commanders if both have partner.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Partner with",
        kind: Ability,
        text: "When this creature enters, target player may put the named card into their hand from \
              their library, then shuffle. The two named cards can be your commanders together.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Perpetually",
        kind: Action,
        text: "A digital-only modifier: the described change to a card lasts for the rest of the \
              game and continues to apply as that card changes zones.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Persist",
        kind: Ability,
        text: "When this creature dies, if it had no -1/-1 counters on it, return it to the \
              battlefield under its owner's control with a -1/-1 counter on it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Phasing",
        kind: Ability,
        text: "This permanent phases in or out before you untap during each of your untap steps. \
              While it's phased out, it's treated as though it doesn't exist.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Plainscycling",
        kind: Ability,
        text: "Pay the plainscycling cost and discard this card: Search your library for a Plains \
              card, reveal it, put it into your hand, then shuffle.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Plainswalk",
        kind: Ability,
        text: "This creature can't be blocked as long as defending player controls a Plains.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Planeswalk",
        kind: Action,
        text: "In a Planechase game, to planeswalk is to put the face-up plane card on the bottom \
              of its owner's planar deck, then turn the top card of that deck face up.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Play",
        kind: Action,
        text: "To play a card means to cast it as a spell or, if it's a land, to put it onto the \
              battlefield as your land for the turn.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Plot",
        kind: Ability,
        text: "You may pay the plot cost and exile this card from your hand. Cast it as a sorcery \
              on a later turn without paying its mana cost. Plot only as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Poisonous",
        kind: Ability,
        text: "Whenever this creature deals combat damage to a player, that player gets that many \
              poison counters. A player with ten or more poison counters loses the game.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Populate",
        kind: Action,
        text: "Choose a creature token you control, then create a token that's a copy of it. If you \
              control no creature tokens, nothing happens.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Power-up",
        kind: Ability,
        text: "A power-up ability is an activated ability that can be activated only once. Its cost \
              is reduced by this permanent's mana cost if it entered the battlefield this turn.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Proliferate",
        kind: Action,
        text: "Choose any number of permanents and/or players with a counter on them, then give \
              each another counter of each kind already there.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Protection",
        kind: Ability,
        text: "This permanent can't be blocked, targeted, dealt damage, enchanted, or equipped by \
              anything with the stated quality.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Prototype",
        kind: Ability,
        text: "You may cast this spell with a different mana cost, color, and size. It keeps its \
              abilities and types.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Provoke",
        kind: Ability,
        text: "Whenever this creature attacks, you may have target creature defending player \
              controls untap and block it if able.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Prowess",
        kind: Ability,
        text: "Whenever you cast a noncreature spell, this creature gets +1/+1 until end of turn.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Prowl",
        kind: Ability,
        text: "You may cast this spell for its prowl cost if a player was dealt combat damage this \
              turn by a creature of the stated type.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Radiance",
        kind: AbilityWord,
        text: "An ability word marking abilities that affect the targeted permanent plus every \
              other permanent that shares a color with it.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Raid",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether you attacked with a creature \
              this turn.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Rally",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever an Ally enters the \
              battlefield under your control.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Rampage",
        kind: Ability,
        text: "Whenever this creature becomes blocked, it gets that much +1/+1 until end of turn \
              for each creature blocking it beyond the first.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Ravenous",
        kind: Ability,
        text: "This creature enters with X +1/+1 counters on it. If X is 5 or more, draw a card \
              when it enters.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Reach",
        kind: Ability,
        text: "This creature can block creatures with flying.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Read ahead",
        kind: Ability,
        text: "Choose a chapter and start with that many lore counters. Add one after your draw \
              step. Skipped chapters don't trigger. Sacrifice this Saga after its final chapter.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Rebound",
        kind: Ability,
        text: "If you cast this spell from your hand, exile it as it resolves. At the beginning of \
              your next upkeep, you may cast this card from exile without paying its mana cost.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Reconfigure",
        kind: Ability,
        text: "Pay the reconfigure cost: Attach this permanent to target creature you control, or \
              unattach it. Reconfigure only as a sorcery. While attached, this isn't a creature.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Recover",
        kind: Ability,
        text: "When a creature is put into your graveyard from the battlefield, you may pay this \
              card's recover cost. If you do, return it from your graveyard to your hand. \
              Otherwise, exile it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Regenerate",
        kind: Action,
        text: "Regeneration creates a shield: the next time this permanent would be destroyed this \
              turn, instead tap it, remove it from combat, and remove all damage marked on it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Reinforce",
        kind: Ability,
        text: "Pay the reinforce cost and discard this card: Put that many +1/+1 counters on target \
              creature.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Renew",
        kind: AbilityWord,
        text: "An ability word from Tarkir: Dragonstorm marking activated abilities you use from \
              your graveyard by exiling the card, and only as a sorcery.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Renown",
        kind: Ability,
        text: "When this creature deals combat damage to a player, if it isn't renowned, put that \
              many +1/+1 counters on it and it becomes renowned.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Repartee",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger whenever you cast an instant or \
              sorcery spell that targets a creature.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Replicate",
        kind: Ability,
        text: "When you cast this spell, copy it for each time you paid its replicate cost. You may \
              choose new targets for the copies.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Retrace",
        kind: Ability,
        text: "You may cast this card from your graveyard by discarding a land card in addition to \
              paying its other costs.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Reveal",
        kind: Action,
        text: "To reveal a card is to show it to all players long enough for them to see it. Unless \
              an effect moves it, a revealed card stays in the zone it was in.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Revolt",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether a permanent you controlled left \
              the battlefield this turn.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Riot",
        kind: Ability,
        text: "This creature enters with your choice of a +1/+1 counter or haste.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Ripple",
        kind: Ability,
        text: "When you cast this spell, you may reveal that many cards from the top of your \
              library. You may cast any revealed cards with the same name as this spell without \
              paying their mana costs. Put the rest on the bottom of your library.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Roll to visit your Attractions",
        kind: Action,
        text: "Roll a six-sided die. Each Attraction you control whose lights include the number \
              rolled is visited, and its visit ability triggers.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Sacrifice",
        kind: Action,
        text: "To sacrifice a permanent is to move it from the battlefield to its owner's \
              graveyard. You can sacrifice only permanents you control, and sacrificing can't be \
              prevented by indestructible or regeneration.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Saddle",
        kind: Ability,
        text: "Tap any number of other creatures you control with total power equal to or greater \
              than the saddle number: This Mount becomes saddled until end of turn. Saddle only as \
              a sorcery.",
        parameterized: true,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Scavenge",
        kind: Ability,
        text: "Pay the scavenge cost and exile this card from your graveyard: Put a number of +1/+1 \
              counters equal to this card's power on target creature. Scavenge only as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Scry",
        kind: Action,
        text: "Look at that many cards from the top of your library, then put any number of them on \
              the bottom of your library and the rest on top in any order.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Search",
        kind: Action,
        text: "To search a zone is to look at all cards in it — including face-down and hidden \
              cards — for cards matching the given description. If you search a library, shuffle \
              it afterward when instructed, and you may fail to find even if a matching card is \
              there.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Secret council",
        kind: AbilityWord,
        text: "An ability word marking abilities where each player votes secretly and all of the \
              votes are then revealed at once.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Seek",
        kind: Action,
        text: "A digital-only action: the game picks a card at random from your library that \
              matches the given description and puts it into your hand. You don't search your \
              library and it isn't shuffled.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Set in motion",
        kind: Action,
        text: "In an Archenemy game, to set a scheme in motion is to move the top card of your \
              scheme deck off that deck and turn it face up, putting its abilities into effect.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Shadow",
        kind: Ability,
        text: "This creature can block or be blocked by only creatures with shadow.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Shroud",
        kind: Ability,
        text: "This permanent can't be the target of spells or abilities.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Shuffle",
        kind: Action,
        text: "To shuffle a library or a set of cards is to randomize their order so that no player \
              knows the position of any card in it.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Skulk",
        kind: Ability,
        text: "This creature can't be blocked by creatures with greater power.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Slivercycling",
        kind: Ability,
        text: "Pay the slivercycling cost and discard this card: Search your library for a Sliver \
              card, reveal it, put it into your hand, then shuffle.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Sneak",
        kind: Ability,
        text: "You may cast this spell for its sneak cost if you also return an unblocked attacker \
              you control to hand during the declare blockers step. If it's a creature, it enters \
              tapped and attacking.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Solved",
        kind: Ability,
        text: "An ability word on Case enchantments marking the ability that only functions once \
              that Case has been solved.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Soulbond",
        kind: Ability,
        text: "You may pair this creature with another unpaired creature when either enters. They \
              remain paired for as long as you control both of them.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Soulshift",
        kind: Ability,
        text: "When this creature dies, you may return target Spirit card with mana value equal to \
              or less than the soulshift number from your graveyard to your hand.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Space sculptor",
        kind: Ability,
        text: "This creature divides the battlefield into alpha, beta, and gamma sectors. If a \
              creature isn't assigned to a sector, its controller assigns it to one. Opponents \
              assign first.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Specialize",
        kind: Ability,
        text: "Pay the specialize cost and discard a card: this permanent perpetually changes into \
              one of five specialized versions of itself, one for each color. It appears only on \
              digital-only Alchemy cards.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Spectacle",
        kind: Ability,
        text: "You may cast this spell for its spectacle cost rather than its mana cost if an \
              opponent lost life this turn.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Spell mastery",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether there are two or more instant \
              and/or sorcery cards in your graveyard.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Splice onto Arcane",
        kind: Ability,
        text: "As you cast an Arcane spell, you may reveal this card from your hand and pay its \
              splice cost. If you do, add this card's effects to that spell.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Split second",
        kind: Ability,
        text: "As long as this spell is on the stack, players can't cast spells or activate \
              abilities that aren't mana abilities.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Spree",
        kind: Ability,
        text: "Choose one or more additional costs.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Squad",
        kind: Ability,
        text: "As an additional cost to cast this spell, you may pay its squad cost any number of \
              times. When this creature enters, create that many tokens that are copies of it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Start your engines!",
        kind: Ability,
        text: "If you have no speed, it starts at 1. It increases once on each of your turns when \
              an opponent loses life. Max speed is 4.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Station",
        kind: Ability,
        text: "Tap another creature you control: Put charge counters equal to its power on this \
              permanent. Station only as a sorcery. Abilities printed with a STATION number switch \
              on once it has at least that many charge counters.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Storm",
        kind: Ability,
        text: "When you cast this spell, copy it for each spell cast before it this turn. You may \
              choose new targets for the copies.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Strive",
        kind: AbilityWord,
        text: "An ability word marking spells with an additional cost you pay for each target \
              beyond the first.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Sunburst",
        kind: Ability,
        text: "This permanent enters with a +1/+1 counter on it for each color of mana spent to \
              cast it, or a charge counter for each such color if it isn't a creature.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Support",
        kind: Action,
        text: "Put a +1/+1 counter on each of that many other target creatures. Support on an \
              instant or sorcery can target any creatures; on a permanent it targets creatures \
              other than that permanent.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Surge",
        kind: Ability,
        text: "You may cast this spell for its surge cost if you or a teammate has cast another \
              spell this turn.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Surveil",
        kind: Action,
        text: "Look at that many cards from the top of your library, then put any number of them \
              into your graveyard and the rest on top of your library in any order.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Survival",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger at the beginning of your second main \
              phase if this creature is tapped.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Suspect",
        kind: Action,
        text: "A suspected creature has menace and can't block. It stays suspected until an effect \
              says it's no longer suspected.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Suspend",
        kind: Ability,
        text: "Rather than cast this card from your hand, you may pay its suspend cost and exile it \
              with that many time counters on it. At the beginning of your upkeep, remove a time \
              counter. When the last is removed, cast it without paying its mana cost. It has \
              haste.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Swampcycling",
        kind: Ability,
        text: "Pay the swampcycling cost and discard this card: Search your library for a Swamp \
              card, reveal it, put it into your hand, then shuffle.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Swampwalk",
        kind: Ability,
        text: "This creature can't be blocked as long as defending player controls a Swamp.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Sweep",
        kind: AbilityWord,
        text: "An ability word marking spells that return any number of lands of a given type you \
              control to your hand, with the effect scaling to how many you returned.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Take the initiative",
        kind: Action,
        text: "You become the player who has the initiative. When you take it, and at the beginning \
              of your upkeep while you have it, you venture into the Undercity.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Tap",
        kind: Action,
        text: "To tap a permanent is to turn it sideways to show it has been used. A permanent \
              that's already tapped can't be tapped again.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Teamwork",
        kind: Ability,
        text: "As an additional cost to cast this spell, you may tap any number of creatures you \
              control with total power equal to or greater than the teamwork number.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Tempting offer",
        kind: AbilityWord,
        text: "An ability word marking spells that offer every opponent the same effect, and for \
              each opponent who accepts, you get that effect an additional time.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "The Ring tempts you",
        kind: Action,
        text: "You get an emblem named The Ring if you don't have one, then your Ring emblem gains \
              its next ability. Then choose a creature you control as your Ring-bearer, which may \
              be the same one as before.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Threshold",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether seven or more cards are in your \
              graveyard.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Tiered",
        kind: Ability,
        text: "Choose one additional cost.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Time travel",
        kind: Action,
        text: "For each suspended card you own and each permanent you control with a time counter \
              on it, you may add a time counter to it or remove one from it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Toxic",
        kind: Ability,
        text: "Players dealt combat damage by this creature also get that many poison counters. A \
              player with ten or more poison counters loses the game.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Training",
        kind: Ability,
        text: "Whenever this creature attacks with another creature with greater power, put a +1/+1 \
              counter on this creature.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Trample",
        kind: Ability,
        text: "This creature can deal excess combat damage to the player or planeswalker it's \
              attacking.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Transfigure",
        kind: Ability,
        text: "Pay the transfigure cost and sacrifice this creature: Search your library for a \
              creature card with the same mana value as this creature, put that card onto the \
              battlefield, then shuffle. Transfigure only as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Transform",
        kind: Action,
        text: "To transform a double-faced permanent is to turn it over so that its other face is \
              up. Only permanents represented by double-faced cards (or merged permanents) can \
              transform.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Transmute",
        kind: Ability,
        text: "Pay the transmute cost, Discard this card: Search your library for a card with the \
              same mana value as this card, reveal it, put it into your hand, then shuffle. \
              Transmute only as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Tribute",
        kind: Ability,
        text: "As this creature enters, an opponent of your choice may put that many +1/+1 counters \
              on it. Its other abilities check whether tribute was paid.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Triple",
        kind: Action,
        text: "To triple a number is to add twice that amount; tripling a creature's power gives it \
              +X/+0 where X is twice its power.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Typecycling",
        kind: Ability,
        text: "Pay the typecycling cost and discard this card: Search your library for a card of \
              the named type, reveal it, put it into your hand, then shuffle.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Umbra armor",
        kind: Ability,
        text: "If enchanted creature would be destroyed, instead remove all damage from it and \
              destroy this Aura.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Undaunted",
        kind: Ability,
        text: "This spell costs {1} less to cast for each opponent.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Underdog",
        kind: AbilityWord,
        text: "An ability word from a Mystery Booster playtest card marking abilities that give a \
              bonus in later games of a match if you've already lost a game in that match.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Undergrowth",
        kind: AbilityWord,
        text: "An ability word marking abilities whose effect scales with the number of creature \
              cards in your graveyard.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Undying",
        kind: Ability,
        text: "When this creature dies, if it had no +1/+1 counters on it, return it to the \
              battlefield under its owner's control with a +1/+1 counter on it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Unearth",
        kind: Ability,
        text: "Pay the unearth cost: Return this card from your graveyard to the battlefield. It \
              gains haste. Exile it at the beginning of the next end step or if it would leave the \
              battlefield. Unearth only as a sorcery.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Unleash",
        kind: Ability,
        text: "You may have this creature enter with a +1/+1 counter on it. It can't block as long \
              as it has a +1/+1 counter on it.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Untap",
        kind: Action,
        text: "To untap a permanent is to turn it back upright from its tapped position. A \
              permanent that's already untapped can't be untapped.",
        parameterized: false,
        match_mode: Never,
    },
    Entry {
        name: "Valiant",
        kind: AbilityWord,
        text: "An ability word marking abilities that trigger when this creature becomes the target \
              of a spell or ability you control for the first time each turn.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Vanishing",
        kind: Ability,
        text: "This permanent enters with that many time counters on it. At the beginning of your \
              upkeep, remove a time counter from it. When the last is removed, sacrifice it.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Venture into the dungeon",
        kind: Action,
        text: "Enter the first room of a dungeon of your choice, or advance to the next room of the \
              dungeon you're already in, then follow that room's instructions.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Vigilance",
        kind: Ability,
        text: "Attacking doesn't cause this creature to tap.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Visit",
        kind: Ability,
        text: "Whenever you roll to visit your Attractions, if the result is equal to a number that \
              is lit up on this Attraction, the listed effect happens.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Vivid",
        kind: AbilityWord,
        text: "An ability word marking abilities whose effect scales with the number of colors \
              among permanents you control.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Void",
        kind: AbilityWord,
        text: "An ability word marking abilities that check whether a nonland permanent left the \
              battlefield this turn or a spell was warped this turn.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Vote",
        kind: Action,
        text: "Each player votes for one of the listed choices, starting with the player after the \
              controller of the ability and proceeding in turn order. Votes are cast aloud, so \
              later voters know how earlier players voted.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Ward",
        kind: Ability,
        text: "Whenever this permanent becomes the target of a spell or ability an opponent \
              controls, counter it unless that player pays the ward cost.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Warp",
        kind: Ability,
        text: "You may cast this card from your hand for its warp cost. Exile it at the beginning \
              of the next end step, then you may cast it from exile on a later turn.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Waterbend",
        kind: Action,
        text: "While paying a waterbend cost, you can tap your artifacts and creatures to help. \
              Each one pays for {1}.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Web-slinging",
        kind: Ability,
        text: "You may cast this spell for its web-slinging cost if you also return a tapped \
              creature you control to its owner's hand.",
        parameterized: true,
        match_mode: Anywhere,
    },
    Entry {
        name: "Will of the Planeswalkers",
        kind: AbilityWord,
        text: "An ability word on Planechase cards marking abilities where each player votes to \
              planeswalk or for chaos to ensue.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Will of the council",
        kind: AbilityWord,
        text: "An ability word marking abilities where each player votes and the option with the \
              most votes is what happens.",
        parameterized: false,
        match_mode: AbilityLine,
    },
    Entry {
        name: "Wither",
        kind: Ability,
        text: "This deals damage to creatures in the form of -1/-1 counters.",
        parameterized: false,
        match_mode: Anywhere,
    },
    Entry {
        name: "Wizardcycling",
        kind: Ability,
        text: "Pay the wizardcycling cost and discard this card: Search your library for a Wizard \
              card, reveal it, put it into your hand, then shuffle.",
        parameterized: true,
        match_mode: Anywhere,
    },
];

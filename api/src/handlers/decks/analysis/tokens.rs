//! **Tokens this deck makes** — the pieces of cardboard a player has to bring to a game
//! besides the deck itself, worked out from the deck's own cards.
//!
//! It answers a question every other analysis read here answers about the *cards*: what
//! else does sitting down with this list actually require? A Commander deck that makes
//! Treasures, Clues, three sizes of Zombie and an Elspeth emblem needs those in the box, and
//! the only place that list exists today is in a player's memory.
//!
//! **Nothing here is inferred from rules text.** Scryfall publishes, per card, the exact
//! printings it relates to (`all_parts`), and the ingest keeps the token and emblem entries
//! in `cards.token_parts` (see [`crate::scryfall::map::token_parts`]). So this module reads a
//! provider fact, the way legality reads the published legality object and the bracket reads
//! the published Game Changers column — a grammar over "create a 1/1 white Soldier creature
//! token" would be a second, worse copy of a list Scryfall already curates.
//!
//! Four properties of that upstream data drive the shape of this module:
//!
//! * **The relation is oracle-level.** Every printing of a card carries the same tokens, so
//!   which printing a deck happens to hold never changes *what* it makes — no union across
//!   printings is needed, and a 1995 Homelands card knows about its Serf.
//! * **The referenced id is set-specific.** The same Treasure arrives under a dozen ids, one
//!   per set, so the tokens are grouped by the token printing's own `oracle_id` — the id
//!   that *is* shared across sets — and one printing is picked to represent each group.
//! * **A name is not an identity.** Wurmcoil Engine makes two different Wurm tokens with the
//!   same name and the same type line, so grouping by name would merge them and claim the
//!   deck needs one token where it needs two.
//! * **A missing column is not an empty one.** `token_parts` is NULL on a row imported
//!   before the column existed — every row, until the next daily bulk import lands — and
//!   answering "this deck makes no tokens" on that basis would be a confident wrong answer.
//!   Those rows are counted and reported as unchecked instead.
//!
//! What it deliberately does **not** say is *how many* of each token to bring. The catalog
//! carries no such datum: "create a 1/1" and "create X 1/1s" are the same relation upstream,
//! and a number invented here would be read as a count a player could pack against. So the
//! response says which cards make each token and how many copies of those the deck runs, and
//! lets the player do the arithmetic they'd do anyway.
//!
//! Scoped to the **deck proper** (issue #570): a card parked in a maybeboard isn't in the
//! deck, so it doesn't send you looking for a token.

use std::collections::HashMap;

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Serialize;

use crate::entities::card;
use crate::entities::prelude::Card;
use crate::error::AppError;
use crate::handlers::shared::CardResponse;
use crate::scryfall::model::StoredPart;
use crate::state::AppState;

use super::DeckAnalysisInput;

/// Source cards listed per token. The count stays exact — a deck's row count is
/// caller-controlled and this response isn't paginated, so the list is capped the way the
/// bracket caps its counted cards.
const MAX_LISTED_SOURCES: usize = 20;

/// Ids per `WHERE external_id IN (…)` lookup. Deck rows are caller-controlled, so the
/// resolution is chunked rather than built into one unbounded parameter list — the same
/// bound the sealed-product card lookups use.
const RESOLVE_CHUNK: usize = 900;

// ---------- Wire types ----------

/// One card in the deck that makes a token.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckTokenSource {
    /// External (provider) id of the deck's card.
    pub card_id: String,
    pub name: String,
    /// Copies of it in the deck proper, both finishes together.
    pub quantity: i64,
}

/// One token (or emblem) the deck makes, and what makes it.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckToken {
    /// Stable identity of the token across sets — the token printing's `oracle_id` where
    /// the catalog has it, else a name + type-line key. Distinct tokens that share a name
    /// (Wurmcoil Engine's two Wurms) get distinct keys.
    pub key: String,
    pub name: String,
    /// The token's printed type line (`"Token Creature — Soldier"`, `"Emblem — Elspeth"`).
    pub type_line: Option<String>,
    /// A printing of the token, for the artwork and a link to its page: the newest one any
    /// of the deck's cards points at. `None` when no referenced printing is in the catalog
    /// (a digital-only token, or a token set not imported yet) — the name and type line
    /// above still describe it, because they were stored alongside the reference.
    pub card: Option<CardResponse>,
    /// The deck's cards that make it, by name, capped at 20 — `source_count` stays exact.
    pub sources: Vec<DeckTokenSource>,
    /// How many distinct cards in the deck make it.
    pub source_count: i64,
}

/// Everything a deck's list of tokens says.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct DeckTokens {
    /// The tokens, most-made first, then by name.
    pub tokens: Vec<DeckToken>,
    /// Cards in the deck proper whose catalog row hasn't been checked for tokens yet (it
    /// predates the `token_parts` column and is rewritten by the next bulk import). While
    /// this is non-zero the list below is a floor, not the whole answer — which is why it
    /// is on the wire rather than silently folded into "makes none".
    pub unchecked_count: i64,
}

// ---------- The fold ----------

/// One reference from a deck card to a token printing, before the catalog is consulted.
struct TokenRef<'a> {
    part: &'a StoredPart,
    source_id: &'a str,
    source_name: &'a str,
    copies: i64,
}

/// What the deck asks for: every token reference in it, and how many of its cards couldn't
/// be asked. Pure — [`analyse_tokens`] resolves the references against the catalog after.
struct TokenDemand<'a> {
    refs: Vec<TokenRef<'a>>,
    unchecked_count: i64,
}

/// Walk the deck proper, collecting its token references.
///
/// Copies are summed **per card**, not per row: the same printing in two sections is one
/// card that makes one token, running that many copies.
fn collect_demand(input: &DeckAnalysisInput) -> TokenDemand<'_> {
    let mut refs: Vec<TokenRef<'_>> = Vec::new();
    let mut unchecked_count = 0;
    let mut copies_by_card: HashMap<&str, i64> = HashMap::new();

    for entry in input.deck_proper() {
        let id = entry.facts.id.as_str();
        let seen_before = copies_by_card.contains_key(id);
        *copies_by_card.entry(id).or_insert(0) += entry.copies();
        if seen_before {
            continue;
        }
        match &entry.facts.token_parts {
            // NULL, i.e. a row the catalog hasn't rewritten since the column arrived.
            None => unchecked_count += 1,
            Some(parts) => refs.extend(parts.iter().map(|part| TokenRef {
                part,
                source_id: id,
                source_name: entry.facts.name.as_str(),
                copies: 0,
            })),
        }
    }

    // Copies are only known once every row has been walked (a card can sit in two sections).
    for reference in &mut refs {
        reference.copies = copies_by_card
            .get(reference.source_id)
            .copied()
            .unwrap_or_default();
    }

    TokenDemand {
        refs,
        unchecked_count,
    }
}

/// A token being assembled: the printings referenced for it, and the deck cards asking.
#[derive(Default)]
struct TokenGroup<'a> {
    /// Fallback name/type, from the reference itself — used when no printing resolved.
    stored: Option<&'a StoredPart>,
    /// Resolved printings referenced for this token, in resolution order.
    printings: Vec<card::Model>,
    /// `card_id -> (name, copies)`, so a card referencing two printings of one token
    /// counts once.
    sources: HashMap<&'a str, (&'a str, i64)>,
}

impl<'a> TokenGroup<'a> {
    fn add(&mut self, reference: &TokenRef<'a>, printing: Option<&card::Model>) {
        if self.stored.is_none() {
            self.stored = Some(reference.part);
        }
        if let Some(model) = printing
            && !self.printings.iter().any(|p| p.id == model.id)
        {
            self.printings.push(model.clone());
        }
        self.sources.insert(
            reference.source_id,
            (reference.source_name, reference.copies),
        );
    }

    /// The printing that represents the token: the **newest** referenced one, tie-broken by
    /// provider id so the choice is stable run to run. Newest because a token's recent
    /// printing is the one a player is most likely to have and the one with the best
    /// artwork; which specific printing is shown carries no gameplay meaning either way.
    fn representative(&self) -> Option<&card::Model> {
        self.printings
            .iter()
            .max_by(|a, b| {
                a.released_at
                    .cmp(&b.released_at)
                    .then_with(|| b.external_id.cmp(&a.external_id))
            })
            .or_else(|| self.printings.first())
    }
}

/// Identity of a token when its printing isn't in the catalog: name + type line, which is
/// all the reference itself carries. Only ever used as a *fallback* — a resolved printing is
/// keyed by its `oracle_id`, which is what keeps Wurmcoil Engine's two Wurms apart.
fn name_key(name: &str, type_line: Option<&str>) -> String {
    format!(
        "name:{}|{}",
        name.to_lowercase(),
        type_line.unwrap_or_default().to_lowercase()
    )
}

/// The key a resolved printing groups under: its gameplay identity, shared by every set's
/// printing of that token, falling back to the name key for a row carrying no `oracle_id`.
fn resolved_key(model: &card::Model) -> String {
    model
        .oracle_id
        .clone()
        .unwrap_or_else(|| name_key(&model.name, model.type_line.as_deref()))
}

/// Group the deck's references into tokens, given the printings that resolved.
///
/// **Two passes, deliberately.** Resolved references are grouped first, by gameplay identity;
/// only then are unresolved ones placed, joining a resolved group when exactly one has the
/// same name and type line and standing alone otherwise. Doing it in one pass would make the
/// answer depend on deck order — the same token could split into two entries purely because
/// the card holding the unresolvable reference sorted first — and merging into one of two
/// same-named groups would be a guess rather than a fallback.
fn group_tokens<'a>(
    refs: &'a [TokenRef<'a>],
    resolved: &HashMap<String, card::Model>,
) -> Vec<(String, TokenGroup<'a>)> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, TokenGroup<'a>> = HashMap::new();
    // name key -> the resolved group keys carrying that name, for the second pass.
    let mut by_name: HashMap<String, Vec<String>> = HashMap::new();

    for reference in refs {
        let Some(model) = resolved.get(reference.part.id.as_str()) else {
            continue;
        };
        let key = resolved_key(model);
        if !groups.contains_key(&key) {
            order.push(key.clone());
            by_name
                .entry(name_key(&model.name, model.type_line.as_deref()))
                .or_default()
                .push(key.clone());
        }
        groups.entry(key).or_default().add(reference, Some(model));
    }

    for reference in refs {
        if resolved.contains_key(reference.part.id.as_str()) {
            continue;
        }
        let name = name_key(&reference.part.name, reference.part.type_line.as_deref());
        let key = match by_name.get(&name).map(Vec::as_slice) {
            // Exactly one token in this deck goes by that name and type: the unresolved
            // reference is that token, arriving under an id whose printing we don't hold.
            Some([only]) => only.clone(),
            _ => name,
        };
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().add(reference, None);
    }

    order
        .into_iter()
        .filter_map(|key| groups.remove_entry(&key))
        .collect()
}

/// Shape the grouped tokens into the wire response.
fn build(demand: TokenDemand<'_>, resolved: &HashMap<String, card::Model>) -> DeckTokens {
    let mut tokens: Vec<DeckToken> = group_tokens(&demand.refs, resolved)
        .into_iter()
        .map(|(key, group)| {
            let representative = group.representative().cloned();
            let (name, type_line) = match (&representative, group.stored) {
                (Some(model), _) => (model.name.clone(), model.type_line.clone()),
                (None, Some(part)) => (part.name.clone(), part.type_line.clone()),
                // Unreachable: a group exists only because a reference created it.
                (None, None) => (String::new(), None),
            };

            let source_count = group.sources.len() as i64;
            let mut sources: Vec<DeckTokenSource> = group
                .sources
                .into_iter()
                .map(|(card_id, (name, quantity))| DeckTokenSource {
                    card_id: card_id.to_string(),
                    name: name.to_string(),
                    quantity,
                })
                .collect();
            sources.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.card_id.cmp(&b.card_id)));
            sources.truncate(MAX_LISTED_SOURCES);

            DeckToken {
                key,
                name,
                type_line,
                card: representative.map(CardResponse::from),
                sources,
                source_count,
            }
        })
        .collect();

    // Most-made first — the token a dozen cards want is the one to make sure is in the box —
    // then by name, then by key so the order is total and stable.
    tokens.sort_by(|a, b| {
        b.source_count
            .cmp(&a.source_count)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.key.cmp(&b.key))
    });

    DeckTokens {
        tokens,
        unchecked_count: demand.unchecked_count,
    }
}

// ---------- Entry point ----------

/// The tokens and emblems a deck makes.
///
/// The one analysis read that goes back to the database: a token reference is a *printing*
/// id, and the artwork, set and price behind it live on that row. The lookup is one chunked
/// `IN` over `(game, external_id)` — the index `idx_cards_game_external_id` covers it — and
/// is skipped entirely for a deck that references nothing.
pub(crate) async fn analyse_tokens(
    state: &AppState,
    game: &str,
    input: &DeckAnalysisInput,
) -> Result<DeckTokens, AppError> {
    let demand = collect_demand(input);

    let mut wanted: Vec<&str> = demand.refs.iter().map(|r| r.part.id.as_str()).collect();
    wanted.sort_unstable();
    wanted.dedup();

    let mut resolved: HashMap<String, card::Model> = HashMap::new();
    for chunk in wanted.chunks(RESOLVE_CHUNK) {
        let rows = Card::find()
            .filter(card::Column::Game.eq(game))
            .filter(card::Column::ExternalId.is_in(chunk.iter().copied()))
            .all(&state.db)
            .await?;
        for row in rows {
            resolved.insert(row.external_id.clone(), row);
        }
    }

    Ok(build(demand, &resolved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::decks::analysis::test_fixtures::{deck, entry, section};
    use crate::test_support::card_model;

    /// A catalog row for a token printing.
    fn token_row(
        id: i32,
        external_id: &str,
        name: &str,
        oracle: &str,
        released: &str,
    ) -> card::Model {
        card::Model {
            external_id: external_id.to_string(),
            oracle_id: Some(oracle.to_string()),
            name: name.to_string(),
            type_line: Some(format!("Token Creature — {name}")),
            layout: Some("token".to_string()),
            released_at: Some(released.to_string()),
            ..card_model(id)
        }
    }

    fn resolve(rows: &[card::Model], ids: &[&str]) -> HashMap<String, card::Model> {
        ids.iter()
            .filter_map(|id| {
                rows.iter()
                    .find(|r| r.external_id == *id)
                    .map(|r| ((*id).to_string(), r.clone()))
            })
            .collect()
    }

    fn sections() -> Vec<crate::handlers::decks::DeckSectionResponse> {
        vec![section(1, "Deck", false), section(2, "Ideas", true)]
    }

    /// The same token printed in two sets arrives under two ids; both are the one token, and
    /// the newest printing represents it.
    #[test]
    fn one_token_across_two_sets_is_one_entry() {
        let rows = vec![
            token_row(1, "tok-c19", "Treasure", "oracle-treasure", "2019-08-23"),
            token_row(2, "tok-2x2", "Treasure", "oracle-treasure", "2022-07-08"),
        ];
        let input = deck(
            sections(),
            vec![
                entry("dockside", "Dockside Extortionist", 1, 1, 0).tokens(&[(
                    "tok-c19",
                    "Treasure",
                    "Token Artifact — Treasure",
                )]),
                entry("goldspan", "Goldspan Dragon", 1, 1, 0).tokens(&[(
                    "tok-2x2",
                    "Treasure",
                    "Token Artifact — Treasure",
                )]),
            ],
        );

        let demand = collect_demand(&input);
        let result = build(demand, &resolve(&rows, &["tok-c19", "tok-2x2"]));

        assert_eq!(result.tokens.len(), 1);
        let treasure = &result.tokens[0];
        assert_eq!(treasure.key, "oracle-treasure");
        assert_eq!(treasure.source_count, 2);
        assert_eq!(
            treasure.card.as_ref().map(|c| c.id.as_str()),
            Some("tok-2x2"),
            "the newest referenced printing represents the token"
        );
        assert_eq!(
            treasure
                .sources
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Dockside Extortionist", "Goldspan Dragon"]
        );
    }

    /// Wurmcoil Engine's two Wurms share a name, a type line and a source card. They are two
    /// tokens, and a player who brings one of them is a token short.
    #[test]
    fn same_named_tokens_with_different_identities_stay_apart() {
        let rows = vec![
            token_row(
                1,
                "tok-wurm-a",
                "Wurm",
                "oracle-wurm-deathtouch",
                "2010-10-01",
            ),
            token_row(
                2,
                "tok-wurm-b",
                "Wurm",
                "oracle-wurm-lifelink",
                "2010-10-01",
            ),
        ];
        let input = deck(
            sections(),
            vec![entry("wurmcoil", "Wurmcoil Engine", 1, 1, 0).tokens(&[
                ("tok-wurm-a", "Wurm", "Token Artifact Creature — Wurm"),
                ("tok-wurm-b", "Wurm", "Token Artifact Creature — Wurm"),
            ])],
        );

        let demand = collect_demand(&input);
        let result = build(demand, &resolve(&rows, &["tok-wurm-a", "tok-wurm-b"]));
        assert_eq!(result.tokens.len(), 2);
        assert!(result.tokens.iter().all(|t| t.source_count == 1));
    }

    /// A maybeboard card is under consideration, not in the deck — every "what is this deck"
    /// reader skips it (issue #570), and so does the box you pack.
    #[test]
    fn a_maybeboard_card_sends_you_looking_for_nothing() {
        let rows = vec![token_row(
            1,
            "tok-soldier",
            "Soldier",
            "oracle-soldier",
            "2020-01-01",
        )];
        let input = deck(
            sections(),
            vec![entry("elspeth", "Elspeth", 2, 1, 0).tokens(&[(
                "tok-soldier",
                "Soldier",
                "Token Creature — Soldier",
            )])],
        );

        let demand = collect_demand(&input);
        let result = build(demand, &resolve(&rows, &["tok-soldier"]));
        assert!(result.tokens.is_empty());
        assert_eq!(result.unchecked_count, 0);
    }

    /// A row the catalog hasn't rewritten since the column arrived is *unchecked*, not
    /// tokenless — the difference between "we don't know yet" and a wrong answer.
    #[test]
    fn unchecked_rows_are_counted_not_assumed_empty() {
        let input = deck(
            sections(),
            vec![
                entry("old", "Never Reimported", 1, 1, 0),
                entry("new", "Plain Card", 1, 1, 0).no_tokens(),
            ],
        );
        let result = build(collect_demand(&input), &HashMap::new());
        assert!(result.tokens.is_empty());
        assert_eq!(result.unchecked_count, 1);
    }

    /// Copies are per card, not per row: one printing in two sections is one card making one
    /// token, and its quantity is the deck's whole count of it.
    #[test]
    fn copies_sum_across_sections_and_the_card_counts_once() {
        let rows = vec![token_row(
            1,
            "tok-goblin",
            "Goblin",
            "oracle-goblin",
            "2024-11-15",
        )];
        let mut input = deck(
            vec![section(1, "Deck", false), section(3, "Sideboard", false)],
            vec![
                entry("krenko", "Krenko, Mob Boss", 1, 1, 1).tokens(&[(
                    "tok-goblin",
                    "Goblin",
                    "Token Creature — Goblin",
                )]),
                entry("krenko", "Krenko, Mob Boss", 3, 2, 0).tokens(&[(
                    "tok-goblin",
                    "Goblin",
                    "Token Creature — Goblin",
                )]),
            ],
        );
        input.sections[1].position = 3;

        let result = build(collect_demand(&input), &resolve(&rows, &["tok-goblin"]));
        assert_eq!(result.tokens.len(), 1);
        assert_eq!(result.tokens[0].source_count, 1);
        assert_eq!(result.tokens[0].sources[0].quantity, 4);
    }

    /// A token whose printing isn't in the catalog still tells the player what to bring,
    /// out of the name and type line stored beside the reference.
    #[test]
    fn an_unresolved_token_keeps_its_stored_name() {
        let input = deck(
            sections(),
            vec![entry("maker", "Token Maker", 1, 1, 0).tokens(&[(
                "tok-missing",
                "Food",
                "Token Artifact — Food",
            )])],
        );
        let result = build(collect_demand(&input), &HashMap::new());
        assert_eq!(result.tokens.len(), 1);
        assert_eq!(result.tokens[0].name, "Food");
        assert_eq!(
            result.tokens[0].type_line.as_deref(),
            Some("Token Artifact — Food")
        );
        assert!(result.tokens[0].card.is_none());
    }

    /// An unresolvable reference to a token the deck *also* makes through a printing we do
    /// hold is the same token — and it must land in that group whichever card sorted first.
    #[test]
    fn an_unresolved_reference_joins_its_resolved_token() {
        let rows = vec![token_row(
            1,
            "tok-known",
            "Treasure",
            "oracle-treasure",
            "2022-07-08",
        )];
        // The card holding the unresolvable reference is walked FIRST, which is exactly the
        // order a single-pass grouping would get wrong.
        let input = deck(
            sections(),
            vec![
                entry("a-card", "Awkward Card", 1, 1, 0).tokens(&[(
                    "tok-unknown",
                    "Treasure",
                    "Token Creature — Treasure",
                )]),
                entry("z-card", "Zealous Card", 1, 1, 0).tokens(&[(
                    "tok-known",
                    "Treasure",
                    "Token Creature — Treasure",
                )]),
            ],
        );

        let result = build(collect_demand(&input), &resolve(&rows, &["tok-known"]));
        assert_eq!(result.tokens.len(), 1, "one Treasure, not two");
        assert_eq!(result.tokens[0].source_count, 2);
        assert!(result.tokens[0].card.is_some());
    }

    /// Two same-named tokens make the merge above ambiguous, and a guess would attribute a
    /// token to the wrong one — so the unresolved reference stands on its own instead.
    #[test]
    fn an_ambiguous_unresolved_reference_is_not_guessed_into_a_group() {
        let rows = vec![
            token_row(1, "tok-wurm-a", "Wurm", "oracle-wurm-a", "2010-10-01"),
            token_row(2, "tok-wurm-b", "Wurm", "oracle-wurm-b", "2010-10-01"),
        ];
        let input = deck(
            sections(),
            vec![
                entry("wurmcoil", "Wurmcoil Engine", 1, 1, 0).tokens(&[
                    ("tok-wurm-a", "Wurm", "Token Creature — Wurm"),
                    ("tok-wurm-b", "Wurm", "Token Creature — Wurm"),
                ]),
                entry("other", "Other Wurm Maker", 1, 1, 0).tokens(&[(
                    "tok-wurm-c",
                    "Wurm",
                    "Token Creature — Wurm",
                )]),
            ],
        );

        let result = build(
            collect_demand(&input),
            &resolve(&rows, &["tok-wurm-a", "tok-wurm-b"]),
        );
        assert_eq!(result.tokens.len(), 3);
    }

    /// The most-wanted token leads the list, which is the one worth checking is in the box.
    #[test]
    fn tokens_are_ordered_by_how_many_cards_make_them() {
        let rows = vec![
            token_row(
                1,
                "tok-treasure",
                "Treasure",
                "oracle-treasure",
                "2022-07-08",
            ),
            token_row(2, "tok-angel", "Angel", "oracle-angel", "2021-01-01"),
        ];
        let input = deck(
            sections(),
            vec![
                entry("one", "One", 1, 1, 0).tokens(&[(
                    "tok-angel",
                    "Angel",
                    "Token Creature — Angel",
                )]),
                entry("two", "Two", 1, 1, 0).tokens(&[(
                    "tok-treasure",
                    "Treasure",
                    "Token Artifact — Treasure",
                )]),
                entry("three", "Three", 1, 1, 0).tokens(&[(
                    "tok-treasure",
                    "Treasure",
                    "Token Artifact — Treasure",
                )]),
            ],
        );

        let result = build(
            collect_demand(&input),
            &resolve(&rows, &["tok-treasure", "tok-angel"]),
        );
        assert_eq!(
            result
                .tokens
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Treasure", "Angel"]
        );
    }

    /// The listed sources are capped; the count they summarise is not.
    #[test]
    fn the_source_list_is_capped_but_the_count_is_exact() {
        let rows = vec![token_row(1, "tok", "Zombie", "oracle-zombie", "2020-01-01")];
        let entries = (0..MAX_LISTED_SOURCES + 5)
            .map(|n| {
                entry(&format!("card-{n:03}"), &format!("Maker {n:03}"), 1, 1, 0).tokens(&[(
                    "tok",
                    "Zombie",
                    "Token Creature — Zombie",
                )])
            })
            .collect();
        let input = deck(sections(), entries);

        let result = build(collect_demand(&input), &resolve(&rows, &["tok"]));
        assert_eq!(result.tokens[0].sources.len(), MAX_LISTED_SOURCES);
        assert_eq!(
            result.tokens[0].source_count,
            (MAX_LISTED_SOURCES + 5) as i64
        );
    }
}

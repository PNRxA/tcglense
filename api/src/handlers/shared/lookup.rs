//! Shared game/set/card resolution helpers: the small DB lookups (and the pure
//! set-group resolution) that both the catalog and collection handlers use to turn
//! a `game`/`code`/`id` path segment into a validated entity (or a 404).

use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::catalog::{self, Game};
use crate::entities::prelude::{Card, CardSet};
use crate::entities::{card, card_set};
use crate::error::AppError;
use crate::state::AppState;

/// Resolve a game slug to its static metadata, 404 if unknown.
pub(crate) fn require_game(game: &str) -> Result<&'static Game, AppError> {
    catalog::find(game).ok_or_else(|| AppError::NotFound(format!("unknown game '{game}'")))
}

/// Load a set by its (case-insensitive) code within a game, 404 if unknown.
pub(crate) async fn load_set(
    state: &AppState,
    game: &str,
    code: &str,
) -> Result<card_set::Model, AppError> {
    let code = code.to_lowercase();
    CardSet::find()
        .filter(card_set::Column::Game.eq(game))
        .filter(card_set::Column::Code.eq(code.as_str()))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("set '{code}' not found")))
}

/// Resolve a card by its external (provider) id within a game, 404 if unknown.
pub(crate) async fn load_card(
    state: &AppState,
    game: &str,
    id: &str,
) -> Result<card::Model, AppError> {
    Card::find()
        .filter(card::Column::Game.eq(game))
        .filter(card::Column::ExternalId.eq(id))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("card '{id}' not found")))
}

/// Resolve every set code in `code`'s group: its top-level root plus all
/// descendants. Mirrors the frontend `groupSets` resolution (walk `parent_set_code`
/// up to a root, guarding missing parents and — defensively — cycles) so the
/// "include related sets" view spans exactly the sets nested under one main set.
/// Falls back to `[code]` if the set somehow isn't in the list.
pub(crate) fn group_set_codes(all_sets: &[card_set::Model], code: &str) -> Vec<String> {
    use std::collections::{HashMap, HashSet};
    let by_code: HashMap<&str, &card_set::Model> =
        all_sets.iter().map(|s| (s.code.as_str(), s)).collect();

    let root_of = |start: &str| -> String {
        let mut current = start;
        let mut seen = HashSet::new();
        while let Some(set) = by_code.get(current) {
            let Some(parent) = set.parent_set_code.as_deref() else {
                break;
            };
            // Stop at an orphan (parent not in the catalogue) or a cycle.
            if !by_code.contains_key(parent) || !seen.insert(current) {
                break;
            }
            current = parent;
        }
        current.to_string()
    };

    let root = root_of(code);
    let codes: Vec<String> = all_sets
        .iter()
        .filter(|s| root_of(&s.code) == root)
        .map(|s| s.code.clone())
        .collect();
    if codes.is_empty() {
        vec![code.to_string()]
    } else {
        codes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::prelude::DateTimeUtc;

    fn test_set(code: &str, parent: Option<&str>) -> card_set::Model {
        let ts: DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
        card_set::Model {
            id: 0,
            game: "mtg".into(),
            code: code.into(),
            name: code.to_uppercase(),
            set_type: None,
            released_at: None,
            card_count: 0,
            digital: false,
            icon_svg_uri: None,
            parent_set_code: parent.map(str::to_string),
            external_id: None,
            created_at: ts,
            updated_at: ts,
        }
    }

    fn sorted(mut codes: Vec<String>) -> Vec<String> {
        codes.sort();
        codes
    }

    #[test]
    fn group_codes_standalone_set_is_alone() {
        let sets = vec![test_set("a", None), test_set("b", None)];
        assert_eq!(group_set_codes(&sets, "a"), vec!["a".to_string()]);
    }

    #[test]
    fn group_codes_span_root_and_descendants_from_any_member() {
        // tblc -> blc -> blb: a two-level chain flattened into one group.
        let sets = vec![
            test_set("blb", None),
            test_set("blc", Some("blb")),
            test_set("tblc", Some("blc")),
            test_set("other", None),
        ];
        let expected = vec!["blb".to_string(), "blc".to_string(), "tblc".to_string()];
        // Asking from the root, a middle set, or a leaf all yield the same group.
        assert_eq!(sorted(group_set_codes(&sets, "blb")), expected);
        assert_eq!(sorted(group_set_codes(&sets, "blc")), expected);
        assert_eq!(sorted(group_set_codes(&sets, "tblc")), expected);
    }

    #[test]
    fn group_codes_span_all_siblings_from_one_child() {
        // The common MTG shape: a root with two direct children (e.g. a Commander
        // deck + a token set). Querying any member returns the whole group, and an
        // unrelated multi-member group is excluded.
        let sets = vec![
            test_set("blb", None),
            test_set("blc", Some("blb")),
            test_set("tblb", Some("blb")),
            test_set("dft", None),
            test_set("tdft", Some("dft")),
        ];
        let blb_group = vec!["blb".to_string(), "blc".to_string(), "tblb".to_string()];
        assert_eq!(sorted(group_set_codes(&sets, "tblb")), blb_group);
        assert_eq!(sorted(group_set_codes(&sets, "blc")), blb_group);
        assert_eq!(sorted(group_set_codes(&sets, "blb")), blb_group);
        assert_eq!(
            sorted(group_set_codes(&sets, "dft")),
            vec!["dft".to_string(), "tdft".to_string()],
        );
    }

    #[test]
    fn group_codes_orphan_parent_is_its_own_group() {
        // Parent 'past' isn't in the catalogue, so 'pmic' is its own root.
        let sets = vec![test_set("pmic", Some("past"))];
        assert_eq!(group_set_codes(&sets, "pmic"), vec!["pmic".to_string()]);
    }

    #[test]
    fn group_codes_unknown_set_falls_back_to_itself() {
        let sets = vec![test_set("a", None)];
        assert_eq!(group_set_codes(&sets, "zzz"), vec!["zzz".to_string()]);
    }

    #[test]
    fn group_codes_survive_a_cyclic_reference() {
        // Degenerate data: a <-> b. Each resolves to itself rather than hanging.
        let sets = vec![test_set("a", Some("b")), test_set("b", Some("a"))];
        assert_eq!(group_set_codes(&sets, "a"), vec!["a".to_string()]);
        assert_eq!(group_set_codes(&sets, "b"), vec!["b".to_string()]);
    }
}

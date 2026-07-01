//! `is:` / `not:` predicates over layout, colour, mana, and card type.

use sea_orm::Condition;

use super::common::{raw, raw_vals};
use super::super::error::SearchError;

pub(super) fn is_predicate(value: &str, negated: bool) -> Result<Condition, SearchError> {
    let v = value.to_lowercase();
    let positive: Condition = match v.as_str() {
        "split" | "flip" | "transform" | "meld" | "saga" | "leveler" | "adventure" | "emblem"
        | "class" | "case" | "battle" | "planar" | "scheme" | "vanguard" | "mutate"
        | "prototype" | "augment" | "host" | "normal" => {
            raw_vals("IFNULL(layout, '') = ?".to_string(), [v.clone()])
        }
        "mdfc" | "modaldfc" | "modal_dfc" => raw("IFNULL(layout, '') = 'modal_dfc'"),
        "dfc" | "doublefaced" | "double_faced" => raw(
            "IFNULL(layout, '') IN ('transform', 'modal_dfc', 'meld', 'reversible_card')",
        ),
        "token" => raw("IFNULL(layout, '') IN ('token', 'double_faced_token')"),
        "colorless" => raw("colors IS NULL"),
        "multicolored" | "multicolor" => raw("colors IS NOT NULL AND colors LIKE '%,%'"),
        "monocolored" | "monocolor" => raw("colors IS NOT NULL AND colors NOT LIKE '%,%'"),
        "phyrexian" => raw("IFNULL(mana_cost, '') LIKE '%/P}%'"),
        "hybrid" => raw(
            "IFNULL(mana_cost, '') LIKE '%/%' AND IFNULL(mana_cost, '') NOT LIKE '%/P}%'",
        ),
        "digital" => raw("1 = 0"),
        // Card-type-derived predicates. type_line is title-case from Scryfall but
        // SQLite LIKE folds ASCII case, so lower-case patterns match. Each arm is
        // total (0/1, NULL-safe) so `not:` negation stays exact.
        "permanent" => raw(
            "type_line IS NOT NULL \
             AND (type_line LIKE '%artifact%' OR type_line LIKE '%creature%' \
                  OR type_line LIKE '%enchantment%' OR type_line LIKE '%land%' \
                  OR type_line LIKE '%planeswalker%' OR type_line LIKE '%battle%') \
             AND type_line NOT LIKE '%instant%' AND type_line NOT LIKE '%sorcery%'",
        ),
        // "Spell" is decided by the FRONT face you cast: a card's stored type_line
        // joins faces as "front // back", so test only the part before " // " for
        // land-ness — otherwise spell//land modal DFCs (Kazandu Mammoth and the rest
        // of the Zendikar Rising cycle) would be wrongly excluded by their land back.
        "spell" => raw(
            "type_line IS NOT NULL \
             AND (CASE WHEN INSTR(type_line, ' // ') > 0 \
                       THEN SUBSTR(type_line, 1, INSTR(type_line, ' // ') - 1) \
                       ELSE type_line END) NOT LIKE '%land%' \
             AND IFNULL(layout, '') NOT IN \
                 ('token', 'double_faced_token', 'emblem', 'art_series')",
        ),
        "vanilla" => raw(
            "type_line IS NOT NULL AND type_line LIKE '%creature%' \
             AND (oracle_text IS NULL OR oracle_text = '')",
        ),
        _ => {
            let prefix = if negated { "not" } else { "is" };
            return Err(SearchError::UnsupportedKey(format!("{prefix}:{v}")));
        }
    };
    Ok(if negated { positive.not() } else { positive })
}

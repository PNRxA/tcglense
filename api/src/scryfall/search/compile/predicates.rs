//! `is:` / `not:` predicates over layout, colour, mana, and card type.

use sea_orm::Condition;

use super::super::error::SearchError;
use super::common::{array_member, bool_true, col_present, cond_one, raw, raw_vals};
use crate::db::Dialect;

pub(super) fn is_predicate(
    value: &str,
    negated: bool,
    dialect: Dialect,
) -> Result<Condition, SearchError> {
    let v = value.to_lowercase();
    let positive: Condition = match v.as_str() {
        "split" | "flip" | "transform" | "meld" | "saga" | "leveler" | "adventure" | "emblem"
        | "class" | "case" | "battle" | "planar" | "scheme" | "vanguard" | "mutate"
        | "prototype" | "augment" | "host" | "normal" => {
            raw_vals(dialect, "COALESCE(layout, '') = ?".to_string(), [v.clone()])
        }
        "mdfc" | "modaldfc" | "modal_dfc" => raw("COALESCE(layout, '') = 'modal_dfc'"),
        "dfc" | "doublefaced" | "double_faced" => {
            raw("COALESCE(layout, '') IN ('transform', 'modal_dfc', 'meld', 'reversible_card')")
        }
        "token" => raw("COALESCE(layout, '') IN ('token', 'double_faced_token')"),
        "colorless" => raw("colors IS NULL"),
        "multicolored" | "multicolor" => raw("colors IS NOT NULL AND colors LIKE '%,%'"),
        "monocolored" | "monocolor" => raw("colors IS NOT NULL AND colors NOT LIKE '%,%'"),
        "phyrexian" => raw("COALESCE(mana_cost, '') LIKE '%/P}%'"),
        "hybrid" => {
            raw("COALESCE(mana_cost, '') LIKE '%/%' AND COALESCE(mana_cost, '') NOT LIKE '%/P}%'")
        }
        "digital" => raw("1 = 0"),
        // Card-type-derived predicates. type_line is title-case from Scryfall, so the
        // column is LOWER-ed to match the lower-case patterns on Postgres too (SQLite
        // LIKE folds ASCII, so the result set is unchanged; type lines are pure ASCII).
        // Each arm is total (0/1, NULL-safe) so `not:` negation stays exact.
        "permanent" => raw("type_line IS NOT NULL \
             AND (LOWER(type_line) LIKE '%artifact%' OR LOWER(type_line) LIKE '%creature%' \
                  OR LOWER(type_line) LIKE '%enchantment%' OR LOWER(type_line) LIKE '%land%' \
                  OR LOWER(type_line) LIKE '%planeswalker%' OR LOWER(type_line) LIKE '%battle%') \
             AND LOWER(type_line) NOT LIKE '%instant%' AND LOWER(type_line) NOT LIKE '%sorcery%'"),
        // "Spell" is decided by the FRONT face you cast: a card's stored type_line
        // joins faces as "front // back", so test only the part before " // " for
        // land-ness — otherwise spell//land modal DFCs (Kazandu Mammoth and the rest
        // of the Zendikar Rising cycle) would be wrongly excluded by their land back.
        // `INSTR`↔`STRPOS` per backend; the front face is LOWER-ed for the land test.
        "spell" => {
            let pos = dialect.strpos("type_line", "' // '");
            raw(format!(
                "type_line IS NOT NULL \
                 AND LOWER(CASE WHEN {pos} > 0 THEN SUBSTR(type_line, 1, {pos} - 1) \
                           ELSE type_line END) NOT LIKE '%land%' \
                 AND COALESCE(layout, '') NOT IN \
                     ('token', 'double_faced_token', 'emblem', 'art_series')"
            ))
        }
        "vanilla" => raw(
            "type_line IS NOT NULL AND LOWER(type_line) LIKE '%creature%' \
             AND (oracle_text IS NULL OR oracle_text = '')",
        ),
        // Finish availability (from the finishes array).
        "foil" => cond_one(array_member(dialect, "finishes", "foil")),
        "nonfoil" => cond_one(array_member(dialect, "finishes", "nonfoil")),
        "etched" => cond_one(array_member(dialect, "finishes", "etched")),
        // Print-detail boolean flags.
        "fullart" => bool_true("full_art"),
        "textless" => bool_true("textless"),
        "oversized" => bool_true("oversized"),
        "promo" => bool_true("promo"),
        "reprint" => bool_true("reprint"),
        "variation" => bool_true("variation"),
        "booster" => bool_true("booster"),
        "spotlight" | "storyspotlight" => bool_true("story_spotlight"),
        "contentwarning" => bool_true("content_warning"),
        "hires" | "highres" => bool_true("highres_image"),
        "reserved" => bool_true("reserved"),
        "gamechanger" => bool_true("game_changer"),
        // Presence of an optional print attribute.
        "watermark" => col_present("watermark"),
        "indicator" | "colorindicator" => col_present("color_indicator"),
        // Promo / product-origin categories (from promo_types).
        "buyabox" | "prerelease" | "promopack" | "gameday" | "intropack" | "giftbox" | "bundle"
        | "release" | "datestamped" | "planeswalkerdeck" | "draftweekend" | "boosterfun"
        | "textured" | "galaxyfoil" | "surgefoil" | "gilded" | "neonink" | "halofoil"
        | "confettifoil" | "oilslick" | "stepandcompleat" | "embossed" | "serialized"
        | "doublerainbow" | "rainbowfoil" | "silverfoil" => {
            cond_one(array_member(dialect, "promo_types", &v))
        }
        _ => {
            let prefix = if negated { "not" } else { "is" };
            return Err(SearchError::UnsupportedKey(format!("{prefix}:{v}")));
        }
    };
    Ok(if negated { positive.not() } else { positive })
}

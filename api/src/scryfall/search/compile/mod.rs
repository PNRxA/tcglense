//! Compile a parsed `Node` AST into a `sea_orm::Condition`, one submodule per
//! supported filter family. Every user value binds as a parameter; only fixed
//! column-name constants are ever interpolated.
//!
//! [`compile`] walks the boolean tree (and/or/not) and dispatches each leaf to
//! its filter family: [`text`] (name/type/oracle), [`color`] (colours & colour
//! identity), [`mana`] (mana cost), [`numeric`] (mana value, power/toughness/
//! loyalty, prices, collector number), [`date`], [`rarity`], [`sets`],
//! [`enums`] (language/layout/game/oracle id), and [`predicates`] (`is:`/`not:`).
//! Shared low-level helpers live in [`common`].

mod color;
mod common;
mod date;
mod enums;
mod mana;
mod numeric;
mod predicates;
mod rarity;
mod sets;
mod text;

use sea_orm::Condition;

use crate::db::Dialect;
use super::error::SearchError;
use super::lexer::Op;
use super::parser::{Leaf, Node};

use color::color;
use common::{artists_count, cond_one, frame, has_predicate, keyword, legality, str_eq};
use date::{date, year};
use enums::{game, lang, layout, oracleid};
use mana::mana;
use numeric::{cmc, collector_number, price, pt, ptl};
use predicates::is_predicate;
use rarity::rarity;
use sets::{prints_filter, set, set_type, sets_filter};
use text::{exact, text_field, text_pattern};

pub(crate) use common::escape_like;

pub(super) fn compile(node: &Node, dialect: Dialect) -> Result<Condition, SearchError> {
    match node {
        Node::And(parts) => {
            let mut cond = Condition::all();
            for part in parts {
                cond = cond.add(compile(part, dialect)?);
            }
            Ok(cond)
        }
        Node::Or(parts) => {
            let mut cond = Condition::any();
            for part in parts {
                cond = cond.add(compile(part, dialect)?);
            }
            Ok(cond)
        }
        // Leaves are total (0/1), so a plain NOT is exact and NULL-safe.
        Node::Not(inner) => Ok(compile(inner, dialect)?.not()),
        Node::Leaf(leaf) => compile_leaf(leaf, dialect),
    }
}

fn compile_leaf(leaf: &Leaf, dialect: Dialect) -> Result<Condition, SearchError> {
    match leaf {
        Leaf::Name(s) => Ok(cond_one(text_pattern(dialect, "name", s)?)),
        Leaf::ExactName(s) => Ok(cond_one(exact(dialect, "name", s))),
        Leaf::Filter { key, op, value } => compile_filter(key, *op, value, dialect),
    }
}

fn compile_filter(key: &str, op: Op, value: &str, dialect: Dialect) -> Result<Condition, SearchError> {
    match key {
        "name" | "n" => Ok(cond_one(text_pattern(dialect, "name", value)?)),
        "t" | "type" => text_field(dialect, "type_line", "type", op, value),
        "o" | "oracle" | "fo" | "fulloracle" => {
            text_field(dialect, "oracle_text", "oracle", op, value)
        }
        "m" | "mana" => mana(dialect, op, value),
        "c" | "color" | "colors" => color(dialect, "colors", "c", op, value),
        "id" | "identity" | "ci" | "commander" | "cmdr" => {
            color(dialect, "color_identity", "id", op, value)
        }
        "cmc" | "mv" | "manavalue" => cmc(dialect, op, value),
        "pow" | "power" => ptl(dialect, "power", "pow", op, value),
        "tou" | "toughness" => ptl(dialect, "toughness", "tou", op, value),
        "loy" | "loyalty" => ptl(dialect, "loyalty", "loy", op, value),
        "pt" | "powtou" => pt(dialect, op, value),
        "def" | "defense" => ptl(dialect, "defense", "defense", op, value),
        "usd" => price(dialect, "price_usd", "usd", op, value),
        "usdfoil" => price(dialect, "price_usd_foil", "usdfoil", op, value),
        "eur" => price(dialect, "price_eur", "eur", op, value),
        "tix" => price(dialect, "price_tix", "tix", op, value),
        "year" => year(dialect, op, value),
        "date" | "released_at" => date(dialect, op, value),
        "r" | "rarity" => rarity(dialect, op, value),
        "s" | "set" | "e" | "edition" => set(dialect, op, value),
        "st" | "settype" => set_type(dialect, op, value),
        "cn" | "number" => collector_number(dialect, op, value),
        "lang" | "language" => lang(dialect, op, value),
        "layout" => layout(dialect, op, value),
        "is" => is_predicate(value, false, dialect),
        "not" => is_predicate(value, true, dialect),
        "game" => game(op, value),
        "oracleid" => oracleid(dialect, op, value),
        // Column-backed filters (Scryfall search parity).
        "f" | "format" | "legal" => legality(dialect, op, value, &["legal", "restricted"]),
        "banned" => legality(dialect, op, value, &["banned"]),
        "restricted" => legality(dialect, op, value, &["restricted"]),
        "kw" | "keyword" => keyword(dialect, op, value),
        "a" | "artist" => text_field(dialect, "artist", "artist", op, value),
        "artists" => artists_count(dialect, op, value),
        "ft" | "flavor" | "flavour" | "flavortext" => {
            text_field(dialect, "flavor_text", "flavor", op, value)
        }
        "wm" | "watermark" => text_field(dialect, "watermark", "watermark", op, value),
        "border" => str_eq(dialect, "border_color", "border", op, value),
        "frame" => frame(dialect, op, value),
        "stamp" => str_eq(dialect, "security_stamp", "stamp", op, value),
        "produces" => color(dialect, "produced_mana", "produces", op, value),
        "has" => has_predicate(value),
        // Sibling-print aggregates: counts over a card's other printings.
        "prints" => prints_filter(dialect, op, value),
        "sets" | "papersets" => sets_filter(dialect, op, value),
        // Recognised Scryfall filters we can't back yet: dataset-derived — Tagger
        // tags (#140) and cube (#141) — plus block/in/devotion/cheapest, and the
        // order:/direction:/unique: directives (handled before compile).
        "block" | "b" | "in" | "cube" | "function" | "oracletag" | "otag" | "art" | "arttag"
        | "atag" | "order" | "direction" | "unique" | "display" | "prefer" | "devotion"
        | "cheapest" | "new" | "old" => Err(SearchError::UnsupportedKey(key.to_string())),
        _ => Err(SearchError::UnknownKey(key.to_string())),
    }
}

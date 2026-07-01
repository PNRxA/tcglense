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

use super::error::SearchError;
use super::lexer::Op;
use super::parser::{Leaf, Node};

use color::color;
use common::cond_one;
use date::{date, year};
use enums::{game, lang, layout, oracleid};
use mana::mana;
use numeric::{cmc, collector_number, price, ptl};
use predicates::is_predicate;
use rarity::rarity;
use sets::{set, set_type};
use text::{contains, exact, text_field};

pub(crate) use common::escape_like;

pub(super) fn compile(node: &Node) -> Result<Condition, SearchError> {
    match node {
        Node::And(parts) => {
            let mut cond = Condition::all();
            for part in parts {
                cond = cond.add(compile(part)?);
            }
            Ok(cond)
        }
        Node::Or(parts) => {
            let mut cond = Condition::any();
            for part in parts {
                cond = cond.add(compile(part)?);
            }
            Ok(cond)
        }
        // Leaves are total (0/1), so a plain NOT is exact and NULL-safe.
        Node::Not(inner) => Ok(compile(inner)?.not()),
        Node::Leaf(leaf) => compile_leaf(leaf),
    }
}

fn compile_leaf(leaf: &Leaf) -> Result<Condition, SearchError> {
    match leaf {
        Leaf::Name(s) => Ok(cond_one(contains("name", s))),
        Leaf::ExactName(s) => Ok(cond_one(exact("name", s))),
        Leaf::Filter { key, op, value } => compile_filter(key, *op, value),
    }
}

fn compile_filter(key: &str, op: Op, value: &str) -> Result<Condition, SearchError> {
    match key {
        "name" | "n" => Ok(cond_one(contains("name", value))),
        "t" | "type" => text_field("type_line", "type", op, value),
        "o" | "oracle" | "fo" => text_field("oracle_text", "oracle", op, value),
        "m" | "mana" => mana(op, value),
        "c" | "color" | "colors" => color("colors", "c", op, value),
        "id" | "identity" | "ci" => color("color_identity", "id", op, value),
        "cmc" | "mv" | "manavalue" => cmc(op, value),
        "pow" | "power" => ptl("power", "pow", op, value),
        "tou" | "toughness" => ptl("toughness", "tou", op, value),
        "loy" | "loyalty" => ptl("loyalty", "loy", op, value),
        "usd" => price("price_usd", "usd", op, value),
        "usdfoil" => price("price_usd_foil", "usdfoil", op, value),
        "eur" => price("price_eur", "eur", op, value),
        "tix" => price("price_tix", "tix", op, value),
        "year" => year(op, value),
        "date" | "released_at" => date(op, value),
        "r" | "rarity" => rarity(op, value),
        "s" | "set" | "e" | "edition" => set(op, value),
        "st" | "settype" => set_type(op, value),
        "cn" | "number" => collector_number(op, value),
        "lang" | "language" => lang(op, value),
        "layout" => layout(op, value),
        "is" => is_predicate(value, false),
        "not" => is_predicate(value, true),
        "game" => game(op, value),
        "oracleid" => oracleid(op, value),
        // Recognised Scryfall filters we deliberately cannot back.
        "f" | "format" | "legal" | "banned" | "restricted" | "kw" | "keyword" | "a" | "artist"
        | "artists" | "ft" | "flavor" | "flavour" | "flavortext" | "wm" | "watermark" | "frame"
        | "border" | "stamp" | "block" | "b" | "in" | "prints" | "sets" | "papersets" | "cube"
        | "function" | "oracletag" | "otag" | "art" | "arttag" | "atag" | "order" | "direction"
        | "unique" | "display" | "prefer" | "produces" | "devotion" | "cheapest" | "has"
        | "new" | "old" => Err(SearchError::UnsupportedKey(key.to_string())),
        _ => Err(SearchError::UnknownKey(key.to_string())),
    }
}

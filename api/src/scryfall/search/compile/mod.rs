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
        Leaf::Name(s) => Ok(cond_one(text_pattern("name", s)?)),
        Leaf::ExactName(s) => Ok(cond_one(exact("name", s))),
        Leaf::Filter { key, op, value } => compile_filter(key, *op, value),
    }
}

fn compile_filter(key: &str, op: Op, value: &str) -> Result<Condition, SearchError> {
    match key {
        "name" | "n" => Ok(cond_one(text_pattern("name", value)?)),
        "t" | "type" => text_field("type_line", "type", op, value),
        "o" | "oracle" | "fo" | "fulloracle" => text_field("oracle_text", "oracle", op, value),
        "m" | "mana" => mana(op, value),
        "c" | "color" | "colors" => color("colors", "c", op, value),
        "id" | "identity" | "ci" | "commander" | "cmdr" => {
            color("color_identity", "id", op, value)
        }
        "cmc" | "mv" | "manavalue" => cmc(op, value),
        "pow" | "power" => ptl("power", "pow", op, value),
        "tou" | "toughness" => ptl("toughness", "tou", op, value),
        "loy" | "loyalty" => ptl("loyalty", "loy", op, value),
        "pt" | "powtou" => pt(op, value),
        "def" | "defense" => ptl("defense", "defense", op, value),
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
        // Column-backed filters (Scryfall search parity).
        "f" | "format" | "legal" => legality(op, value, &["legal", "restricted"]),
        "banned" => legality(op, value, &["banned"]),
        "restricted" => legality(op, value, &["restricted"]),
        "kw" | "keyword" => keyword(op, value),
        "a" | "artist" => text_field("artist", "artist", op, value),
        "artists" => artists_count(op, value),
        "ft" | "flavor" | "flavour" | "flavortext" => text_field("flavor_text", "flavor", op, value),
        "wm" | "watermark" => text_field("watermark", "watermark", op, value),
        "border" => str_eq("border_color", "border", op, value),
        "frame" => frame(op, value),
        "stamp" => str_eq("security_stamp", "stamp", op, value),
        "produces" => color("produced_mana", "produces", op, value),
        "has" => has_predicate(value),
        // Sibling-print aggregates: counts over a card's other printings.
        "prints" => prints_filter(op, value),
        "sets" | "papersets" => sets_filter(op, value),
        // Recognised Scryfall filters we can't back yet: dataset-derived — Tagger
        // tags (#140) and cube (#141) — plus block/in/devotion/cheapest, and the
        // order:/direction:/unique: directives (handled before compile).
        "block" | "b" | "in" | "cube" | "function" | "oracletag" | "otag" | "art" | "arttag"
        | "atag" | "order" | "direction" | "unique" | "display" | "prefer" | "devotion"
        | "cheapest" | "new" | "old" => Err(SearchError::UnsupportedKey(key.to_string())),
        _ => Err(SearchError::UnknownKey(key.to_string())),
    }
}

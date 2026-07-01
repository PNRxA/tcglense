//! Rarity filter: name normalisation, equality, and ordered comparisons.

use sea_orm::Condition;

use super::common::{raw, raw_vals, text_eq, text_ne};
use super::super::RARITIES;
use super::super::error::{SearchError, invalid};
use super::super::lexer::Op;

fn normalize_rarity(v: &str) -> Option<&'static str> {
    match v.to_lowercase().as_str() {
        "c" | "common" => Some("common"),
        "u" | "uncommon" => Some("uncommon"),
        "r" | "rare" => Some("rare"),
        "s" | "special" => Some("special"),
        "m" | "mythic" => Some("mythic"),
        "b" | "bonus" => Some("bonus"),
        _ => None,
    }
}

pub(super) fn rarity(op: Op, value: &str) -> Result<Condition, SearchError> {
    let name = normalize_rarity(value).ok_or_else(|| invalid("rarity", value, "unknown rarity"))?;
    match op {
        Op::Colon | Op::Eq => Ok(text_eq("rarity", name)),
        Op::Ne => Ok(text_ne("rarity", name)),
        Op::Gt | Op::Ge | Op::Lt | Op::Le => {
            let target = RARITIES.iter().position(|r| *r == name).unwrap();
            let names: Vec<String> = RARITIES
                .iter()
                .enumerate()
                .filter(|(rank, _)| cmp_rank(*rank, op, target))
                .map(|(_, r)| r.to_string())
                .collect();
            if names.is_empty() {
                return Ok(raw("1 = 0"));
            }
            let placeholders = vec!["?"; names.len()].join(", ");
            Ok(raw_vals(format!("IFNULL(rarity, '') IN ({placeholders})"), names))
        }
    }
}

fn cmp_rank(rank: usize, op: Op, target: usize) -> bool {
    match op {
        Op::Gt => rank > target,
        Op::Ge => rank >= target,
        Op::Lt => rank < target,
        Op::Le => rank <= target,
        _ => false,
    }
}

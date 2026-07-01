//! Mana-cost filter (`m:` / `m=`): symbol tokenisation, normalisation, and
//! multiset containment.

use sea_orm::Condition;
use sea_orm::Value;
use sea_orm::sea_query::Expr;

use super::super::MAX_MANA_SYMBOLS;
use super::super::WUBRG;
use super::super::error::{SearchError, invalid, unsupported_op};
use super::super::lexer::Op;

/// Normalise a mana symbol's interior (uppercase; order a two-colour hybrid WUBRG).
fn normalize_symbol(inner: &str) -> String {
    let up = inner.to_ascii_uppercase();
    if up.contains('/') {
        let parts: Vec<&str> = up.split('/').collect();
        if parts.len() == 2 && parts.iter().all(|p| p.chars().count() == 1) {
            let mut ps: Vec<char> = parts.iter().map(|p| p.chars().next().unwrap()).collect();
            ps.sort_by_key(|c| wubrg_index(*c));
            return format!("{{{}/{}}}", ps[0], ps[1]);
        }
    }
    format!("{{{up}}}")
}

fn wubrg_index(c: char) -> usize {
    WUBRG.iter().position(|&x| x == c).unwrap_or(99)
}

fn mana_tokens(value: &str) -> Result<Vec<String>, SearchError> {
    let chars: Vec<char> = value.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut out = Vec::new();
    while i < n {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '{' {
            let mut j = i + 1;
            let mut inner = String::new();
            while j < n && chars[j] != '}' {
                inner.push(chars[j]);
                j += 1;
            }
            if j >= n {
                return Err(invalid("mana", value, "unclosed mana symbol"));
            }
            out.push(normalize_symbol(&inner));
            i = j + 1;
        } else if c.is_ascii_digit() {
            let mut j = i;
            let mut num = String::new();
            while j < n && chars[j].is_ascii_digit() {
                num.push(chars[j]);
                j += 1;
            }
            out.push(format!("{{{num}}}"));
            i = j;
        } else if c.is_ascii_alphabetic() {
            out.push(format!("{{{}}}", c.to_ascii_uppercase()));
            i += 1;
        } else {
            return Err(invalid(
                "mana",
                value,
                &format!("unexpected '{c}' in mana cost"),
            ));
        }
    }
    Ok(out)
}

pub(super) fn mana(op: Op, value: &str) -> Result<Condition, SearchError> {
    let tokens = mana_tokens(value)?;
    if tokens.is_empty() {
        return Err(invalid("mana", value, "no mana symbols"));
    }
    if tokens.len() > MAX_MANA_SYMBOLS {
        return Err(invalid("mana", value, "too many mana symbols"));
    }
    match op {
        Op::Colon | Op::Ge => Ok(mana_contains(&tokens)),
        // Exact multiset: contains every symbol with its multiplicity AND no others
        // (equal total symbol count). Multiset comparison makes `=` order-independent
        // (e.g. `m=WW2` == `m=2WW`), matching Scryfall.
        Op::Eq => Ok(mana_contains(&tokens).add(mana_total_count("=", tokens.len() as i64))),
        // Strict superset: contains all query symbols AND strictly more in total.
        Op::Gt => Ok(mana_contains(&tokens).add(mana_total_count(">", tokens.len() as i64))),
        // Anything but the exact multiset.
        Op::Ne => Ok(mana_contains(&tokens)
            .add(mana_total_count("=", tokens.len() as i64))
            .not()),
        // Subset (`<`, `<=`) needs the cost's own symbol set — not supported yet.
        _ => Err(unsupported_op("mana", op)),
    }
}

/// Compare the total mana-symbol count of `mana_cost` (the number of `}`) to `n`.
fn mana_total_count(op_sql: &str, n: i64) -> sea_orm::sea_query::SimpleExpr {
    Expr::cust_with_values(
        format!(
            "(LENGTH(IFNULL(mana_cost, '')) - LENGTH(REPLACE(IFNULL(mana_cost, ''), '}}', ''))) {op_sql} ?"
        ),
        [n],
    )
}

/// Per-symbol multiplicity containment: each distinct symbol must appear at least
/// its query count (occurrences derived from the `REPLACE` length delta).
fn mana_contains(tokens: &[String]) -> Condition {
    let mut counts: Vec<(&str, i64)> = Vec::new();
    for tok in tokens {
        if let Some(entry) = counts.iter_mut().find(|(t, _)| *t == tok.as_str()) {
            entry.1 += 1;
        } else {
            counts.push((tok.as_str(), 1));
        }
    }
    let mut cond = Condition::all();
    for (tok, n) in counts {
        let tlen = tok.chars().count() as i64;
        cond = cond.add(Expr::cust_with_values(
            "(LENGTH(IFNULL(mana_cost, '')) - LENGTH(REPLACE(IFNULL(mana_cost, ''), ?, ''))) >= ?"
                .to_string(),
            [Value::from(tok.to_string()), Value::from(n * tlen)],
        ));
    }
    cond
}

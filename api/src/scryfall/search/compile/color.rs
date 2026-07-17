//! Colour and colour-identity filters (`c:` / `id:`).

use sea_orm::Condition;
use sea_orm::sea_query::SimpleExpr;

use super::super::WUBRG;
use super::super::error::{SearchError, invalid, unsupported_op};
use super::super::lexer::Op;
use super::common::{cmp_sql, cust_vals, raw, raw_vals};
use crate::db::Dialect;

enum ColorOperand {
    Letters(Vec<char>),
    Colorless,
    Multicolor,
    Count(i64),
}

fn nickname(lower: &str) -> Option<Vec<char>> {
    let letters = match lower {
        // guilds
        "azorius" => "WU",
        "dimir" => "UB",
        "rakdos" => "BR",
        "gruul" => "RG",
        "selesnya" => "GW",
        "orzhov" => "WB",
        "izzet" => "UR",
        "golgari" => "BG",
        "boros" => "RW",
        "simic" => "GU",
        // shards
        "bant" => "GWU",
        "esper" => "WUB",
        "grixis" => "UBR",
        "jund" => "BRG",
        "naya" => "RGW",
        // wedges
        "mardu" => "RWB",
        "temur" => "GUR",
        "abzan" => "WBG",
        "jeskai" => "URW",
        "sultai" => "BGU",
        // four-colour
        "artifice" | "yore-tiller" => "WUBR",
        "chaos" | "glint-eye" => "UBRG",
        "aggression" | "dune-brood" => "BRGW",
        "altruism" | "ink-treader" => "RGWU",
        "growth" | "witch-maw" => "GWUB",
        // five-colour
        "rainbow" => "WUBRG",
        _ => return None,
    };
    Some(order_wubrg(&letters.chars().collect::<Vec<_>>()))
}

fn order_wubrg(letters: &[char]) -> Vec<char> {
    WUBRG.into_iter().filter(|c| letters.contains(c)).collect()
}

fn complement(q: &[char]) -> Vec<char> {
    WUBRG.into_iter().filter(|c| !q.contains(c)).collect()
}

fn parse_color_operand(key: &str, value: &str) -> Result<ColorOperand, SearchError> {
    let lower = value.to_lowercase();
    if !lower.is_empty() && lower.chars().all(|c| c.is_ascii_digit()) {
        let n: i64 = lower
            .parse()
            .map_err(|_| invalid(key, value, "expected a number"))?;
        return Ok(ColorOperand::Count(n));
    }
    match lower.as_str() {
        "c" | "colorless" => return Ok(ColorOperand::Colorless),
        "m" | "multi" | "multicolor" | "multicolored" => return Ok(ColorOperand::Multicolor),
        // Full colour-name words.
        "white" => return Ok(ColorOperand::Letters(vec!['W'])),
        "blue" => return Ok(ColorOperand::Letters(vec!['U'])),
        "black" => return Ok(ColorOperand::Letters(vec!['B'])),
        "red" => return Ok(ColorOperand::Letters(vec!['R'])),
        "green" => return Ok(ColorOperand::Letters(vec!['G'])),
        _ => {}
    }
    if let Some(set) = nickname(&lower) {
        return Ok(ColorOperand::Letters(set));
    }
    let mut q: Vec<char> = Vec::new();
    for ch in lower.chars() {
        let up = match ch {
            'w' => 'W',
            'u' => 'U',
            'b' => 'B',
            'r' => 'R',
            'g' => 'G',
            _ => return Err(invalid(key, value, &format!("unknown color '{ch}'"))),
        };
        if !q.contains(&up) {
            q.push(up);
        }
    }
    Ok(ColorOperand::Letters(order_wubrg(&q)))
}

/// `(',' || COALESCE(col, '') || ',') LIKE '%,X,%'` — true iff colour X is present.
/// The colour letters are stored uppercase and the pattern is uppercase, so no case
/// fold is needed.
fn has(dialect: Dialect, col: &str, letter: char) -> SimpleExpr {
    cust_vals(
        dialect,
        format!("(',' || COALESCE({col}, '') || ',') LIKE ?"),
        [format!("%,{letter},%")],
    )
}

fn lacks(dialect: Dialect, col: &str, letter: char) -> SimpleExpr {
    cust_vals(
        dialect,
        format!("(',' || COALESCE({col}, '') || ',') NOT LIKE ?"),
        [format!("%,{letter},%")],
    )
}

fn all_has(dialect: Dialect, col: &str, q: &[char]) -> Condition {
    q.iter()
        .fold(Condition::all(), |cond, &x| cond.add(has(dialect, col, x)))
}

/// The exact-set condition: has every colour in Q and lacks every other.
fn exact_color(dialect: Dialect, col: &str, q: &[char]) -> Condition {
    let mut cond = all_has(dialect, col, q);
    for x in complement(q) {
        cond = cond.add(lacks(dialect, col, x));
    }
    cond
}

pub(super) fn color(
    dialect: Dialect,
    col: &str,
    key: &str,
    op: Op,
    value: &str,
) -> Result<Condition, SearchError> {
    match parse_color_operand(key, value)? {
        ColorOperand::Colorless => Ok(match op {
            Op::Colon | Op::Eq | Op::Le => raw(format!("{col} IS NULL")),
            Op::Ne | Op::Gt => raw(format!("{col} IS NOT NULL")),
            Op::Ge => Condition::all(),
            Op::Lt => raw("1 = 0"),
        }),
        ColorOperand::Multicolor => match op {
            Op::Colon | Op::Ge | Op::Eq => Ok(raw(format!("COALESCE({col}, '') LIKE '%,%'"))),
            _ => Err(unsupported_op(key, op)),
        },
        ColorOperand::Count(n) => {
            let sql = format!(
                "(CASE WHEN {col} IS NULL OR {col} = '' THEN 0 \
                 ELSE LENGTH({col}) - LENGTH(REPLACE({col}, ',', '')) + 1 END) {} ?",
                cmp_sql(op),
            );
            Ok(raw_vals(dialect, sql, [n]))
        }
        ColorOperand::Letters(q) => Ok(color_letters(dialect, col, op, &q)),
    }
}

fn color_letters(dialect: Dialect, col: &str, op: Op, q: &[char]) -> Condition {
    let comp = complement(q);
    match op {
        Op::Colon | Op::Ge => all_has(dialect, col, q),
        Op::Eq => exact_color(dialect, col, q),
        Op::Ne => exact_color(dialect, col, q).not(),
        Op::Gt => {
            let mut cond = all_has(dialect, col, q);
            if comp.is_empty() {
                cond = cond.add(raw("1 = 0"));
            } else {
                let extra = comp
                    .iter()
                    .fold(Condition::any(), |c, &x| c.add(has(dialect, col, x)));
                cond = cond.add(extra);
            }
            cond
        }
        Op::Le => comp
            .iter()
            .fold(Condition::all(), |c, &x| c.add(lacks(dialect, col, x))),
        Op::Lt => {
            let subset = comp
                .iter()
                .fold(Condition::all(), |c, &x| c.add(lacks(dialect, col, x)));
            subset.add(all_has(dialect, col, q).not())
        }
    }
}

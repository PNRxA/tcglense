//! Colour and colour-identity filters (`c:` / `id:`).

use sea_orm::Condition;
use sea_orm::sea_query::{Expr, SimpleExpr};

use super::common::{cmp_sql, raw, raw_vals};
use super::super::WUBRG;
use super::super::error::{SearchError, invalid, unsupported_op};
use super::super::lexer::Op;

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

/// `(',' || IFNULL(col, '') || ',') LIKE '%,X,%'` — true iff colour X is present.
fn has(col: &str, letter: char) -> SimpleExpr {
    Expr::cust_with_values(
        format!("(',' || IFNULL({col}, '') || ',') LIKE ?"),
        [format!("%,{letter},%")],
    )
}

fn lacks(col: &str, letter: char) -> SimpleExpr {
    Expr::cust_with_values(
        format!("(',' || IFNULL({col}, '') || ',') NOT LIKE ?"),
        [format!("%,{letter},%")],
    )
}

fn all_has(col: &str, q: &[char]) -> Condition {
    q.iter()
        .fold(Condition::all(), |cond, &x| cond.add(has(col, x)))
}

/// The exact-set condition: has every colour in Q and lacks every other.
fn exact_color(col: &str, q: &[char]) -> Condition {
    let mut cond = all_has(col, q);
    for x in complement(q) {
        cond = cond.add(lacks(col, x));
    }
    cond
}

pub(super) fn color(col: &str, key: &str, op: Op, value: &str) -> Result<Condition, SearchError> {
    match parse_color_operand(key, value)? {
        ColorOperand::Colorless => Ok(match op {
            Op::Colon | Op::Eq | Op::Le => raw(format!("{col} IS NULL")),
            Op::Ne | Op::Gt => raw(format!("{col} IS NOT NULL")),
            Op::Ge => Condition::all(),
            Op::Lt => raw("1 = 0"),
        }),
        ColorOperand::Multicolor => match op {
            Op::Colon | Op::Ge | Op::Eq => Ok(raw(format!("IFNULL({col}, '') LIKE '%,%'"))),
            _ => Err(unsupported_op(key, op)),
        },
        ColorOperand::Count(n) => {
            let sql = format!(
                "(CASE WHEN {col} IS NULL OR {col} = '' THEN 0 \
                 ELSE LENGTH({col}) - LENGTH(REPLACE({col}, ',', '')) + 1 END) {} ?",
                cmp_sql(op),
            );
            Ok(raw_vals(sql, [n]))
        }
        ColorOperand::Letters(q) => Ok(color_letters(col, op, &q)),
    }
}

fn color_letters(col: &str, op: Op, q: &[char]) -> Condition {
    let comp = complement(q);
    match op {
        Op::Colon | Op::Ge => all_has(col, q),
        Op::Eq => exact_color(col, q),
        Op::Ne => exact_color(col, q).not(),
        Op::Gt => {
            let mut cond = all_has(col, q);
            if comp.is_empty() {
                cond = cond.add(Expr::cust("1 = 0"));
            } else {
                let extra = comp
                    .iter()
                    .fold(Condition::any(), |c, &x| c.add(has(col, x)));
                cond = cond.add(extra);
            }
            cond
        }
        Op::Le => comp
            .iter()
            .fold(Condition::all(), |c, &x| c.add(lacks(col, x))),
        Op::Lt => {
            let subset = comp
                .iter()
                .fold(Condition::all(), |c, &x| c.add(lacks(col, x)));
            subset.add(all_has(col, q).not())
        }
    }
}

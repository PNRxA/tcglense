//! Release-date filters: `year:` and `date:` (ISO date or bare year).

use sea_orm::Condition;

use super::common::{cmp_sql, raw_vals};
use super::super::error::{SearchError, invalid};
use super::super::lexer::Op;

pub(super) fn year(op: Op, value: &str) -> Result<Condition, SearchError> {
    if value.len() != 4 || !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid("year", value, "expected a 4-digit year"));
    }
    let y: i32 = value.parse().unwrap();
    let sql = format!(
        "(released_at IS NOT NULL AND CAST(substr(released_at, 1, 4) AS INTEGER) {} ?)",
        cmp_sql(op)
    );
    Ok(raw_vals(sql, [y]))
}

fn is_iso_date(v: &str) -> bool {
    let b = v.as_bytes();
    v.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && v.char_indices()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

pub(super) fn date(op: Op, value: &str) -> Result<Condition, SearchError> {
    if is_iso_date(value) {
        let sql = format!(
            "(released_at IS NOT NULL AND released_at {} ?)",
            cmp_sql(op)
        );
        return Ok(raw_vals(sql, [value.to_string()]));
    }
    if value.len() == 4 && value.chars().all(|c| c.is_ascii_digit()) {
        let bound = |sym: &str, b: String| {
            raw_vals(
                format!("(released_at IS NOT NULL AND released_at {sym} ?)"),
                [b],
            )
        };
        let cond = match op {
            Op::Lt => bound("<", format!("{value}-01-01")),
            Op::Le => bound("<=", format!("{value}-12-31")),
            Op::Gt => bound(">", format!("{value}-12-31")),
            Op::Ge => bound(">=", format!("{value}-01-01")),
            Op::Colon | Op::Eq => raw_vals(
                "(released_at IS NOT NULL AND released_at LIKE ? ESCAPE '\\')".to_string(),
                [format!("{value}-%")],
            ),
            Op::Ne => raw_vals(
                "(released_at IS NOT NULL AND released_at NOT LIKE ? ESCAPE '\\')".to_string(),
                [format!("{value}-%")],
            ),
        };
        return Ok(cond);
    }
    Err(invalid(
        "date",
        value,
        "expected a date (YYYY-MM-DD) or a year",
    ))
}

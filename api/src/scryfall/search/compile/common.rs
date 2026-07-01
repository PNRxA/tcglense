//! Shared low-level helpers: single-clause condition wrappers, the custom-SQL
//! builders, the comparison-operator SQL, LIKE escaping, the numeric-string
//! guard, and the NULL-safe text equality/inequality builders.

use sea_orm::Condition;
use sea_orm::Value;
use sea_orm::sea_query::{Expr, SimpleExpr};

use super::super::error::{SearchError, invalid, unsupported_op};
use super::super::lexer::Op;

pub(super) fn cond_one(expr: SimpleExpr) -> Condition {
    Condition::all().add(expr)
}

/// A single-clause condition from a parameterless custom SQL fragment.
pub(super) fn raw<T>(sql: T) -> Condition
where
    T: Into<String>,
{
    cond_one(Expr::cust(sql))
}

/// A single-clause condition from a custom SQL fragment with bound values.
pub(super) fn raw_vals<T, V, I>(sql: T, values: I) -> Condition
where
    T: Into<String>,
    V: Into<Value>,
    I: IntoIterator<Item = V>,
{
    cond_one(Expr::cust_with_values(sql, values))
}

/// NULL-safe equality on a nullable text column (`IFNULL(col, '') = ?`).
pub(super) fn text_eq(col: &str, value: &str) -> Condition {
    raw_vals(format!("IFNULL({col}, '') = ?"), [value.to_string()])
}

/// NULL-safe inequality on a nullable text column (`col IS NULL OR col <> ?`).
pub(super) fn text_ne(col: &str, value: &str) -> Condition {
    raw_vals(format!("({col} IS NULL OR {col} <> ?)"), [value.to_string()])
}

/// Escape LIKE metacharacters so user input matches literally (with `ESCAPE '\'`).
pub(crate) fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// SQL symbol for a comparison operator (`:` means equality in numeric contexts).
pub(super) fn cmp_sql(op: Op) -> &'static str {
    match op {
        Op::Colon | Op::Eq => "=",
        Op::Ne => "<>",
        Op::Gt => ">",
        Op::Ge => ">=",
        Op::Lt => "<",
        Op::Le => "<=",
    }
}

/// SQL that is true only when `col` holds a plain integer string.
pub(super) fn numeric_guard(col: &str) -> String {
    format!("{col} IS NOT NULL AND {col} GLOB '[0-9]*' AND {col} NOT GLOB '*[^0-9]*'")
}

// ----- Additional column-backed builders (Scryfall search parity) -----

/// Comma-delimited membership: true iff `value` is one of the comma-joined tokens
/// in `col` (case-insensitive). Tokens are comma-wrapped so `foil` can't match
/// inside `nonfoil`.
pub(super) fn array_member(col: &str, value: &str) -> SimpleExpr {
    let needle = format!("%,{},%", escape_like(&value.to_lowercase()));
    Expr::cust_with_values(
        format!("(',' || LOWER(IFNULL({col}, '')) || ',') LIKE ? ESCAPE '\\'"),
        [needle],
    )
}

/// `col IS NOT NULL AND col <> ''` — a total presence test.
pub(super) fn col_present(col: &str) -> Condition {
    raw(format!("{col} IS NOT NULL AND {col} <> ''"))
}

/// `IFNULL(col, 0) = 1` — total boolean-column test (NULL treated as false).
pub(super) fn bool_true(col: &str) -> Condition {
    raw(format!("IFNULL({col}, 0) = 1"))
}

/// Case-insensitive exact match on a short enum-like column (border, stamp, …).
/// Total/NULL-safe so `-`/`not:` negate cleanly. Supports `:`/`=` and `!=`.
pub(super) fn str_eq(col: &str, key: &str, op: Op, value: &str) -> Result<Condition, SearchError> {
    let v = value.to_lowercase();
    match op {
        Op::Colon | Op::Eq => Ok(raw_vals(format!("LOWER(IFNULL({col}, '')) = ?"), [v])),
        Op::Ne => Ok(raw_vals(format!("LOWER(IFNULL({col}, '')) <> ?"), [v])),
        _ => Err(unsupported_op(key, op)),
    }
}

/// `kw:`/`keyword:` — keyword-ability membership.
pub(super) fn keyword(op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(array_member("keywords", value))),
        _ => Err(unsupported_op("keyword", op)),
    }
}

/// `artists:`/`artists>N` — number of credited artists (counted from artist_ids).
pub(super) fn artists_count(op: Op, value: &str) -> Result<Condition, SearchError> {
    let n: i64 = value
        .parse()
        .map_err(|_| invalid("artists", value, "expected a number"))?;
    let sql = format!(
        "(CASE WHEN artist_ids IS NULL OR artist_ids = '' THEN 0 \
         ELSE LENGTH(artist_ids) - LENGTH(REPLACE(artist_ids, ',', '')) + 1 END) {} ?",
        cmp_sql(op)
    );
    Ok(raw_vals(sql, [n]))
}

/// `frame:` — matches either the frame edition (`frame` column, e.g. `2015`) or a
/// frame effect (`frame_effects`, e.g. `showcase`, `extendedart`).
pub(super) fn frame(op: Op, value: &str) -> Result<Condition, SearchError> {
    let v = value.to_lowercase();
    let leaf = Condition::any()
        .add(Expr::cust_with_values(
            "LOWER(IFNULL(frame, '')) = ?".to_string(),
            [v.clone()],
        ))
        .add(array_member("frame_effects", &v));
    match op {
        Op::Colon | Op::Eq => Ok(leaf),
        Op::Ne => Ok(leaf.not()),
        _ => Err(unsupported_op("frame", op)),
    }
}

/// `f:`/`legal:`/`banned:`/`restricted:` — per-format legality from the stored
/// `legalities` JSON. The format is bound as the json path; an unknown format
/// simply matches nothing (`json_extract` → NULL), mirroring `settype`.
pub(super) fn legality(op: Op, value: &str, statuses: &[&str]) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => {}
        _ => return Err(unsupported_op("format", op)),
    }
    let fmt = value.to_lowercase();
    if fmt.is_empty() || !fmt.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(invalid("format", value, "unknown format"));
    }
    let placeholders = vec!["?"; statuses.len()].join(", ");
    let sql = format!("IFNULL(json_extract(legalities, ?), '') IN ({placeholders})");
    let mut vals: Vec<String> = Vec::with_capacity(statuses.len() + 1);
    vals.push(format!("$.{fmt}"));
    for s in statuses {
        vals.push((*s).to_string());
    }
    Ok(raw_vals(sql, vals))
}

/// `has:` presence filters over columns that carry optional print detail.
pub(super) fn has_predicate(value: &str) -> Result<Condition, SearchError> {
    match value.to_lowercase().as_str() {
        "flavor" | "flavour" | "flavortext" => Ok(col_present("flavor_text")),
        "watermark" => Ok(col_present("watermark")),
        "indicator" | "colorindicator" => Ok(col_present("color_indicator")),
        other => Err(SearchError::UnsupportedKey(format!("has:{other}"))),
    }
}

//! Shared low-level helpers: single-clause condition wrappers, the custom-SQL
//! builders, the comparison-operator SQL, LIKE escaping, the numeric-string
//! guard, and the NULL-safe text equality/inequality builders.

use std::borrow::Cow;

use sea_orm::Condition;
use sea_orm::Value;
use sea_orm::sea_query::{Expr, SimpleExpr};

use super::super::error::{SearchError, invalid, unsupported_op};
use super::super::lexer::Op;
use crate::db::Dialect;

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

/// Build a value-bearing custom `SimpleExpr`, renumbering `?`→`$N` for Postgres
/// (a no-op on SQLite). Every value-binding cust fragment must route through this
/// (or [`raw_vals`]) so its placeholders match the connection's backend — sea-query
/// stores the template verbatim and only tokenizes the *backend's own* placeholder
/// character, so a raw `?` fragment would drop its bound values on Postgres.
/// `pub(crate)` (re-exported beside [`escape_like`]): the card-name autocomplete
/// builds its index-matching `LOWER(COALESCE(…))` fragment through the same seam.
pub(crate) fn cust_vals<T, V, I>(dialect: Dialect, sql: T, values: I) -> SimpleExpr
where
    T: Into<String>,
    V: Into<Value>,
    I: IntoIterator<Item = V>,
{
    let sql = sql.into();
    // SQLite leaves the template untouched (`placeholders` borrows it), so avoid the
    // `into_owned()` clone on that hot path: reuse the owned `sql` when the renumber
    // was a no-op, and only take the Postgres-renumbered `String` when it allocated one.
    let renumbered = match dialect.placeholders(&sql) {
        Cow::Owned(s) => Some(s),
        Cow::Borrowed(_) => None,
    };
    Expr::cust_with_values(renumbered.unwrap_or(sql), values)
}

/// A single-clause condition from a custom SQL fragment with bound values (with the
/// same `?`→`$N` Postgres renumbering as [`cust_vals`]).
pub(super) fn raw_vals<T, V, I>(dialect: Dialect, sql: T, values: I) -> Condition
where
    T: Into<String>,
    V: Into<Value>,
    I: IntoIterator<Item = V>,
{
    cond_one(cust_vals(dialect, sql, values))
}

/// NULL-safe equality on a nullable text column (`COALESCE(col, '') = ?`).
pub(super) fn text_eq(dialect: Dialect, col: &str, value: &str) -> Condition {
    raw_vals(
        dialect,
        format!("COALESCE({col}, '') = ?"),
        [value.to_string()],
    )
}

/// NULL-safe inequality on a nullable text column (`col IS NULL OR col <> ?`).
pub(super) fn text_ne(dialect: Dialect, col: &str, value: &str) -> Condition {
    raw_vals(
        dialect,
        format!("({col} IS NULL OR {col} <> ?)"),
        [value.to_string()],
    )
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

// ----- Additional column-backed builders (Scryfall search parity) -----

/// Comma-delimited membership: true iff `value` is one of the comma-joined tokens
/// in `col` (case-insensitive). Tokens are comma-wrapped so `foil` can't match
/// inside `nonfoil`. The column values and `value` are short ASCII enum tokens
/// (finishes, keywords, promo/frame effects, colour letters), so `to_lowercase`
/// and SQLite's ASCII `LOWER()` agree byte-for-byte.
pub(super) fn array_member(dialect: Dialect, col: &str, value: &str) -> SimpleExpr {
    let needle = format!("%,{},%", escape_like(&value.to_lowercase()));
    cust_vals(
        dialect,
        format!("(',' || LOWER(COALESCE({col}, '')) || ',') LIKE ? ESCAPE '\\'"),
        [needle],
    )
}

/// `col IS NOT NULL AND col <> ''` — a total presence test.
pub(super) fn col_present(col: &str) -> Condition {
    raw(format!("{col} IS NOT NULL AND {col} <> ''"))
}

/// `col IS TRUE` — total boolean-column test (NULL treated as false). Valid and
/// identical on SQLite (≥3.23) and Postgres for the stored 0/1/NULL values.
pub(super) fn bool_true(col: &str) -> Condition {
    raw(format!("{col} IS TRUE"))
}

/// Case-insensitive exact match on a short enum-like column (border, stamp, …).
/// Total/NULL-safe so `-`/`not:` negate cleanly. Supports `:`/`=` and `!=`. The
/// column holds a short ASCII enum value, so `to_lowercase` == SQLite's `LOWER()`.
pub(super) fn str_eq(
    dialect: Dialect,
    col: &str,
    key: &str,
    op: Op,
    value: &str,
) -> Result<Condition, SearchError> {
    let v = value.to_lowercase();
    match op {
        Op::Colon | Op::Eq => Ok(raw_vals(
            dialect,
            format!("LOWER(COALESCE({col}, '')) = ?"),
            [v],
        )),
        Op::Ne => Ok(raw_vals(
            dialect,
            format!("LOWER(COALESCE({col}, '')) <> ?"),
            [v],
        )),
        _ => Err(unsupported_op(key, op)),
    }
}

/// `kw:`/`keyword:` — keyword-ability membership.
pub(super) fn keyword(dialect: Dialect, op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(array_member(dialect, "keywords", value))),
        _ => Err(unsupported_op("keyword", op)),
    }
}

/// `artists:`/`artists>N` — number of credited artists (counted from artist_ids).
pub(super) fn artists_count(
    dialect: Dialect,
    op: Op,
    value: &str,
) -> Result<Condition, SearchError> {
    let n: i64 = value
        .parse()
        .map_err(|_| invalid("artists", value, "expected a number"))?;
    let sql = format!(
        "(CASE WHEN artist_ids IS NULL OR artist_ids = '' THEN 0 \
         ELSE LENGTH(artist_ids) - LENGTH(REPLACE(artist_ids, ',', '')) + 1 END) {} ?",
        cmp_sql(op)
    );
    Ok(raw_vals(dialect, sql, [n]))
}

/// `frame:` — matches either the frame edition (`frame` column, e.g. `2015`) or a
/// frame effect (`frame_effects`, e.g. `showcase`, `extendedart`).
pub(super) fn frame(dialect: Dialect, op: Op, value: &str) -> Result<Condition, SearchError> {
    let v = value.to_lowercase();
    let leaf = Condition::any()
        .add(cust_vals(
            dialect,
            "LOWER(COALESCE(frame, '')) = ?".to_string(),
            [v.clone()],
        ))
        .add(array_member(dialect, "frame_effects", &v));
    match op {
        Op::Colon | Op::Eq => Ok(leaf),
        Op::Ne => Ok(leaf.not()),
        _ => Err(unsupported_op("frame", op)),
    }
}

/// `f:`/`legal:`/`banned:`/`restricted:` — per-format legality from the stored
/// `legalities` JSON. The format is bound as the json key; an unknown format
/// simply matches nothing (the extract yields NULL), mirroring `settype`. The
/// extract expression + bound key shape differ per backend (SQLite `json_extract`
/// with a `$.fmt` JSONPath vs. Postgres `jsonb ->>` with a bare `fmt` key).
pub(super) fn legality(
    dialect: Dialect,
    op: Op,
    value: &str,
    statuses: &[&str],
) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => {}
        _ => return Err(unsupported_op("format", op)),
    }
    let fmt = value.to_lowercase();
    if fmt.is_empty() || !fmt.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(invalid("format", value, "unknown format"));
    }
    let placeholders = vec!["?"; statuses.len()].join(", ");
    let sql = format!("{} IN ({placeholders})", dialect.legality_status_expr());
    // The status-expr's key `?` is the first placeholder, so its value leads the
    // bind vec — the `?`→`$N` renumber preserves that position.
    let mut vals: Vec<String> = Vec::with_capacity(statuses.len() + 1);
    vals.push(dialect.legality_key(&fmt));
    for s in statuses {
        vals.push((*s).to_string());
    }
    Ok(raw_vals(dialect, sql, vals))
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

//! Compile a parsed `Node` AST into a `sea_orm::Condition`, with one function per
//! supported filter. Every user value binds as a parameter; only fixed column-name
//! constants are ever interpolated.

use sea_orm::Condition;
use sea_orm::Value;
use sea_orm::sea_query::{Expr, SimpleExpr};

use super::error::{SearchError, invalid, unsupported_op};
use super::lexer::Op;
use super::parser::{Leaf, Node};
use super::{MAX_MANA_SYMBOLS, RARITIES, WUBRG};

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
        // Recognised Scryfall filters we can't back yet: sibling-print aggregates
        // (prints/sets — Phase 5), result-shaping (order/unique/… — handled before
        // compile), and dataset-derived filters we don't ingest — Tagger tags
        // (function/otag/atag/… → issue #140) and cube (issue #141).
        // Sibling-print aggregates (Phase 5): counts over a card's other printings.
        "prints" => prints_filter(op, value),
        "sets" | "papersets" => sets_filter(op, value),
        "block" | "b" | "in" | "cube" | "function" | "oracletag" | "otag" | "art" | "arttag"
        | "atag" | "order" | "direction" | "unique" | "display" | "prefer" | "devotion"
        | "cheapest" | "new" | "old" => Err(SearchError::UnsupportedKey(key.to_string())),
        _ => Err(SearchError::UnknownKey(key.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Leaf builders
// ---------------------------------------------------------------------------

fn cond_one(expr: SimpleExpr) -> Condition {
    Condition::all().add(expr)
}

/// Case-insensitive substring match; total (NULL → no match).
fn contains(col: &str, value: &str) -> SimpleExpr {
    let pattern = format!("%{}%", escape_like(value));
    Expr::cust_with_values(format!("IFNULL({col}, '') LIKE ? ESCAPE '\\'"), [pattern])
}

/// Case-insensitive exact match (wildcard-free LIKE keeps ASCII case folding).
fn exact(col: &str, value: &str) -> SimpleExpr {
    let pattern = escape_like(value);
    Expr::cust_with_values(format!("IFNULL({col}, '') LIKE ? ESCAPE '\\'"), [pattern])
}

fn text_field(col: &str, key: &str, op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(text_pattern(col, value)?)),
        _ => Err(unsupported_op(key, op)),
    }
}

/// A text-column predicate: a Scryfall `/regex/` literal compiles to a `REGEXP`
/// match, otherwise a case-insensitive substring.
fn text_pattern(col: &str, value: &str) -> Result<SimpleExpr, SearchError> {
    match as_regex(value) {
        Some(pattern) => regex_expr(col, pattern),
        None => Ok(contains(col, value)),
    }
}

/// Recognise a `/pattern/` regex literal, returning the inner pattern.
fn as_regex(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'/' && bytes[bytes.len() - 1] == b'/' {
        Some(&value[1..value.len() - 1])
    } else {
        None
    }
}

/// Compile a regex literal to `IFNULL(col, '') REGEXP ?`. The pattern is validated
/// with the same `regex` crate SQLite uses (a bad pattern is a 422, not a SQL
/// error) and made case-insensitive to match Scryfall's default.
fn regex_expr(col: &str, pattern: &str) -> Result<SimpleExpr, SearchError> {
    let ci = format!("(?i){pattern}");
    regex::Regex::new(&ci).map_err(|_| invalid("regex", pattern, "invalid regular expression"))?;
    Ok(Expr::cust_with_values(
        format!("IFNULL({col}, '') REGEXP ?"),
        [ci],
    ))
}

/// Case-insensitive exact match on a short enum-like column (border, stamp, …).
/// Total/NULL-safe so `-`/`not:` negate cleanly. Supports `:`/`=` and `!=`.
fn str_eq(col: &str, key: &str, op: Op, value: &str) -> Result<Condition, SearchError> {
    let v = value.to_lowercase();
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(Expr::cust_with_values(
            format!("LOWER(IFNULL({col}, '')) = ?"),
            [v],
        ))),
        Op::Ne => Ok(cond_one(Expr::cust_with_values(
            format!("LOWER(IFNULL({col}, '')) <> ?"),
            [v],
        ))),
        _ => Err(unsupported_op(key, op)),
    }
}

/// Comma-delimited membership: true iff `value` is one of the comma-joined tokens
/// in `col` (case-insensitive). Tokens are comma-wrapped so `foil` can't match
/// inside `nonfoil`.
fn array_member(col: &str, value: &str) -> SimpleExpr {
    let needle = format!("%,{},%", escape_like(&value.to_lowercase()));
    Expr::cust_with_values(
        format!("(',' || LOWER(IFNULL({col}, '')) || ',') LIKE ? ESCAPE '\\'"),
        [needle],
    )
}

/// `col IS NOT NULL AND col <> ''` — a total presence test.
fn col_present(col: &str) -> Condition {
    cond_one(Expr::cust(format!("{col} IS NOT NULL AND {col} <> ''")))
}

/// `IFNULL(col, 0) = 1` — total boolean-column test (NULL treated as false).
fn bool_true(col: &str) -> Condition {
    cond_one(Expr::cust(format!("IFNULL({col}, 0) = 1")))
}

/// `kw:`/`keyword:` — keyword-ability membership.
fn keyword(op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(array_member("keywords", value))),
        _ => Err(unsupported_op("keyword", op)),
    }
}

/// `artists:`/`artists>N` — number of credited artists (counted from artist_ids).
fn artists_count(op: Op, value: &str) -> Result<Condition, SearchError> {
    let n: i64 = value
        .parse()
        .map_err(|_| invalid("artists", value, "expected a number"))?;
    let sql = format!(
        "(CASE WHEN artist_ids IS NULL OR artist_ids = '' THEN 0 \
         ELSE LENGTH(artist_ids) - LENGTH(REPLACE(artist_ids, ',', '')) + 1 END) {} ?",
        cmp_sql(op)
    );
    Ok(cond_one(Expr::cust_with_values(sql, [n])))
}

/// `frame:` — matches either the frame edition (`frame` column, e.g. `2015`) or a
/// frame effect (`frame_effects`, e.g. `showcase`, `extendedart`).
fn frame(op: Op, value: &str) -> Result<Condition, SearchError> {
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
fn legality(op: Op, value: &str, statuses: &[&str]) -> Result<Condition, SearchError> {
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
    let mut vals: Vec<Value> = Vec::with_capacity(statuses.len() + 1);
    vals.push(Value::from(format!("$.{fmt}")));
    for s in statuses {
        vals.push(Value::from((*s).to_string()));
    }
    Ok(cond_one(Expr::cust_with_values(sql, vals)))
}

/// `has:` presence filters over columns that carry optional print detail.
fn has_predicate(value: &str) -> Result<Condition, SearchError> {
    match value.to_lowercase().as_str() {
        "flavor" | "flavour" | "flavortext" => Ok(col_present("flavor_text")),
        "watermark" => Ok(col_present("watermark")),
        "indicator" | "colorindicator" => Ok(col_present("color_indicator")),
        other => Err(SearchError::UnsupportedKey(format!("has:{other}"))),
    }
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
fn cmp_sql(op: Op) -> &'static str {
    match op {
        Op::Colon | Op::Eq => "=",
        Op::Ne => "<>",
        Op::Gt => ">",
        Op::Ge => ">=",
        Op::Lt => "<",
        Op::Le => "<=",
    }
}

// ----- mana value (cmc) -----

fn cmc(op: Op, value: &str) -> Result<Condition, SearchError> {
    let lower = value.to_lowercase();
    if lower == "even" || lower == "odd" {
        // Parity is an equality-only predicate; a relational operator is meaningless.
        if !matches!(op, Op::Colon | Op::Eq) {
            return Err(unsupported_op("cmc", op));
        }
        let parity = if lower == "even" { 0 } else { 1 };
        let sql = format!(
            "(cmc IS NOT NULL AND cmc = CAST(cmc AS INTEGER) AND CAST(cmc AS INTEGER) % 2 = {parity})"
        );
        return Ok(cond_one(Expr::cust(sql)));
    }
    let n: f64 = value
        .parse()
        .map_err(|_| invalid("cmc", value, "expected a number"))?;
    let sql = format!("(cmc IS NOT NULL AND cmc {} ?)", cmp_sql(op));
    Ok(cond_one(Expr::cust_with_values(sql, [n])))
}

// ----- power / toughness / loyalty (text columns, can be *, 1+*, X) -----

fn stat_column(s: &str) -> Option<&'static str> {
    match s {
        "pow" | "power" => Some("power"),
        "tou" | "toughness" => Some("toughness"),
        "loy" | "loyalty" => Some("loyalty"),
        "def" | "defense" => Some("defense"),
        _ => None,
    }
}

/// SQL that is true only when `col` holds a plain integer string.
fn numeric_guard(col: &str) -> String {
    format!("{col} IS NOT NULL AND {col} GLOB '[0-9]*' AND {col} NOT GLOB '*[^0-9]*'")
}

fn ptl(col: &str, key: &str, op: Op, value: &str) -> Result<Condition, SearchError> {
    // RHS may name another stat column (pow>tou): numeric compare, both guarded.
    if let Some(other) = stat_column(&value.to_lowercase()) {
        let sql = format!(
            "(({}) AND ({}) AND CAST({col} AS REAL) {} CAST({other} AS REAL))",
            numeric_guard(col),
            numeric_guard(other),
            cmp_sql(op),
        );
        return Ok(cond_one(Expr::cust(sql)));
    }
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(Expr::cust_with_values(
            format!("IFNULL({col}, '') = ?"),
            [value.to_string()],
        ))),
        Op::Ne => Ok(cond_one(Expr::cust_with_values(
            format!("({col} IS NULL OR {col} <> ?)"),
            [value.to_string()],
        ))),
        Op::Gt | Op::Ge | Op::Lt | Op::Le => {
            let n: f64 = value
                .parse()
                .map_err(|_| invalid(key, value, "expected a number"))?;
            let sql = format!(
                "(({}) AND CAST({col} AS REAL) {} ?)",
                numeric_guard(col),
                cmp_sql(op)
            );
            Ok(cond_one(Expr::cust_with_values(sql, [n])))
        }
    }
}

/// `pt:`/`powtou:` — compare power + toughness (both numeric-guarded).
fn pt(op: Op, value: &str) -> Result<Condition, SearchError> {
    let n: f64 = value
        .parse()
        .map_err(|_| invalid("pt", value, "expected a number"))?;
    let sql = format!(
        "(({}) AND ({}) AND CAST(power AS REAL) + CAST(toughness AS REAL) {} ?)",
        numeric_guard("power"),
        numeric_guard("toughness"),
        cmp_sql(op)
    );
    Ok(cond_one(Expr::cust_with_values(sql, [n])))
}

// ----- prices (text decimal columns) -----

fn price(col: &str, key: &str, op: Op, value: &str) -> Result<Condition, SearchError> {
    let n: f64 = value
        .parse()
        .map_err(|_| invalid(key, value, "expected a number"))?;
    let sql = format!(
        "({col} IS NOT NULL AND {col} <> '' AND CAST({col} AS REAL) {} ?)",
        cmp_sql(op)
    );
    Ok(cond_one(Expr::cust_with_values(sql, [n])))
}

// ----- release year / date -----

fn year(op: Op, value: &str) -> Result<Condition, SearchError> {
    if value.len() != 4 || !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid("year", value, "expected a 4-digit year"));
    }
    let y: i32 = value.parse().unwrap();
    let sql = format!(
        "(released_at IS NOT NULL AND CAST(substr(released_at, 1, 4) AS INTEGER) {} ?)",
        cmp_sql(op)
    );
    Ok(cond_one(Expr::cust_with_values(sql, [y])))
}

fn is_iso_date(v: &str) -> bool {
    let b = v.as_bytes();
    v.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && v.char_indices()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn date(op: Op, value: &str) -> Result<Condition, SearchError> {
    if is_iso_date(value) {
        let sql = format!(
            "(released_at IS NOT NULL AND released_at {} ?)",
            cmp_sql(op)
        );
        return Ok(cond_one(Expr::cust_with_values(sql, [value.to_string()])));
    }
    if value.len() == 4 && value.chars().all(|c| c.is_ascii_digit()) {
        let bound = |sym: &str, b: String| {
            cond_one(Expr::cust_with_values(
                format!("(released_at IS NOT NULL AND released_at {sym} ?)"),
                [b],
            ))
        };
        let cond = match op {
            Op::Lt => bound("<", format!("{value}-01-01")),
            Op::Le => bound("<=", format!("{value}-12-31")),
            Op::Gt => bound(">", format!("{value}-12-31")),
            Op::Ge => bound(">=", format!("{value}-01-01")),
            Op::Colon | Op::Eq => cond_one(Expr::cust_with_values(
                "(released_at IS NOT NULL AND released_at LIKE ? ESCAPE '\\')".to_string(),
                [format!("{value}-%")],
            )),
            Op::Ne => cond_one(Expr::cust_with_values(
                "(released_at IS NOT NULL AND released_at NOT LIKE ? ESCAPE '\\')".to_string(),
                [format!("{value}-%")],
            )),
        };
        return Ok(cond);
    }
    Err(invalid(
        "date",
        value,
        "expected a date (YYYY-MM-DD) or a year",
    ))
}

// ----- rarity -----

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

fn rarity(op: Op, value: &str) -> Result<Condition, SearchError> {
    let name = normalize_rarity(value).ok_or_else(|| invalid("rarity", value, "unknown rarity"))?;
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(Expr::cust_with_values(
            "IFNULL(rarity, '') = ?".to_string(),
            [name.to_string()],
        ))),
        Op::Ne => Ok(cond_one(Expr::cust_with_values(
            "(rarity IS NULL OR rarity <> ?)".to_string(),
            [name.to_string()],
        ))),
        Op::Gt | Op::Ge | Op::Lt | Op::Le => {
            let target = RARITIES.iter().position(|r| *r == name).unwrap();
            let names: Vec<String> = RARITIES
                .iter()
                .enumerate()
                .filter(|(rank, _)| cmp_rank(*rank, op, target))
                .map(|(_, r)| r.to_string())
                .collect();
            if names.is_empty() {
                return Ok(cond_one(Expr::cust("1 = 0")));
            }
            let placeholders = vec!["?"; names.len()].join(", ");
            Ok(cond_one(Expr::cust_with_values(
                format!("IFNULL(rarity, '') IN ({placeholders})"),
                names,
            )))
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

// ----- set / collector number / language / layout -----

fn set(op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(Expr::cust_with_values(
            "set_code = ?".to_string(),
            [value.to_lowercase()],
        ))),
        Op::Ne => Ok(cond_one(Expr::cust_with_values(
            "set_code <> ?".to_string(),
            [value.to_lowercase()],
        ))),
        _ => Err(unsupported_op("set", op)),
    }
}

/// Map a Scryfall `st:` value to the provider's stored `set_type`. Most pass
/// through unchanged; a couple of Scryfall aliases differ from the stored name.
fn normalize_set_type(v: &str) -> String {
    match v.to_lowercase().as_str() {
        "boxset" => "box",
        "unset" => "funny",
        other => other,
    }
    .to_string()
}

/// `st:` / `settype:` — match a printing whose *set* has the given Scryfall
/// `set_type` (e.g. `expansion`, `commander`, `funny`). `set_type` lives on
/// `card_sets`, not `cards`, so we resolve it with a game-scoped subquery on the
/// set code. `set_code` is non-null, so `IN` / `NOT IN` stay total (0/1) and the
/// leaf negates cleanly. An unrecognised set type simply matches no rows (mirrors
/// Scryfall, and lets new provider set types work without a code change).
fn set_type(op: Op, value: &str) -> Result<Condition, SearchError> {
    let st = normalize_set_type(value);
    let select = "SELECT code FROM card_sets WHERE game = ? AND LOWER(IFNULL(set_type, '')) = ?";
    let bind = || {
        [
            Value::from(crate::scryfall::GAME.to_string()),
            Value::from(st.clone()),
        ]
    };
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(Expr::cust_with_values(
            format!("set_code IN ({select})"),
            bind(),
        ))),
        Op::Ne => Ok(cond_one(Expr::cust_with_values(
            format!("set_code NOT IN ({select})"),
            bind(),
        ))),
        _ => Err(unsupported_op("settype", op)),
    }
}

/// `prints <op> N` — number of printings of this card (its `oracle_id` siblings).
fn prints_filter(op: Op, value: &str) -> Result<Condition, SearchError> {
    let n: i64 = value
        .parse()
        .map_err(|_| invalid("prints", value, "expected a number"))?;
    Ok(cond_one(sibling_count("COUNT(*)", op, n)))
}

/// `sets`/`papersets <op> N` — number of distinct sets this card appears in.
/// (Equal here since the catalogue is paper-only.)
fn sets_filter(op: Op, value: &str) -> Result<Condition, SearchError> {
    let n: i64 = value
        .parse()
        .map_err(|_| invalid("sets", value, "expected a number"))?;
    Ok(cond_one(sibling_count("COUNT(DISTINCT c2.set_code)", op, n)))
}

/// A `GAME`-scoped correlated subquery over a card's `oracle_id` siblings (a card
/// with no `oracle_id` is its own sole sibling, so the count is always ≥ 1 and the
/// leaf stays total for `-`/`not:`). `agg` is a fixed aggregate; user input binds.
fn sibling_count(agg: &str, op: Op, n: i64) -> SimpleExpr {
    let sql = format!(
        "(SELECT {agg} FROM cards c2 WHERE c2.game = ? AND \
         ((cards.oracle_id IS NOT NULL AND c2.oracle_id = cards.oracle_id) \
          OR (cards.oracle_id IS NULL AND c2.id = cards.id))) {} ?",
        cmp_sql(op)
    );
    Expr::cust_with_values(
        sql,
        [
            Value::from(crate::scryfall::GAME.to_string()),
            Value::from(n),
        ],
    )
}

fn collector_number(op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(Expr::cust_with_values(
            "lower(collector_number) = ?".to_string(),
            [value.to_lowercase()],
        ))),
        Op::Ne => Ok(cond_one(Expr::cust_with_values(
            "lower(collector_number) <> ?".to_string(),
            [value.to_lowercase()],
        ))),
        Op::Gt | Op::Ge | Op::Lt | Op::Le => {
            let n: i32 = value
                .parse()
                .map_err(|_| invalid("cn", value, "range requires a numeric collector number"))?;
            let sql = format!(
                "(collector_number_int IS NOT NULL AND collector_number_int {} ?)",
                cmp_sql(op)
            );
            Ok(cond_one(Expr::cust_with_values(sql, [n])))
        }
    }
}

fn lang_code(lower: &str) -> String {
    match lower {
        "english" => "en",
        "japanese" => "ja",
        "german" => "de",
        "french" => "fr",
        "italian" => "it",
        "spanish" => "es",
        "portuguese" => "pt",
        "russian" => "ru",
        "korean" => "ko",
        "chinese simplified" => "zhs",
        "chinese traditional" => "zht",
        other => other,
    }
    .to_string()
}

fn lang(op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => {
            let lower = value.to_lowercase();
            if lower == "any" || lower == "*" {
                return Ok(Condition::all());
            }
            Ok(cond_one(Expr::cust_with_values(
                "lang = ?".to_string(),
                [lang_code(&lower)],
            )))
        }
        _ => Err(unsupported_op("lang", op)),
    }
}

fn layout(op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(Expr::cust_with_values(
            "IFNULL(layout, '') = ?".to_string(),
            [value.to_lowercase()],
        ))),
        _ => Err(unsupported_op("layout", op)),
    }
}

// ----- is: / not: predicates -----

fn is_predicate(value: &str, negated: bool) -> Result<Condition, SearchError> {
    let v = value.to_lowercase();
    let positive: Condition = match v.as_str() {
        "split" | "flip" | "transform" | "meld" | "saga" | "leveler" | "adventure" | "emblem"
        | "class" | "case" | "battle" | "planar" | "scheme" | "vanguard" | "mutate"
        | "prototype" | "augment" | "host" | "normal" => cond_one(Expr::cust_with_values(
            "IFNULL(layout, '') = ?".to_string(),
            [v.clone()],
        )),
        "mdfc" | "modaldfc" | "modal_dfc" => {
            cond_one(Expr::cust("IFNULL(layout, '') = 'modal_dfc'"))
        }
        "dfc" | "doublefaced" | "double_faced" => cond_one(Expr::cust(
            "IFNULL(layout, '') IN ('transform', 'modal_dfc', 'meld', 'reversible_card')",
        )),
        "token" => cond_one(Expr::cust(
            "IFNULL(layout, '') IN ('token', 'double_faced_token')",
        )),
        "colorless" => cond_one(Expr::cust("colors IS NULL")),
        "multicolored" | "multicolor" => {
            cond_one(Expr::cust("colors IS NOT NULL AND colors LIKE '%,%'"))
        }
        "monocolored" | "monocolor" => {
            cond_one(Expr::cust("colors IS NOT NULL AND colors NOT LIKE '%,%'"))
        }
        "phyrexian" => cond_one(Expr::cust("IFNULL(mana_cost, '') LIKE '%/P}%'")),
        "hybrid" => cond_one(Expr::cust(
            "IFNULL(mana_cost, '') LIKE '%/%' AND IFNULL(mana_cost, '') NOT LIKE '%/P}%'",
        )),
        "digital" => cond_one(Expr::cust("1 = 0")),
        // Card-type-derived predicates. type_line is title-case from Scryfall but
        // SQLite LIKE folds ASCII case, so lower-case patterns match. Each arm is
        // total (0/1, NULL-safe) so `not:` negation stays exact.
        "permanent" => cond_one(Expr::cust(
            "type_line IS NOT NULL \
             AND (type_line LIKE '%artifact%' OR type_line LIKE '%creature%' \
                  OR type_line LIKE '%enchantment%' OR type_line LIKE '%land%' \
                  OR type_line LIKE '%planeswalker%' OR type_line LIKE '%battle%') \
             AND type_line NOT LIKE '%instant%' AND type_line NOT LIKE '%sorcery%'",
        )),
        // "Spell" is decided by the FRONT face you cast: a card's stored type_line
        // joins faces as "front // back", so test only the part before " // " for
        // land-ness — otherwise spell//land modal DFCs (Kazandu Mammoth and the rest
        // of the Zendikar Rising cycle) would be wrongly excluded by their land back.
        "spell" => cond_one(Expr::cust(
            "type_line IS NOT NULL \
             AND (CASE WHEN INSTR(type_line, ' // ') > 0 \
                       THEN SUBSTR(type_line, 1, INSTR(type_line, ' // ') - 1) \
                       ELSE type_line END) NOT LIKE '%land%' \
             AND IFNULL(layout, '') NOT IN \
                 ('token', 'double_faced_token', 'emblem', 'art_series')",
        )),
        "vanilla" => cond_one(Expr::cust(
            "type_line IS NOT NULL AND type_line LIKE '%creature%' \
             AND (oracle_text IS NULL OR oracle_text = '')",
        )),
        // Finish availability (from the finishes array).
        "foil" => cond_one(array_member("finishes", "foil")),
        "nonfoil" => cond_one(array_member("finishes", "nonfoil")),
        "etched" => cond_one(array_member("finishes", "etched")),
        // Print-detail boolean flags.
        "fullart" => bool_true("full_art"),
        "textless" => bool_true("textless"),
        "oversized" => bool_true("oversized"),
        "promo" => bool_true("promo"),
        "reprint" => bool_true("reprint"),
        "variation" => bool_true("variation"),
        "booster" => bool_true("booster"),
        "spotlight" | "storyspotlight" => bool_true("story_spotlight"),
        "contentwarning" => bool_true("content_warning"),
        "hires" | "highres" => bool_true("highres_image"),
        "reserved" => bool_true("reserved"),
        "gamechanger" => bool_true("game_changer"),
        // Presence of an optional print attribute.
        "watermark" => col_present("watermark"),
        "indicator" | "colorindicator" => col_present("color_indicator"),
        // Promo / product-origin categories (from promo_types).
        "buyabox" | "prerelease" | "promopack" | "gameday" | "intropack" | "giftbox" | "bundle"
        | "release" | "datestamped" | "planeswalkerdeck" | "draftweekend" | "boosterfun"
        | "textured" | "galaxyfoil" | "surgefoil" | "gilded" | "neonink" | "halofoil"
        | "confettifoil" | "oilslick" | "stepandcompleat" | "embossed" | "serialized"
        | "doublerainbow" | "rainbowfoil" | "silverfoil" => {
            cond_one(array_member("promo_types", &v))
        }
        _ => {
            let prefix = if negated { "not" } else { "is" };
            return Err(SearchError::UnsupportedKey(format!("{prefix}:{v}")));
        }
    };
    Ok(if negated { positive.not() } else { positive })
}

fn game(op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => match value.to_lowercase().as_str() {
            // Catalogue is paper-only: paper matches all, other engines match none.
            "paper" => Ok(Condition::all()),
            _ => Ok(cond_one(Expr::cust("1 = 0"))),
        },
        _ => Err(unsupported_op("game", op)),
    }
}

fn oracleid(op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => Ok(cond_one(Expr::cust_with_values(
            "IFNULL(oracle_id, '') = ?".to_string(),
            [value.to_string()],
        ))),
        Op::Ne => Ok(cond_one(Expr::cust_with_values(
            "(oracle_id IS NULL OR oracle_id <> ?)".to_string(),
            [value.to_string()],
        ))),
        _ => Err(unsupported_op("oracleid", op)),
    }
}

// ----- colours & colour identity -----

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

fn color(col: &str, key: &str, op: Op, value: &str) -> Result<Condition, SearchError> {
    match parse_color_operand(key, value)? {
        ColorOperand::Colorless => Ok(match op {
            Op::Colon | Op::Eq | Op::Le => cond_one(Expr::cust(format!("{col} IS NULL"))),
            Op::Ne | Op::Gt => cond_one(Expr::cust(format!("{col} IS NOT NULL"))),
            Op::Ge => Condition::all(),
            Op::Lt => cond_one(Expr::cust("1 = 0")),
        }),
        ColorOperand::Multicolor => match op {
            Op::Colon | Op::Ge | Op::Eq => Ok(cond_one(Expr::cust(format!(
                "IFNULL({col}, '') LIKE '%,%'"
            )))),
            _ => Err(unsupported_op(key, op)),
        },
        ColorOperand::Count(n) => {
            let sql = format!(
                "(CASE WHEN {col} IS NULL OR {col} = '' THEN 0 \
                 ELSE LENGTH({col}) - LENGTH(REPLACE({col}, ',', '')) + 1 END) {} ?",
                cmp_sql(op),
            );
            Ok(cond_one(Expr::cust_with_values(sql, [n])))
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

// ----- mana cost -----

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

fn mana(op: Op, value: &str) -> Result<Condition, SearchError> {
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
fn mana_total_count(op_sql: &str, n: i64) -> SimpleExpr {
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

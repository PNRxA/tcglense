//! A Scryfall-style search-query compiler.
//!
//! Turns a query string like `c:rg t:creature mv>=3 r:rare -o:flying` into a
//! [`sea_orm::Condition`] over the `cards` table, supporting the subset of
//! [Scryfall syntax](https://scryfall.com/docs/syntax) our columns can back:
//! name / type / oracle text, colours & colour identity, mana cost, mana value,
//! power / toughness / loyalty, prices, rarity, set, collector number, language,
//! layout, release date, plus boolean `and`/`or`, `-` negation, parentheses, and
//! quoted phrases. Filters we cannot back (oracle keywords, format legality,
//! artist, …) return a [`SearchError`] that the handler maps to HTTP 422.
//!
//! Pipeline: [`lex`] → [`Parser::parse_query`] (recursive descent → [`Node`]) →
//! [`compile`] (→ `Condition`). Three rules keep it safe and predictable:
//!
//! 1. **No SQL injection.** Every user value is bound as a parameter (via
//!    `Expr::cust_with_values` `?` placeholders or the typed `Expr` builders);
//!    the only interpolated identifiers are trusted, fixed column-name constants.
//! 2. **Total leaves.** Every leaf predicate evaluates to 0/1 for every row
//!    (nullable columns are wrapped with `IFNULL` / explicit `IS [NOT] NULL`), so
//!    negation is a single uniform, NULL-safe `Condition::not()`.
//! 3. **Bounded work.** Token count and parenthesis depth are capped so the
//!    public, unauthenticated search route can't be driven into pathological work.

use sea_orm::Value;
use sea_orm::sea_query::{Expr, SimpleExpr};
use sea_orm::Condition;

use crate::error::AppError;

/// Max input length (bytes) — bounds lexer allocation and LIKE-pattern length on
/// this public route, independent of the token count.
const MAX_QUERY_BYTES: usize = 4096;
/// Max tokens in one query — guards the public route against pathological input.
const MAX_TOKENS: usize = 256;
/// Max parenthesis nesting depth.
const MAX_DEPTH: usize = 64;
/// Max distinct/total mana symbols in one `m:` value (bounds the dedup scan).
const MAX_MANA_SYMBOLS: usize = 64;

/// The five MTG colours, in canonical WUBRG order.
const WUBRG: [char; 5] = ['W', 'U', 'B', 'R', 'G'];
/// Rarities low→high; index is the ordinal used by `r>=`/`r<` comparisons.
const RARITIES: [&str; 6] = ["common", "uncommon", "rare", "special", "mythic", "bonus"];

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A query that could not be parsed or that uses an unsupported filter. The
/// `Display` text is user-facing (surfaced verbatim as the 422 body).
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("search query ended unexpectedly")]
    UnexpectedEof,
    #[error("unexpected '{0}' in search query")]
    UnexpectedToken(String),
    #[error("unbalanced parentheses in search query")]
    UnbalancedParen,
    #[error("unterminated quoted text in search query")]
    UnterminatedString,
    #[error("empty parentheses in search query")]
    EmptyGroup,
    #[error("a search operator is missing its field name")]
    MissingKey,
    #[error("filter '{key}' is missing a value after '{op}'")]
    MissingValue { key: String, op: Op },
    #[error("unknown search filter '{0}'")]
    UnknownKey(String),
    #[error("search filter '{0}' is not supported")]
    UnsupportedKey(String),
    #[error("filter '{key}' does not support the '{op}' operator")]
    UnsupportedOperator { key: String, op: Op },
    #[error("invalid value '{value}' for '{key}': {reason}")]
    InvalidValue { key: String, value: String, reason: String },
    #[error("search query is too complex")]
    TooComplex,
}

impl From<SearchError> for AppError {
    fn from(err: SearchError) -> Self {
        AppError::Validation(err.to_string())
    }
}

fn invalid(key: &str, value: &str, reason: &str) -> SearchError {
    SearchError::InvalidValue { key: key.to_string(), value: value.to_string(), reason: reason.to_string() }
}

fn unsupported_op(key: &str, op: Op) -> SearchError {
    SearchError::UnsupportedOperator { key: key.to_string(), op }
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/// Comparison operator attached to a `key<op>value` filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Colon,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Op::Colon => ":",
            Op::Eq => "=",
            Op::Ne => "!=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Lt => "<",
            Op::Le => "<=",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    LParen,
    RParen,
    Or,
    And,
    Not,
    Filter { key: String, op: Op, value: String },
    Word(String),
    Phrase(String),
    Exact(String),
}

fn describe(token: &Token) -> String {
    match token {
        Token::LParen => "(".to_string(),
        Token::RParen => ")".to_string(),
        Token::Or => "or".to_string(),
        Token::And => "and".to_string(),
        Token::Not => "-".to_string(),
        Token::Filter { key, .. } => key.clone(),
        Token::Word(s) | Token::Phrase(s) | Token::Exact(s) => s.clone(),
    }
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

fn lex(input: &str) -> Result<Vec<Token>, SearchError> {
    if input.len() > MAX_QUERY_BYTES {
        return Err(SearchError::TooComplex);
    }
    let chars: Vec<char> = input.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut tokens = Vec::new();

    while i < n {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '"' => {
                let (s, end) = read_quoted(&chars, i)?;
                tokens.push(Token::Phrase(s));
                i = end;
            }
            '!' => {
                // Exact full-name match: value is a following quote or bareword.
                let p = i + 1;
                if p < n && chars[p] == '"' {
                    let (s, end) = read_quoted(&chars, p)?;
                    tokens.push(Token::Exact(s));
                    i = end;
                } else if p < n && !chars[p].is_whitespace() && chars[p] != '(' && chars[p] != ')' {
                    let (s, end) = read_bareword(&chars, p);
                    tokens.push(Token::Exact(s));
                    i = end;
                } else {
                    tokens.push(Token::Word("!".to_string()));
                    i += 1;
                }
            }
            '-' => {
                // Negation only when glued to the next term; a lone '-' is literal.
                let p = i + 1;
                if p < n && !chars[p].is_whitespace() && chars[p] != ')' {
                    tokens.push(Token::Not);
                    i += 1;
                } else {
                    let (s, end) = read_bareword(&chars, i);
                    tokens.push(Token::Word(s));
                    i = end;
                }
            }
            ':' | '=' | '<' | '>' => return Err(SearchError::MissingKey),
            _ => {
                // A filter is `letters <op> value`; otherwise a bare word / and / or.
                let mut j = i;
                while j < n && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                if j > i
                    && let Some((op, oplen)) = match_op(&chars, j)
                {
                    let key = chars[i..j].iter().collect::<String>().to_lowercase();
                    let (value, end) = read_value(&chars, j + oplen)?;
                    if value.is_empty() {
                        return Err(SearchError::MissingValue { key, op });
                    }
                    tokens.push(Token::Filter { key, op, value });
                    i = end;
                    if tokens.len() > MAX_TOKENS {
                        return Err(SearchError::TooComplex);
                    }
                    continue;
                }
                let (word, end) = read_bareword(&chars, i);
                i = end;
                if word.eq_ignore_ascii_case("or") {
                    tokens.push(Token::Or);
                } else if word.eq_ignore_ascii_case("and") {
                    tokens.push(Token::And);
                } else {
                    tokens.push(Token::Word(word));
                }
            }
        }
        if tokens.len() > MAX_TOKENS {
            return Err(SearchError::TooComplex);
        }
    }

    Ok(tokens)
}

/// Longest-match an operator at `j` (`>=`/`<=`/`!=` beat the single chars).
fn match_op(chars: &[char], j: usize) -> Option<(Op, usize)> {
    let n = chars.len();
    if j >= n {
        return None;
    }
    match chars[j] {
        ':' => Some((Op::Colon, 1)),
        '=' => Some((Op::Eq, 1)),
        '!' if j + 1 < n && chars[j + 1] == '=' => Some((Op::Ne, 2)),
        '>' if j + 1 < n && chars[j + 1] == '=' => Some((Op::Ge, 2)),
        '>' => Some((Op::Gt, 1)),
        '<' if j + 1 < n && chars[j + 1] == '=' => Some((Op::Le, 2)),
        '<' => Some((Op::Lt, 1)),
        _ => None,
    }
}

/// Read a run up to the next whitespace or parenthesis.
fn read_bareword(chars: &[char], start: usize) -> (String, usize) {
    let n = chars.len();
    let mut i = start;
    let mut s = String::new();
    while i < n {
        let c = chars[i];
        if c.is_whitespace() || c == '(' || c == ')' {
            break;
        }
        s.push(c);
        i += 1;
    }
    (s, i)
}

/// A value following an operator: a quoted phrase (spaces preserved) or a bareword.
fn read_value(chars: &[char], start: usize) -> Result<(String, usize), SearchError> {
    if start < chars.len() && chars[start] == '"' {
        read_quoted(chars, start)
    } else {
        Ok(read_bareword(chars, start))
    }
}

/// Read a `"`-delimited string starting at the opening quote. `\"`→`"`, `\\`→`\`.
fn read_quoted(chars: &[char], start: usize) -> Result<(String, usize), SearchError> {
    let n = chars.len();
    let mut i = start + 1;
    let mut s = String::new();
    while i < n {
        let c = chars[i];
        if c == '"' {
            return Ok((s, i + 1));
        }
        if c == '\\' && i + 1 < n {
            let next = chars[i + 1];
            if next == '"' || next == '\\' {
                s.push(next);
                i += 2;
                continue;
            }
        }
        s.push(c);
        i += 1;
    }
    Err(SearchError::UnterminatedString)
}

// ---------------------------------------------------------------------------
// Parser (recursive descent: OR < AND < NOT < primary)
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Node {
    And(Vec<Node>),
    Or(Vec<Node>),
    Not(Box<Node>),
    Leaf(Leaf),
}

#[derive(Debug)]
enum Leaf {
    Name(String),
    ExactName(String),
    Filter { key: String, op: Op, value: String },
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0, depth: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_query(&mut self) -> Result<Node, SearchError> {
        let node = self.or_expr()?;
        if self.pos < self.tokens.len() {
            return Err(SearchError::UnexpectedToken(describe(&self.tokens[self.pos])));
        }
        Ok(node)
    }

    fn or_expr(&mut self) -> Result<Node, SearchError> {
        let mut parts = vec![self.and_expr()?];
        while matches!(self.peek(), Some(Token::Or)) {
            self.bump();
            parts.push(self.and_expr()?);
        }
        Ok(if parts.len() == 1 { parts.pop().unwrap() } else { Node::Or(parts) })
    }

    fn and_expr(&mut self) -> Result<Node, SearchError> {
        let mut parts = vec![self.unary()?];
        loop {
            match self.peek() {
                Some(Token::And) => {
                    self.bump();
                    parts.push(self.unary()?);
                }
                Some(t) if starts_primary(t) => parts.push(self.unary()?),
                _ => break,
            }
        }
        Ok(if parts.len() == 1 { parts.pop().unwrap() } else { Node::And(parts) })
    }

    fn unary(&mut self) -> Result<Node, SearchError> {
        let mut negate = false;
        while matches!(self.peek(), Some(Token::Not)) {
            self.bump();
            negate = !negate;
        }
        let node = self.primary()?;
        Ok(if negate { Node::Not(Box::new(node)) } else { node })
    }

    fn primary(&mut self) -> Result<Node, SearchError> {
        match self.peek() {
            Some(Token::LParen) => {
                self.bump();
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err(SearchError::TooComplex);
                }
                if matches!(self.peek(), Some(Token::RParen)) {
                    return Err(SearchError::EmptyGroup);
                }
                let inner = self.or_expr()?;
                if !matches!(self.peek(), Some(Token::RParen)) {
                    return Err(SearchError::UnbalancedParen);
                }
                self.bump();
                self.depth -= 1;
                Ok(inner)
            }
            Some(Token::Filter { .. }) => {
                let Some(Token::Filter { key, op, value }) = self.bump() else { unreachable!() };
                Ok(Node::Leaf(Leaf::Filter { key, op, value }))
            }
            Some(Token::Word(_)) => {
                let Some(Token::Word(s)) = self.bump() else { unreachable!() };
                Ok(Node::Leaf(Leaf::Name(s)))
            }
            Some(Token::Phrase(_)) => {
                let Some(Token::Phrase(s)) = self.bump() else { unreachable!() };
                Ok(Node::Leaf(Leaf::Name(s)))
            }
            Some(Token::Exact(_)) => {
                let Some(Token::Exact(s)) = self.bump() else { unreachable!() };
                Ok(Node::Leaf(Leaf::ExactName(s)))
            }
            Some(other) => Err(SearchError::UnexpectedToken(describe(other))),
            None => Err(SearchError::UnexpectedEof),
        }
    }
}

fn starts_primary(t: &Token) -> bool {
    matches!(
        t,
        Token::LParen
            | Token::Not
            | Token::Filter { .. }
            | Token::Word(_)
            | Token::Phrase(_)
            | Token::Exact(_)
    )
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Parse a Scryfall-style query into a `Condition`. An empty / whitespace-only
/// query yields `Condition::all()` (no filter).
pub fn parse(input: &str) -> Result<Condition, SearchError> {
    let tokens = lex(input)?;
    if tokens.is_empty() {
        return Ok(Condition::all());
    }
    let node = Parser::new(tokens).parse_query()?;
    compile(&node)
}

// ---------------------------------------------------------------------------
// Compile AST → Condition
// ---------------------------------------------------------------------------

fn compile(node: &Node) -> Result<Condition, SearchError> {
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
        | "border" | "stamp" | "st" | "settype" | "block" | "b" | "in" | "prints" | "sets"
        | "papersets" | "cube" | "function" | "oracletag" | "otag" | "art" | "arttag" | "atag"
        | "order" | "direction" | "unique" | "display" | "prefer" | "produces" | "devotion"
        | "cheapest" | "has" | "new" | "old" => Err(SearchError::UnsupportedKey(key.to_string())),
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
        Op::Colon | Op::Eq => Ok(cond_one(contains(col, value))),
        _ => Err(unsupported_op(key, op)),
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
    let n: f64 = value.parse().map_err(|_| invalid("cmc", value, "expected a number"))?;
    let sql = format!("(cmc IS NOT NULL AND cmc {} ?)", cmp_sql(op));
    Ok(cond_one(Expr::cust_with_values(sql, [n])))
}

// ----- power / toughness / loyalty (text columns, can be *, 1+*, X) -----

fn stat_column(s: &str) -> Option<&'static str> {
    match s {
        "pow" | "power" => Some("power"),
        "tou" | "toughness" => Some("toughness"),
        "loy" | "loyalty" => Some("loyalty"),
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
        Op::Colon | Op::Eq => {
            Ok(cond_one(Expr::cust_with_values(format!("IFNULL({col}, '') = ?"), [value.to_string()])))
        }
        Op::Ne => Ok(cond_one(Expr::cust_with_values(
            format!("({col} IS NULL OR {col} <> ?)"),
            [value.to_string()],
        ))),
        Op::Gt | Op::Ge | Op::Lt | Op::Le => {
            let n: f64 = value.parse().map_err(|_| invalid(key, value, "expected a number"))?;
            let sql = format!("(({}) AND CAST({col} AS REAL) {} ?)", numeric_guard(col), cmp_sql(op));
            Ok(cond_one(Expr::cust_with_values(sql, [n])))
        }
    }
}

// ----- prices (text decimal columns) -----

fn price(col: &str, key: &str, op: Op, value: &str) -> Result<Condition, SearchError> {
    let n: f64 = value.parse().map_err(|_| invalid(key, value, "expected a number"))?;
    let sql = format!("({col} IS NOT NULL AND {col} <> '' AND CAST({col} AS REAL) {} ?)", cmp_sql(op));
    Ok(cond_one(Expr::cust_with_values(sql, [n])))
}

// ----- release year / date -----

fn year(op: Op, value: &str) -> Result<Condition, SearchError> {
    if value.len() != 4 || !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid("year", value, "expected a 4-digit year"));
    }
    let y: i32 = value.parse().unwrap();
    let sql =
        format!("(released_at IS NOT NULL AND CAST(substr(released_at, 1, 4) AS INTEGER) {} ?)", cmp_sql(op));
    Ok(cond_one(Expr::cust_with_values(sql, [y])))
}

fn is_iso_date(v: &str) -> bool {
    let b = v.as_bytes();
    v.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && v.char_indices().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn date(op: Op, value: &str) -> Result<Condition, SearchError> {
    if is_iso_date(value) {
        let sql = format!("(released_at IS NOT NULL AND released_at {} ?)", cmp_sql(op));
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
    Err(invalid("date", value, "expected a date (YYYY-MM-DD) or a year"))
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
        Op::Colon | Op::Eq => {
            Ok(cond_one(Expr::cust_with_values("IFNULL(rarity, '') = ?".to_string(), [name.to_string()])))
        }
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
            Ok(cond_one(Expr::cust_with_values(format!("IFNULL(rarity, '') IN ({placeholders})"), names)))
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
        Op::Colon | Op::Eq => {
            Ok(cond_one(Expr::cust_with_values("set_code = ?".to_string(), [value.to_lowercase()])))
        }
        Op::Ne => Ok(cond_one(Expr::cust_with_values("set_code <> ?".to_string(), [value.to_lowercase()]))),
        _ => Err(unsupported_op("set", op)),
    }
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
            let sql =
                format!("(collector_number_int IS NOT NULL AND collector_number_int {} ?)", cmp_sql(op));
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
            Ok(cond_one(Expr::cust_with_values("lang = ?".to_string(), [lang_code(&lower)])))
        }
        _ => Err(unsupported_op("lang", op)),
    }
}

fn layout(op: Op, value: &str) -> Result<Condition, SearchError> {
    match op {
        Op::Colon | Op::Eq => {
            Ok(cond_one(Expr::cust_with_values("IFNULL(layout, '') = ?".to_string(), [value.to_lowercase()])))
        }
        _ => Err(unsupported_op("layout", op)),
    }
}

// ----- is: / not: predicates -----

fn is_predicate(value: &str, negated: bool) -> Result<Condition, SearchError> {
    let v = value.to_lowercase();
    let positive: Condition = match v.as_str() {
        "split" | "flip" | "transform" | "meld" | "saga" | "leveler" | "adventure" | "emblem"
        | "class" | "case" | "battle" | "planar" | "scheme" | "vanguard" | "mutate"
        | "prototype" | "augment" | "host" | "normal" => {
            cond_one(Expr::cust_with_values("IFNULL(layout, '') = ?".to_string(), [v.clone()]))
        }
        "mdfc" | "modaldfc" | "modal_dfc" => cond_one(Expr::cust("IFNULL(layout, '') = 'modal_dfc'")),
        "dfc" | "doublefaced" | "double_faced" => {
            cond_one(Expr::cust("IFNULL(layout, '') IN ('transform', 'modal_dfc', 'meld', 'reversible_card')"))
        }
        "token" => cond_one(Expr::cust("IFNULL(layout, '') IN ('token', 'double_faced_token')")),
        "colorless" => cond_one(Expr::cust("colors IS NULL")),
        "multicolored" | "multicolor" => {
            cond_one(Expr::cust("colors IS NOT NULL AND colors LIKE '%,%'"))
        }
        "monocolored" | "monocolor" => {
            cond_one(Expr::cust("colors IS NOT NULL AND colors NOT LIKE '%,%'"))
        }
        "phyrexian" => cond_one(Expr::cust("IFNULL(mana_cost, '') LIKE '%/P}%'")),
        "hybrid" => {
            cond_one(Expr::cust("IFNULL(mana_cost, '') LIKE '%/%' AND IFNULL(mana_cost, '') NOT LIKE '%/P}%'"))
        }
        "digital" => cond_one(Expr::cust("1 = 0")),
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
        Op::Colon | Op::Eq => {
            Ok(cond_one(Expr::cust_with_values("IFNULL(oracle_id, '') = ?".to_string(), [value.to_string()])))
        }
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
        let n: i64 = lower.parse().map_err(|_| invalid(key, value, "expected a number"))?;
        return Ok(ColorOperand::Count(n));
    }
    match lower.as_str() {
        "c" | "colorless" => return Ok(ColorOperand::Colorless),
        "m" | "multi" | "multicolor" | "multicolored" => return Ok(ColorOperand::Multicolor),
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
    q.iter().fold(Condition::all(), |cond, &x| cond.add(has(col, x)))
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
            Op::Colon | Op::Ge | Op::Eq => {
                Ok(cond_one(Expr::cust(format!("IFNULL({col}, '') LIKE '%,%'"))))
            }
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
                let extra = comp.iter().fold(Condition::any(), |c, &x| c.add(has(col, x)));
                cond = cond.add(extra);
            }
            cond
        }
        Op::Le => comp.iter().fold(Condition::all(), |c, &x| c.add(lacks(col, x))),
        Op::Lt => {
            let subset = comp.iter().fold(Condition::all(), |c, &x| c.add(lacks(col, x)));
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
            return Err(invalid("mana", value, &format!("unexpected '{c}' in mana cost")));
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
        // Exact multiset: contains every symbol with its multiplicity AND has no
        // others (total symbol count equal). Comparing the symbol multiset rather
        // than a concatenated string makes `=` order-independent, matching Scryfall
        // (e.g. `m=WW2` and `m=2WW` behave the same against canonical `mana_cost`).
        Op::Eq => {
            let total = tokens.len() as i64;
            let cond = mana_contains(&tokens).add(Expr::cust_with_values(
                "(LENGTH(IFNULL(mana_cost, '')) - LENGTH(REPLACE(IFNULL(mana_cost, ''), '}', ''))) = ?"
                    .to_string(),
                [total],
            ));
            Ok(cond)
        }
        _ => Err(unsupported_op("mana", op)),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::sea_query::{Alias, Query, SqliteQueryBuilder};

    /// Render a parsed query's WHERE clause to inlined SQLite SQL for assertions.
    fn sql(input: &str) -> String {
        let cond = parse(input).expect("query should parse");
        Query::select()
            .expr(Expr::val(1))
            .from(Alias::new("cards"))
            .cond_where(cond)
            .to_string(SqliteQueryBuilder)
    }

    #[test]
    fn empty_query_matches_everything() {
        // Empty/whitespace queries impose no column predicate (a trivial match-all).
        for q in ["", "   "] {
            let s = sql(q);
            assert!(!s.contains("LIKE"), "{q:?} -> {s}");
            assert!(!s.contains("IFNULL"), "{q:?} -> {s}");
        }
    }

    #[test]
    fn bare_word_is_name_substring() {
        assert!(sql("bolt").contains("LIKE '%bolt%'"));
    }

    #[test]
    fn multiple_words_and_together() {
        let s = sql("lightning bolt");
        assert!(s.contains("LIKE '%lightning%'"));
        assert!(s.contains("LIKE '%bolt%'"));
        assert!(s.contains("AND"));
    }

    #[test]
    fn quoted_phrase_is_one_term() {
        assert!(sql("\"lightning bolt\"").contains("LIKE '%lightning bolt%'"));
    }

    #[test]
    fn like_wildcards_are_escaped() {
        assert!(sql("50%").contains("LIKE '%50\\%%'"));
        assert!(sql("a_b").contains("LIKE '%a\\_b%'"));
    }

    #[test]
    fn exact_name_has_no_surrounding_wildcards() {
        let s = sql("!\"Lightning Bolt\"");
        assert!(s.contains("LIKE 'Lightning Bolt'"));
        assert!(!s.contains("%Lightning Bolt%"));
    }

    #[test]
    fn type_and_oracle_substring() {
        assert!(sql("t:creature").contains("IFNULL(type_line, '') LIKE '%creature%'"));
        assert!(sql("o:flying").contains("IFNULL(oracle_text, '') LIKE '%flying%'"));
    }

    #[test]
    fn color_at_least_uses_has() {
        let s = sql("c:r");
        assert!(s.contains("|| IFNULL(colors, '') ||"));
        assert!(s.contains("LIKE '%,R,%'"));
    }

    #[test]
    fn color_exact_has_and_lacks() {
        let s = sql("c=rw");
        assert!(s.contains("LIKE '%,R,%'"));
        assert!(s.contains("LIKE '%,W,%'"));
        assert!(s.contains("NOT LIKE '%,U,%'"));
        assert!(s.contains("NOT LIKE '%,B,%'"));
        assert!(s.contains("NOT LIKE '%,G,%'"));
    }

    #[test]
    fn color_subset_only_lacks_complement() {
        let s = sql("c<=uw");
        assert!(s.contains("NOT LIKE '%,B,%'"));
        assert!(s.contains("NOT LIKE '%,R,%'"));
        assert!(s.contains("NOT LIKE '%,G,%'"));
        assert!(!s.contains(" LIKE '%,W,%'")); // no positive has() for a subset query
    }

    #[test]
    fn nickname_resolves_to_letters() {
        let s = sql("c>=esper");
        assert!(s.contains("LIKE '%,W,%'"));
        assert!(s.contains("LIKE '%,U,%'"));
        assert!(s.contains("LIKE '%,B,%'"));
    }

    #[test]
    fn colorless_and_multicolor_tokens() {
        assert!(sql("c:c").contains("colors IS NULL"));
        assert!(sql("c!=c").contains("colors IS NOT NULL"));
        assert!(sql("c:m").contains("IFNULL(colors, '') LIKE '%,%'"));
    }

    #[test]
    fn color_count() {
        assert!(sql("c=3").contains("REPLACE(colors, ',', '')"));
    }

    #[test]
    fn identity_uses_its_column() {
        assert!(sql("id:r").contains("IFNULL(color_identity, '') ||"));
        assert!(sql("id<=wu").contains("IFNULL(color_identity, '') ||"));
    }

    #[test]
    fn mana_value_numeric() {
        assert!(sql("mv>=3").contains("cmc >= 3"));
        assert!(sql("cmc:3").contains("cmc = 3"));
        assert!(sql("mv:even").contains("% 2 = 0"));
    }

    #[test]
    fn power_text_and_range() {
        assert!(sql("pow=*").contains("IFNULL(power, '') = '*'"));
        let r = sql("pow>=5");
        assert!(r.contains("GLOB '[0-9]*'"));
        assert!(r.contains("CAST(power AS REAL) >= 5"));
    }

    #[test]
    fn power_cross_column() {
        let s = sql("pow>tou");
        assert!(s.contains("CAST(power AS REAL) > CAST(toughness AS REAL)"));
    }

    #[test]
    fn prices_cast() {
        assert!(sql("usd<1").contains("CAST(price_usd AS REAL) < 1"));
        assert!(sql("tix<=0.25").contains("CAST(price_tix AS REAL) <= 0.25"));
    }

    #[test]
    fn year_and_date() {
        assert!(sql("year<=2010").contains("CAST(substr(released_at, 1, 4) AS INTEGER) <= 2010"));
        assert!(sql("date>=2015-01-01").contains("released_at >= '2015-01-01'"));
        assert!(sql("date<2018").contains("released_at < '2018-01-01'"));
        assert!(sql("date=2019").contains("released_at LIKE '2019-%'"));
    }

    #[test]
    fn rarity_eq_and_ordered() {
        assert!(sql("r:mythic").contains("IFNULL(rarity, '') = 'mythic'"));
        let s = sql("r>=rare");
        assert!(s.contains("IN ('rare', 'special', 'mythic', 'bonus')"));
        assert!(sql("r<uncommon").contains("IN ('common')"));
    }

    #[test]
    fn set_and_collector_number() {
        assert!(sql("e:DOM").contains("set_code = 'dom'"));
        assert!(sql("cn:12a").contains("lower(collector_number) = '12a'"));
        assert!(sql("cn>=250").contains("collector_number_int >= 250"));
    }

    #[test]
    fn lang_any_is_no_filter() {
        assert!(!sql("lang:any").contains("lang ="));
        assert!(sql("lang:japanese").contains("lang = 'ja'"));
    }

    #[test]
    fn is_predicates() {
        assert!(sql("is:split").contains("IFNULL(layout, '') = 'split'"));
        assert!(sql("is:dfc").contains("IN ('transform', 'modal_dfc', 'meld', 'reversible_card')"));
        assert!(sql("is:colorless").contains("colors IS NULL"));
        assert!(sql("is:phyrexian").contains("LIKE '%/P}%'"));
    }

    #[test]
    fn negation_is_not_wrapped() {
        assert!(sql("-t:land").contains("NOT"));
        assert!(sql("not:transform").contains("NOT"));
    }

    #[test]
    fn boolean_precedence() {
        // a or b c  ==  a OR (b AND c)
        let s = sql("a or b c");
        assert!(s.contains("OR"));
        assert!(s.contains("AND"));
    }

    #[test]
    fn grouping_with_parens() {
        let s = sql("(c:r or c:u) t:instant");
        assert!(s.contains("OR"));
        assert!(s.contains("IFNULL(type_line, '') LIKE '%instant%'"));
    }

    #[test]
    fn case_insensitive_keyword_and_value() {
        assert_eq!(sql("C:R"), sql("c:r"));
    }

    fn err(input: &str) -> SearchError {
        parse(input).expect_err("should be an error")
    }

    #[test]
    fn error_cases() {
        assert!(matches!(err("foo:bar"), SearchError::UnknownKey(_)));
        assert!(matches!(err("kw:flying"), SearchError::UnsupportedKey(_)));
        assert!(matches!(err("f:modern"), SearchError::UnsupportedKey(_)));
        assert!(matches!(err("is:reprint"), SearchError::UnsupportedKey(_)));
        assert!(matches!(err("set>dom"), SearchError::UnsupportedOperator { .. }));
        assert!(matches!(err("mana<=2"), SearchError::UnsupportedOperator { .. }));
        assert!(matches!(err("t:"), SearchError::MissingValue { .. }));
        assert!(matches!(err("cmc>=x"), SearchError::InvalidValue { .. }));
        assert!(matches!(err("c:x"), SearchError::InvalidValue { .. }));
        assert!(matches!(err("cn>=12a"), SearchError::InvalidValue { .. }));
        assert!(matches!(err("r:legendary"), SearchError::InvalidValue { .. }));
        assert!(matches!(err(">=3"), SearchError::MissingKey));
        assert!(matches!(err("()"), SearchError::EmptyGroup));
        assert!(matches!(err("(c:r or c:u"), SearchError::UnbalancedParen));
        assert!(matches!(err("a)"), SearchError::UnexpectedToken(_)));
        assert!(matches!(err("a or"), SearchError::UnexpectedEof));
        assert!(matches!(err("\"abc"), SearchError::UnterminatedString));
    }

    #[test]
    fn too_many_tokens_is_rejected() {
        let big = "a ".repeat(MAX_TOKENS + 10);
        assert!(matches!(parse(&big), Err(SearchError::TooComplex)));
    }

    #[test]
    fn mana_containment_with_multiplicity() {
        let s = sql("m:2WW");
        assert!(s.contains("REPLACE(IFNULL(mana_cost, ''), '{2}', '')"));
        assert!(s.contains("REPLACE(IFNULL(mana_cost, ''), '{W}', '')"));
        // {W} appears twice -> threshold 2 * len('{W}') = 6
        assert!(s.contains(">= 6"));
    }

    #[test]
    fn mana_hybrid_normalized() {
        assert!(sql("m:{u/w}").contains("{W/U}"));
    }

    #[test]
    fn mana_exact_is_order_independent_multiset() {
        let s = sql("mana=2WW");
        // Exact = containment (per symbol) + equal total symbol count (3 symbols).
        assert!(s.contains("'}', ''))) = 3"), "{s}");
        assert!(s.contains("'{W}', ''))) >= 6"), "{s}");
        // Not the old order-sensitive string-equality form.
        assert!(!s.contains("= '{2}{W}{W}'"), "{s}");
    }

    #[test]
    fn cmc_parity_rejects_relational_operator() {
        assert!(matches!(parse("mv>even"), Err(SearchError::UnsupportedOperator { .. })));
        assert!(sql("mv:even").contains("% 2 = 0"));
    }

    #[test]
    fn oversized_query_is_rejected() {
        let big = "a".repeat(MAX_QUERY_BYTES + 1);
        assert!(matches!(parse(&big), Err(SearchError::TooComplex)));
    }

    #[test]
    fn too_many_mana_symbols_rejected() {
        let q = format!("m:{}", "{W}".repeat(MAX_MANA_SYMBOLS + 1));
        assert!(matches!(parse(&q), Err(SearchError::InvalidValue { .. })));
    }
}
